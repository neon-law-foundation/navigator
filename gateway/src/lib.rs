//! The private-mode edge gateway's decision core.
//!
//! The Pingora sidecar runs in the `navigator-web` pod. `/health` remains
//! open for Kubernetes and load-balancer probes; every other request must
//! first come from an allowed network and then carry the shared basic-auth
//! credential. Keeping that policy independent of Pingora makes it auditable
//! and unit-testable.

pub mod proxy;

use std::net::{IpAddr, SocketAddr};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ipnet::IpNet;
use sha2::{Digest, Sha256};

const AFFIRMATIVE: [&str; 4] = ["1", "true", "yes", "on"];

/// Resolved startup configuration for the gateway.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub listen: String,
    pub upstream: SocketAddr,
    pub allowed_nets: Vec<IpNet>,
    pub username: String,
    pub password: String,
    pub trust_forwarded_for: bool,
}

/// A configuration error which is safe to show without leaking credentials.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{name} is not set and has no default; refusing to guess a {what}")]
    Missing {
        name: &'static str,
        what: &'static str,
    },
    #[error("{name} value {value:?} is not a valid {what}")]
    Invalid {
        name: &'static str,
        value: String,
        what: &'static str,
    },
}

impl GatewayConfig {
    /// Read the gateway settings from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_getter(|name| std::env::var(name).ok())
    }

    /// Read settings through an injected environment getter.
    pub fn from_getter(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let listen = get("GATEWAY_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into());
        let upstream_raw = get("GATEWAY_UPSTREAM").unwrap_or_else(|| "127.0.0.1:3001".into());
        let upstream = upstream_raw.parse().map_err(|_| ConfigError::Invalid {
            name: "GATEWAY_UPSTREAM",
            value: upstream_raw,
            what: "socket address",
        })?;
        let allowed_nets_raw = get("GATEWAY_ALLOWED_NETS").ok_or(ConfigError::Missing {
            name: "GATEWAY_ALLOWED_NETS",
            what: "client allowlist",
        })?;
        let allowed_nets = allowed_nets_raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                entry.parse().map_err(|_| ConfigError::Invalid {
                    name: "GATEWAY_ALLOWED_NETS",
                    value: entry.to_owned(),
                    what: "CIDR network",
                })
            })
            .collect::<Result<Vec<IpNet>, _>>()?;
        if allowed_nets.is_empty() {
            return Err(ConfigError::Missing {
                name: "GATEWAY_ALLOWED_NETS",
                what: "client allowlist",
            });
        }
        let credential = get("GATEWAY_BASIC_AUTH").ok_or(ConfigError::Missing {
            name: "GATEWAY_BASIC_AUTH",
            what: "basic-auth credential",
        })?;
        let Some((username, password)) = credential.split_once(':') else {
            return Err(invalid_credential());
        };
        if username.is_empty() || password.is_empty() {
            return Err(invalid_credential());
        }
        let trust_forwarded_for = get("GATEWAY_TRUST_XFF")
            .is_some_and(|value| AFFIRMATIVE.contains(&value.trim().to_ascii_lowercase().as_str()));
        Ok(Self {
            listen,
            upstream,
            allowed_nets,
            username: username.to_owned(),
            password: password.to_owned(),
            trust_forwarded_for,
        })
    }
}

fn invalid_credential() -> ConfigError {
    ConfigError::Invalid {
        name: "GATEWAY_BASIC_AUTH",
        value: "<redacted>".into(),
        what: "user:password pair",
    }
}

/// The outcome of applying the private-mode request policy.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Proxy,
    Unauthorized,
    Forbidden,
}

/// Apply the ordered private-mode policy to one request.
#[must_use]
pub fn decide(
    config: &GatewayConfig,
    path: &str,
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
    authorization: Option<&str>,
) -> Decision {
    if path == "/health" {
        return Decision::Proxy;
    }
    let Some(client) = client_ip(config, peer, forwarded_for) else {
        return Decision::Forbidden;
    };
    if !config
        .allowed_nets
        .iter()
        .any(|network| network.contains(&client))
    {
        return Decision::Forbidden;
    }
    if !credential_matches(config, authorization) {
        return Decision::Unauthorized;
    }
    Decision::Proxy
}

fn client_ip(
    config: &GatewayConfig,
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
) -> Option<IpAddr> {
    if !config.trust_forwarded_for {
        return peer;
    }
    let Some(forwarded_for) = forwarded_for else {
        return peer;
    };
    let entries = forwarded_for.split(',').map(str::trim).collect::<Vec<_>>();
    let client = if entries.len() >= 2 {
        entries[entries.len() - 2]
    } else {
        entries.first().copied()?
    };
    client.parse().ok()
}

fn credential_matches(config: &GatewayConfig, authorization: Option<&str>) -> bool {
    let Some(value) = authorization else {
        return false;
    };
    let Some((scheme, encoded)) = value.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    let Ok(decoded) = BASE64.decode(encoded.trim()) else {
        return false;
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return false;
    };
    let Some((username, password)) = text.split_once(':') else {
        return false;
    };
    digest_matches(username, &config.username) & digest_matches(password, &config.password)
}

fn digest_matches(provided: &str, expected: &str) -> bool {
    Sha256::digest(provided.as_bytes()) == Sha256::digest(expected.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GatewayConfig {
        GatewayConfig::from_getter(|name| match name {
            "GATEWAY_ALLOWED_NETS" => Some("127.0.0.0/8,10.0.0.0/8".into()),
            "GATEWAY_BASIC_AUTH" => Some("go:bears".into()),
            _ => None,
        })
        .expect("configuration resolves")
    }

    /// Build the `Authorization` header the resolved configuration accepts.
    fn basic(config: &GatewayConfig) -> String {
        let credential = format!("{}:{}", config.username, config.password);
        format!("Basic {}", BASE64.encode(credential))
    }

    #[test]
    fn health_is_open_but_other_requests_need_an_allowed_network_and_credential() {
        let config = config();
        assert_eq!(
            decide(&config, "/health", None, None, None),
            Decision::Proxy
        );
        assert_eq!(
            decide(
                &config,
                "/",
                Some("203.0.113.1".parse().unwrap()),
                None,
                None
            ),
            Decision::Forbidden
        );
        assert_eq!(
            decide(&config, "/", Some("127.0.0.1".parse().unwrap()), None, None),
            Decision::Unauthorized
        );
        assert_eq!(
            decide(
                &config,
                "/",
                Some("10.1.2.3".parse().unwrap()),
                None,
                Some(&basic(&config))
            ),
            Decision::Proxy
        );
    }

    #[test]
    fn configuration_fails_closed_and_never_echoes_a_bad_credential() {
        let missing =
            GatewayConfig::from_getter(|_| None).expect_err("missing allowlist fails closed");
        assert!(missing.to_string().contains("GATEWAY_ALLOWED_NETS"));
        let credential = GatewayConfig::from_getter(|name| match name {
            "GATEWAY_ALLOWED_NETS" => Some("127.0.0.0/8".into()),
            "GATEWAY_BASIC_AUTH" => Some("not-a-pair".into()),
            _ => None,
        })
        .expect_err("bad credential fails");
        assert!(credential.to_string().contains("<redacted>"));
        assert!(!credential.to_string().contains("not-a-pair"));
    }

    #[test]
    fn forwarded_for_is_opt_in_and_a_malformed_trusted_value_denies() {
        let config = GatewayConfig::from_getter(|name| match name {
            "GATEWAY_ALLOWED_NETS" => Some("127.0.0.0/8".into()),
            "GATEWAY_BASIC_AUTH" => Some("go:bears".into()),
            "GATEWAY_TRUST_XFF" => Some("true".into()),
            _ => None,
        })
        .unwrap();
        let auth = basic(&config);
        assert_eq!(
            decide(
                &config,
                "/",
                Some("203.0.113.4".parse().unwrap()),
                Some("127.0.0.1"),
                Some(&auth)
            ),
            Decision::Proxy
        );
        assert_eq!(
            decide(
                &config,
                "/",
                Some("127.0.0.1".parse().unwrap()),
                Some("not-an-ip"),
                Some(&auth)
            ),
            Decision::Forbidden
        );
    }
}
