//! HTTP-server configuration.
//!
//! The database half (`DbConfig`, `DATABASE_URL`) lives in the
//! `store` crate so non-`web` consumers (`cli`, `mcp`) can build a
//! connection without pulling in the HTTP stack. This module owns
//! only what's HTTP-specific: the TCP port and the composite
//! `AppConfig` that bundles port + db.
//!
//! | Variable             | Default              | Purpose                                       |
//! |----------------------|----------------------|-----------------------------------------------|
//! | `PORT`               | `3001`               | TCP port to bind.                             |
//!
//! `from_lookup` is the testable seam: the production `from_env`
//! is a thin wrapper that delegates to `std::env::var`.

use store::{DbConfig, DbConfigError};
use thiserror::Error;

/// Top-level application configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub port: u16,
    pub db: DbConfig,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("PORT must be a u16, got `{0}`")]
    BadPort(String),
    #[error(transparent)]
    Db(#[from] DbConfigError),
}

/// Failures from [`enforce_prod_invariants`]. Carries the full list
/// of violations so operators can fix the deploy in one pass instead
/// of redeploy-fix-redeploy roulette.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("production invariants violated:\n  - {}", violations.join("\n  - "))]
pub struct ProdInvariantError {
    pub violations: Vec<String>,
}

pub const DEFAULT_PORT: u16 = 3001;

impl AppConfig {
    /// Build an `AppConfig` from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Build an `AppConfig` from any `key -> Option<value>` lookup.
    /// This is the seam tests use to avoid mutating process env vars.
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, ConfigError> {
        let port = match get("PORT") {
            None => DEFAULT_PORT,
            Some(raw) => raw.parse().map_err(|_| ConfigError::BadPort(raw))?,
        };
        let db = DbConfig::from_lookup(&get)?;
        Ok(AppConfig { port, db })
    }
}

/// Enforce environment invariants on the running binary before it
/// starts serving traffic. A passthrough OPA client, an in-memory
/// workflow runtime, or a filesystem storage backend silently weaken
/// policy / durability / persistence — crash with a structured error
/// instead.
///
/// These invariants are unconditional: there is no longer an
/// `APP_ENV=production` switch. Every binary that calls this must
/// provide a Postgres URL, an OPA URL, a Restate broker URL, a GCS
/// storage backend, and SendGrid credentials. The KIND overlay
/// supplies stub values for SendGrid; the `navigator` CLI's host-side env file
/// does the same.
#[allow(clippy::too_many_lines)] // a flat checklist; splitting it hurts readability
pub fn enforce_prod_invariants<F: Fn(&str) -> Option<String>>(
    get: F,
) -> Result<(), ProdInvariantError> {
    let mut violations: Vec<String> = Vec::new();
    if get("RESTATE_BROKER_URL").is_none_or(|s| s.is_empty()) {
        violations.push(
            "RESTATE_BROKER_URL must be set (otherwise the in-memory \
             InMemoryRuntime silently runs and notations are lost on restart)"
                .into(),
        );
    }
    if get("NAVIGATOR_OPA_URL").is_none_or(|s| s.is_empty()) {
        violations.push(
            "NAVIGATOR_OPA_URL must be set (otherwise the PolicyClient \
             falls back to allow-all passthrough mode)"
                .into(),
        );
    }
    match get("NAVIGATOR_STORAGE_BACKEND").as_deref() {
        Some("gcs") => {}
        Some(other) => violations.push(format!(
            "NAVIGATOR_STORAGE_BACKEND must be `gcs`, got `{other}`",
        )),
        None => violations.push(
            "NAVIGATOR_STORAGE_BACKEND must be `gcs` (the default \
             filesystem backend is dev-only)"
                .into(),
        ),
    }
    if get("SENDGRID_API_KEY").is_none_or(|s| s.is_empty()) {
        violations.push(
            "SENDGRID_API_KEY must be set (otherwise outbound email \
             silently falls back to the in-memory CapturingEmail and \
             nothing leaves the pod)"
                .into(),
        );
    }
    if get("SENDGRID_INBOUND_SECRET").is_none_or(|s| s.is_empty()) {
        violations.push(
            "SENDGRID_INBOUND_SECRET must be set (otherwise the \
             /webhook/sendgrid/inbound endpoint accepts any path token and \
             anyone on the internet can POST forged mail into the mailroom)"
                .into(),
        );
    }
    if get("SENDGRID_EVENTS_SECRET").is_none_or(|s| s.is_empty()) {
        violations.push(
            "SENDGRID_EVENTS_SECRET must be set (otherwise the \
             /api/email-events endpoint accepts any path token and anyone on \
             the internet can POST forged delivery events into the lake)"
                .into(),
        );
    }
    if get("SENDGRID_EVENTS_PUBLIC_KEY").is_none_or(|s| s.is_empty()) {
        violations.push(
            "SENDGRID_EVENTS_PUBLIC_KEY must be set (otherwise the \
             /api/email-events endpoint skips ECDSA signature verification and \
             trusts the path secret alone — a leaked URL would let anyone POST \
             forged delivery events into the lake)"
                .into(),
        );
    }
    if get("DOCUSIGN_HMAC_KEY").is_none_or(|s| s.is_empty()) {
        violations.push(
            "DOCUSIGN_HMAC_KEY must be set (otherwise the \
             /webhook/esignature endpoint skips HMAC verification and anyone \
             on the internet can forge a `completed` callback that advances a \
             retainer to END — the firm asserting a client signed when they \
             did not)"
                .into(),
        );
    }
    // The HMAC key that signs every browser session cookie AND every
    // `navigator login` CLI bearer. If unset, `web::SessionStore` falls
    // back to a random key minted fresh on each boot (see `main.rs`), so
    // every pod restart / rollout silently invalidates every active
    // session and forces all users to sign in again. Must also carry the
    // >=32 bytes of entropy the cookie design assumes.
    match get("SESSION_SECRET") {
        Some(s) if s.len() >= 32 => {}
        Some(_) => violations.push(
            "SESSION_SECRET must be at least 32 bytes (a shorter key weakens \
             the HMAC that signs every session cookie + CLI bearer token)"
                .into(),
        ),
        None => violations.push(
            "SESSION_SECRET must be set (otherwise SessionStore falls back to a \
             random per-boot key, so every pod restart / rollout invalidates \
             every active session and forces all users to sign in again)"
                .into(),
        ),
    }
    // Repo provisioning is a hard dependency of matter creation: a `web`
    // process must either mount the repo volume (the single writer) or be
    // able to reach it — otherwise every matter-open surface 503s at
    // request time instead of failing loudly at boot.
    let mounts_repo_volume = get(repos::REPO_ROOT_ENV).is_some_and(|s| !s.is_empty());
    let reaches_the_writer = get(store::projects::GIT_WRITER_URL_ENV)
        .is_some_and(|s| !s.is_empty())
        && get(store::projects::GIT_WRITER_TOKEN_ENV).is_some_and(|s| !s.is_empty());
    if !mounts_repo_volume && !reaches_the_writer {
        violations.push(format!(
            "{root} (mount the repo volume) or {url} + {token} (route to the single mounted \
             writer) must be set — matter creation hard-blocks on repo provisioning and every \
             create surface would 503",
            root = repos::REPO_ROOT_ENV,
            url = store::projects::GIT_WRITER_URL_ENV,
            token = store::projects::GIT_WRITER_TOKEN_ENV,
        ));
    }
    if get("OIDC_DISABLED")
        .as_deref()
        .is_some_and(|v| v == "true" || v == "1")
    {
        violations.push(
            "OIDC_DISABLED must not be `true`/`1` (it turns the bearer-token \
             verifier on /mcp + /api into an open pass-through)"
                .into(),
        );
    }
    // When the RS256/JWKS bearer path is configured, the audience and
    // issuer must be pinned, or a token minted for a *different* client
    // of the same IdP is accepted (token confusion). KIND uses the
    // browser OIDC flow without OIDC_JWKS_URL, so this fires only where
    // the JWKS bearer path is actually in play.
    if get("OIDC_JWKS_URL").is_some_and(|s| !s.is_empty()) {
        if get("OIDC_AUDIENCE").is_none_or(|s| s.is_empty()) {
            violations.push(
                "OIDC_AUDIENCE must be set when OIDC_JWKS_URL is (otherwise \
                 bearer tokens are accepted without audience pinning — a token \
                 for another client of the same IdP would be honored)"
                    .into(),
            );
        }
        if get("OIDC_ISSUER").is_none_or(|s| s.is_empty()) {
            violations.push(
                "OIDC_ISSUER must be set when OIDC_JWKS_URL is (otherwise the \
                 bearer token's issuer is unverified)"
                    .into(),
            );
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ProdInvariantError { violations })
    }
}

#[cfg(test)]
mod tests {
    use super::{enforce_prod_invariants, AppConfig, ConfigError, DEFAULT_PORT};
    use std::collections::HashMap;
    use store::{DbConfig, DbConfigError};

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    /// A 32-byte (32-char ASCII) `SESSION_SECRET` for the happy-path
    /// invariant tests — long enough to clear the length check.
    const SECRET32: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn missing_database_url_is_an_error() {
        let err = AppConfig::from_lookup(|_| None).unwrap_err();
        assert_eq!(err, ConfigError::Db(DbConfigError::MissingDatabaseUrl));
    }

    #[test]
    fn database_url_set_picks_postgres() {
        let cfg =
            AppConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://u:p@host:5432/db")]))
                .unwrap();
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(
            cfg.db,
            DbConfig {
                url: "postgres://u:p@host:5432/db".into()
            }
        );
    }

    #[test]
    fn port_is_parsed_from_env() {
        let cfg = AppConfig::from_lookup(lookup(&[
            ("PORT", "8080"),
            ("DATABASE_URL", "postgres://u:p@host/db"),
        ]))
        .unwrap();
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn invalid_port_is_an_error() {
        let err = AppConfig::from_lookup(lookup(&[
            ("PORT", "not-a-number"),
            ("DATABASE_URL", "postgres://u:p@host/db"),
        ]))
        .unwrap_err();
        assert_eq!(err, ConfigError::BadPort("not-a-number".into()));
    }

    #[test]
    fn port_zero_is_accepted_for_dynamic_binding() {
        let cfg = AppConfig::from_lookup(lookup(&[
            ("PORT", "0"),
            ("DATABASE_URL", "postgres://u:p@host/db"),
        ]))
        .unwrap();
        assert_eq!(cfg.port, 0);
    }

    #[test]
    fn prod_invariants_pass_when_all_set() {
        let result = enforce_prod_invariants(lookup(&[
            ("RESTATE_BROKER_URL", "http://restate:9070"),
            ("NAVIGATOR_OPA_URL", "http://opa:8181"),
            ("NAVIGATOR_STORAGE_BACKEND", "gcs"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_INBOUND_SECRET", "secret"),
            ("SENDGRID_EVENTS_SECRET", "secret"),
            ("SENDGRID_EVENTS_PUBLIC_KEY", "base64-spki"),
            ("DOCUSIGN_HMAC_KEY", "hmac-secret"),
            ("SESSION_SECRET", SECRET32),
            ("NAVIGATOR_GIT_REPO_ROOT", "/var/lib/navigator/git-repos"),
        ]));
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn prod_invariants_accept_the_remote_writer_instead_of_the_volume() {
        // The stateless `web` tier mounts no repo volume; it reaches the
        // single mounted writer instead. Both env vars are required — a
        // URL without the bearer can't make an authorized call.
        let base = [
            ("RESTATE_BROKER_URL", "http://restate:9070"),
            ("NAVIGATOR_OPA_URL", "http://opa:8181"),
            ("NAVIGATOR_STORAGE_BACKEND", "gcs"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_INBOUND_SECRET", "secret"),
            ("SENDGRID_EVENTS_SECRET", "secret"),
            ("SENDGRID_EVENTS_PUBLIC_KEY", "base64-spki"),
            ("DOCUSIGN_HMAC_KEY", "hmac-secret"),
            ("SESSION_SECRET", SECRET32),
        ];

        let mut with_writer = base.to_vec();
        with_writer.push(("NAVIGATOR_GIT_WRITER_URL", "http://navigator-git:3001"));
        with_writer.push(("NAVIGATOR_GIT_WRITER_TOKEN", "t0ken"));
        assert!(enforce_prod_invariants(lookup(&with_writer)).is_ok());

        let mut url_only = base.to_vec();
        url_only.push(("NAVIGATOR_GIT_WRITER_URL", "http://navigator-git:3001"));
        let err = enforce_prod_invariants(lookup(&url_only)).unwrap_err();
        assert_eq!(err.violations.len(), 1);
        assert!(err.violations[0].starts_with("NAVIGATOR_GIT_REPO_ROOT"));
    }

    #[test]
    fn prod_invariants_collect_every_missing_var_at_once() {
        // Operators should not have to fix one var, redeploy, fix
        // the next. Every missing var must surface in a single error.
        let err = enforce_prod_invariants(|_| None).unwrap_err();
        assert_eq!(err.violations.len(), 10);
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("NAVIGATOR_GIT_REPO_ROOT")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("SESSION_SECRET")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("DOCUSIGN_HMAC_KEY")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("SENDGRID_EVENTS_PUBLIC_KEY")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("RESTATE_BROKER_URL")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("NAVIGATOR_OPA_URL")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("NAVIGATOR_STORAGE_BACKEND")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("SENDGRID_API_KEY")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("SENDGRID_INBOUND_SECRET")));
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("SENDGRID_EVENTS_SECRET")));
    }

    /// The full happy-set plus the JWKS bearer path: audience + issuer
    /// pinned, OIDC not disabled.
    fn full_with_jwks() -> Vec<(&'static str, &'static str)> {
        vec![
            ("RESTATE_BROKER_URL", "http://restate:9070"),
            ("NAVIGATOR_OPA_URL", "http://opa:8181"),
            ("NAVIGATOR_STORAGE_BACKEND", "gcs"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_INBOUND_SECRET", "secret"),
            ("SENDGRID_EVENTS_SECRET", "secret"),
            ("SENDGRID_EVENTS_PUBLIC_KEY", "base64-spki"),
            ("DOCUSIGN_HMAC_KEY", "hmac-secret"),
            ("SESSION_SECRET", SECRET32),
            ("NAVIGATOR_GIT_REPO_ROOT", "/var/lib/navigator/git-repos"),
            ("OIDC_JWKS_URL", "https://idp/jwks"),
            ("OIDC_AUDIENCE", "navigator-web"),
            ("OIDC_ISSUER", "https://idp"),
        ]
    }

    #[test]
    fn oidc_disabled_true_is_rejected() {
        let mut pairs = full_with_jwks();
        pairs.push(("OIDC_DISABLED", "true"));
        let err = enforce_prod_invariants(lookup(&pairs)).unwrap_err();
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("OIDC_DISABLED")));
    }

    #[test]
    fn jwks_path_requires_audience_and_issuer() {
        // JWKS set but neither audience nor issuer → two violations.
        let err = enforce_prod_invariants(lookup(&[
            ("RESTATE_BROKER_URL", "http://restate:9070"),
            ("NAVIGATOR_OPA_URL", "http://opa:8181"),
            ("NAVIGATOR_STORAGE_BACKEND", "gcs"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_INBOUND_SECRET", "secret"),
            ("SENDGRID_EVENTS_SECRET", "secret"),
            ("SENDGRID_EVENTS_PUBLIC_KEY", "base64-spki"),
            ("DOCUSIGN_HMAC_KEY", "hmac-secret"),
            ("SESSION_SECRET", SECRET32),
            ("NAVIGATOR_GIT_REPO_ROOT", "/var/lib/navigator/git-repos"),
            ("OIDC_JWKS_URL", "https://idp/jwks"),
        ]))
        .unwrap_err();
        assert!(err
            .violations
            .iter()
            .any(|v| v.starts_with("OIDC_AUDIENCE")));
        assert!(err.violations.iter().any(|v| v.starts_with("OIDC_ISSUER")));
    }

    #[test]
    fn jwks_path_passes_with_audience_and_issuer() {
        assert!(enforce_prod_invariants(lookup(&full_with_jwks())).is_ok());
    }

    #[test]
    fn prod_invariants_reject_filesystem_backend() {
        let err = enforce_prod_invariants(lookup(&[
            ("RESTATE_BROKER_URL", "http://restate:9070"),
            ("NAVIGATOR_OPA_URL", "http://opa:8181"),
            ("NAVIGATOR_STORAGE_BACKEND", "fs"),
            ("SENDGRID_API_KEY", "SG.test"),
            ("SENDGRID_INBOUND_SECRET", "secret"),
            ("SENDGRID_EVENTS_SECRET", "secret"),
            ("SENDGRID_EVENTS_PUBLIC_KEY", "base64-spki"),
            ("DOCUSIGN_HMAC_KEY", "hmac-secret"),
            ("SESSION_SECRET", SECRET32),
            ("NAVIGATOR_GIT_REPO_ROOT", "/var/lib/navigator/git-repos"),
        ]))
        .unwrap_err();
        assert_eq!(err.violations.len(), 1);
        assert!(err.violations[0].contains("got `fs`"));
    }

    #[test]
    fn session_secret_shorter_than_32_bytes_is_rejected() {
        let mut pairs = full_with_jwks();
        // Replace the happy-path 32-byte secret with a too-short one.
        pairs.retain(|(k, _)| *k != "SESSION_SECRET");
        pairs.push(("SESSION_SECRET", "too-short"));
        let err = enforce_prod_invariants(lookup(&pairs)).unwrap_err();
        assert_eq!(err.violations.len(), 1);
        assert!(err.violations[0].starts_with("SESSION_SECRET"));
        assert!(err.violations[0].contains("at least 32 bytes"));
    }
}
