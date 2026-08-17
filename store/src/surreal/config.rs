//! The SurrealDB connection contract, resolved from the environment.
//!
//! This is the store's only connection contract: the `NAVIGATOR_SURREAL_*`
//! coordinates and nothing else. [`crate::config`] keeps the
//! deployment-profile selector, so no other environment variable a process
//! happens to be carrying opens a connection.

use thiserror::Error;

/// Where the engine listens. `ws://localhost:<port>` against the KIND
/// dependency tier locally; the Surreal Cloud endpoint in a deployment.
pub const ENDPOINT_ENV: &str = "NAVIGATOR_SURREAL_ENDPOINT";
/// The namespace to select after connecting.
pub const NAMESPACE_ENV: &str = "NAVIGATOR_SURREAL_NAMESPACE";
/// The database to select after connecting.
pub const DATABASE_ENV: &str = "NAVIGATOR_SURREAL_DATABASE";
/// Sign-in username. Set together with [`PASSWORD_ENV`] or not at all.
pub const USER_ENV: &str = "NAVIGATOR_SURREAL_USER";
/// Sign-in password. Set together with [`USER_ENV`] or not at all.
pub const PASSWORD_ENV: &str = "NAVIGATOR_SURREAL_PASSWORD";
/// Which level [`USER_ENV`] authenticates at: `root` (the default),
/// `namespace`, or `database`.
///
/// A managed engine does not always hand out a root user. Surreal Cloud
/// issues one today, but a namespace- or database-scoped user is the
/// least-privilege shape and some providers offer only that — so the
/// level is configuration, not a compile-time decision.
pub const AUTH_SCOPE_ENV: &str = "NAVIGATOR_SURREAL_AUTH_SCOPE";
/// A pre-issued bearer token, as an alternative to [`USER_ENV`] and
/// [`PASSWORD_ENV`]. Mutually exclusive with them.
///
/// Note a token carries its own expiry. One with a short `exp` is a
/// credential that stops working some time after deployment rather than
/// at deployment, which is the worst failure shape available — prefer a
/// username and password unless the engine issues nothing else.
pub const TOKEN_ENV: &str = "NAVIGATOR_SURREAL_TOKEN";

/// The level a username and password authenticate at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthScope {
    /// A user defined `ON ROOT` — authority over every namespace.
    #[default]
    Root,
    /// A user defined `ON NAMESPACE`, scoped to the configured namespace.
    Namespace,
    /// A user defined `ON DATABASE`, scoped to the configured namespace
    /// and database.
    Database,
}

impl AuthScope {
    /// Parse the configured spelling. Case-insensitive; anything else is
    /// rejected rather than defaulted, because silently widening to root
    /// is the wrong way to be wrong about an authorization level.
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "root" => Some(Self::Root),
            "namespace" | "ns" => Some(Self::Namespace),
            "database" | "db" => Some(Self::Database),
            _ => None,
        }
    }

    /// The spelling this scope is configured by.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Namespace => "namespace",
            Self::Database => "database",
        }
    }
}

/// How a connection proves who it is.
///
/// Which variant applies is decided entirely by which variables are set,
/// so a deployment adopts whatever credential its engine actually issues
/// without a code change — the embedded test engine has none, the KIND
/// tier and Surreal Cloud both use a username and password today, and a
/// provider that issues only tokens is already accounted for.
///
/// `Debug` is hand-written so a password or token never reaches a log
/// line: the connection config is printed in CLI diagnostics and error
/// contexts, and a derived `Debug` would carry the secret into all of
/// them.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum SurrealAuth {
    /// No sign-in at all — the embedded `mem://` engine a test opens is
    /// its own address space and has no user to sign in as.
    #[default]
    Anonymous,
    /// A username and password at [`AuthScope`].
    Password {
        scope: AuthScope,
        username: String,
        password: String,
    },
    /// A pre-issued bearer token.
    Token(String),
}

impl std::fmt::Debug for SurrealAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anonymous => f.write_str("Anonymous"),
            Self::Password {
                scope, username, ..
            } => f
                .debug_struct("Password")
                .field("scope", scope)
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            Self::Token(_) => f.debug_tuple("Token").field(&"<redacted>").finish(),
        }
    }
}

/// Everything needed to reach one SurrealDB database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurrealConfig {
    pub endpoint: String,
    pub namespace: String,
    pub database: String,
    /// How the connection proves who it is. [`SurrealAuth::Anonymous`]
    /// for an engine that accepts unauthenticated access.
    pub auth: SurrealAuth,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SurrealConfigError {
    #[error(
        "{0} must be set: there is no implicit SurrealDB endpoint, namespace, or database. \
         Source the environment's `.devx/env`, or name the values explicitly."
    )]
    MissingEnv(&'static str),
    #[error(
        "{USER_ENV} and {PASSWORD_ENV} configure one login and must be set together; \
         {0} is missing"
    )]
    PartialCredentials(&'static str),
    #[error(
        "{TOKEN_ENV} and {USER_ENV}/{PASSWORD_ENV} are two different ways to authenticate \
         and only one may be set: a connection cannot sign in twice, and guessing which \
         the operator meant is how the wrong identity reaches the engine"
    )]
    AmbiguousAuth,
    #[error(
        "{AUTH_SCOPE_ENV} must be `root`, `namespace`, or `database`; got {0:?}. It is not \
         defaulted, because silently widening to root is the wrong way to be wrong about \
         an authorization level"
    )]
    UnknownAuthScope(String),
}

impl SurrealConfig {
    /// Resolve the connection from the process environment.
    pub fn from_env() -> Result<Self, SurrealConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Resolve the connection from any `key -> Option<value>` lookup —
    /// the testable seam [`from_env`](Self::from_env) wraps.
    ///
    /// Fails closed on an unnamed backend, for the reason
    /// `cloud::backend_from_lookup` fails closed on an unset
    /// `NAVIGATOR_STORAGE_BACKEND` (#618): a defaulted embedded engine
    /// would accept every write into the process's own memory and
    /// report success, turning one boot-time misconfiguration into a
    /// far-away "the data never arrived" symptom. A caller that wants
    /// the embedded engine names it.
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, SurrealConfigError> {
        let required = |key: &'static str| {
            get(key)
                .filter(|value| !value.trim().is_empty())
                .ok_or(SurrealConfigError::MissingEnv(key))
        };
        let optional = |key: &str| get(key).filter(|value| !value.trim().is_empty());

        let endpoint = required(ENDPOINT_ENV)?;
        let namespace = required(NAMESPACE_ENV)?;
        let database = required(DATABASE_ENV)?;
        let token = optional(TOKEN_ENV);
        let auth = match (optional(USER_ENV), optional(PASSWORD_ENV)) {
            (Some(_), _) | (_, Some(_)) if token.is_some() => {
                return Err(SurrealConfigError::AmbiguousAuth)
            }
            (Some(username), Some(password)) => {
                let scope = match optional(AUTH_SCOPE_ENV) {
                    None => AuthScope::default(),
                    Some(named) => AuthScope::parse(&named)
                        .ok_or(SurrealConfigError::UnknownAuthScope(named))?,
                };
                SurrealAuth::Password {
                    scope,
                    username,
                    password,
                }
            }
            (Some(_), None) => return Err(SurrealConfigError::PartialCredentials(PASSWORD_ENV)),
            (None, Some(_)) => return Err(SurrealConfigError::PartialCredentials(USER_ENV)),
            (None, None) => match token {
                Some(token) => SurrealAuth::Token(token),
                None => SurrealAuth::Anonymous,
            },
        };

        Ok(Self {
            endpoint,
            namespace,
            database,
            auth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuthScope, SurrealAuth, SurrealConfig, SurrealConfigError, AUTH_SCOPE_ENV, DATABASE_ENV,
        ENDPOINT_ENV, NAMESPACE_ENV, PASSWORD_ENV, TOKEN_ENV, USER_ENV,
    };
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    fn complete() -> Vec<(&'static str, &'static str)> {
        vec![
            (ENDPOINT_ENV, "ws://localhost:18000"),
            (NAMESPACE_ENV, "navigator"),
            (DATABASE_ENV, "navigator"),
        ]
    }

    #[test]
    fn a_complete_environment_resolves_without_credentials() {
        let cfg = SurrealConfig::from_lookup(lookup(&complete())).unwrap();
        assert_eq!(
            cfg,
            SurrealConfig {
                endpoint: "ws://localhost:18000".into(),
                namespace: "navigator".into(),
                database: "navigator".into(),
                auth: crate::surreal::SurrealAuth::Anonymous,
            }
        );
    }

    #[test]
    fn a_username_and_password_default_to_the_root_scope() {
        let mut env = complete();
        env.push((USER_ENV, "root"));
        env.push((PASSWORD_ENV, "root"));

        let cfg = SurrealConfig::from_lookup(lookup(&env)).unwrap();

        assert_eq!(
            cfg.auth,
            SurrealAuth::Password {
                scope: AuthScope::Root,
                username: "root".into(),
                password: "root".into(),
            }
        );
    }

    /// The point of the scope being configuration: a managed engine that
    /// issues only a namespace user is adopted without a code change.
    #[test]
    fn every_scope_spelling_resolves_including_its_abbreviation() {
        for (named, expected) in [
            ("root", AuthScope::Root),
            ("namespace", AuthScope::Namespace),
            ("ns", AuthScope::Namespace),
            ("database", AuthScope::Database),
            ("db", AuthScope::Database),
            ("  NameSpace  ", AuthScope::Namespace),
        ] {
            let mut env = complete();
            env.push((USER_ENV, "admin"));
            env.push((PASSWORD_ENV, "secret"));
            env.push((AUTH_SCOPE_ENV, named));

            let cfg = SurrealConfig::from_lookup(lookup(&env)).unwrap();
            assert!(
                matches!(cfg.auth, SurrealAuth::Password { scope, .. } if scope == expected),
                "{named} must resolve to {expected:?}, got {:?}",
                cfg.auth
            );
        }
    }

    /// Not defaulted: silently widening an unknown spelling to root is
    /// the wrong way to be wrong about an authorization level.
    #[test]
    fn an_unknown_scope_is_rejected_rather_than_defaulted() {
        let mut env = complete();
        env.push((USER_ENV, "admin"));
        env.push((PASSWORD_ENV, "secret"));
        env.push((AUTH_SCOPE_ENV, "superuser"));

        assert_eq!(
            SurrealConfig::from_lookup(lookup(&env)).unwrap_err(),
            SurrealConfigError::UnknownAuthScope("superuser".into())
        );
    }

    #[test]
    fn a_token_alone_resolves_to_token_auth() {
        let mut env = complete();
        env.push((TOKEN_ENV, "a.b.c"));

        assert_eq!(
            SurrealConfig::from_lookup(lookup(&env)).unwrap().auth,
            SurrealAuth::Token("a.b.c".into())
        );
    }

    /// A connection cannot sign in twice, and guessing which the operator
    /// meant is how the wrong identity reaches the engine.
    #[test]
    fn a_token_beside_a_password_is_ambiguous_rather_than_ranked() {
        for extra in [
            vec![(USER_ENV, "admin"), (PASSWORD_ENV, "s")],
            vec![(USER_ENV, "admin")],
        ] {
            let mut env = complete();
            env.push((TOKEN_ENV, "a.b.c"));
            env.extend(extra);

            assert_eq!(
                SurrealConfig::from_lookup(lookup(&env)).unwrap_err(),
                SurrealConfigError::AmbiguousAuth
            );
        }
    }

    /// The fail-closed rule, one case per required variable: an unset or
    /// blank value is an error, never a default endpoint or an implicit
    /// `mem://`.
    #[test]
    fn every_required_variable_fails_closed_when_unset_or_blank() {
        for missing in [ENDPOINT_ENV, NAMESPACE_ENV, DATABASE_ENV] {
            let without: Vec<_> = complete()
                .into_iter()
                .filter(|(key, _)| *key != missing)
                .collect();
            assert_eq!(
                SurrealConfig::from_lookup(lookup(&without)).unwrap_err(),
                SurrealConfigError::MissingEnv(missing),
                "unset {missing}"
            );

            let blank: Vec<_> = complete()
                .into_iter()
                .map(|(key, value)| {
                    if key == missing {
                        (key, "   ")
                    } else {
                        (key, value)
                    }
                })
                .collect();
            assert_eq!(
                SurrealConfig::from_lookup(lookup(&blank)).unwrap_err(),
                SurrealConfigError::MissingEnv(missing),
                "blank {missing}"
            );
        }
    }

    #[test]
    fn half_a_credential_is_rejected_rather_than_silently_dropped() {
        let mut user_only = complete();
        user_only.push((USER_ENV, "root"));
        assert_eq!(
            SurrealConfig::from_lookup(lookup(&user_only)).unwrap_err(),
            SurrealConfigError::PartialCredentials(PASSWORD_ENV)
        );

        let mut password_only = complete();
        password_only.push((PASSWORD_ENV, "root"));
        assert_eq!(
            SurrealConfig::from_lookup(lookup(&password_only)).unwrap_err(),
            SurrealConfigError::PartialCredentials(USER_ENV)
        );
    }

    #[test]
    fn a_stray_database_url_is_not_a_surreal_endpoint() {
        // `DATABASE_URL` configures no connection, and a developer machine
        // can still be exporting one. A process carrying only it must fail
        // on the missing Surreal endpoint rather than connect somewhere
        // surprising.
        let err = SurrealConfig::from_lookup(lookup(&[(
            "DATABASE_URL",
            "postgres://navigator:navigator@localhost:15432/navigator",
        )]))
        .unwrap_err();
        assert_eq!(err, SurrealConfigError::MissingEnv(ENDPOINT_ENV));
    }

    #[test]
    fn debug_output_never_carries_the_password() {
        let cfg = SurrealConfig {
            endpoint: "ws://localhost:18000".into(),
            namespace: "navigator".into(),
            database: "navigator".into(),
            auth: SurrealAuth::Password {
                scope: AuthScope::Root,
                username: "root".into(),
                password: "hunter2".into(),
            },
        };

        let rendered = format!("{cfg:?}");

        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }

    /// The token is key material too, and reaches the same diagnostics.
    #[test]
    fn debug_output_never_carries_the_token() {
        let cfg = SurrealConfig {
            endpoint: "ws://localhost:18000".into(),
            namespace: "navigator".into(),
            database: "navigator".into(),
            auth: SurrealAuth::Token("header.payload.signature".into()),
        };

        let rendered = format!("{cfg:?}");

        assert!(!rendered.contains("payload"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }
}
