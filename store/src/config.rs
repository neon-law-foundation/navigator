//! Database connection configuration.
//!
//! The `store` crate owns this so non-`web` consumers (`cli`, `mcp`)
//! can build a `DbConfig` without pulling in the rest of the HTTP
//! server. `AppConfig` (which embeds a `DbConfig`) lives in `web`.
//!
//! | Variable             | Default              | Purpose                                       |
//! |----------------------|----------------------|-----------------------------------------------|
//! | `DATABASE_URL`       | _(unset, required)_  | Postgres URL. Required — there is no fallback. |
//!
//! Postgres is the only supported backend (Cloud SQL in prod,
//! in-cluster Postgres in KIND). The previous SQLite variant +
//! `APP_ENV` selector are gone.

use thiserror::Error;

/// Database backend configuration. A thin wrapper around the Postgres
/// connection URL — kept as a struct (rather than a bare `String`)
/// so callers continue to pattern-match `Result<DbConfig, _>` and so
/// we have a single place to grow new fields (pool sizing, sslmode
/// override) without touching call sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConfig {
    pub url: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DbConfigError {
    #[error("DATABASE_URL must be set (Postgres is the only supported backend)")]
    MissingDatabaseUrl,
}

impl DbConfig {
    /// Render the backend as a database URL acceptable to SeaORM /
    /// sqlx. A pass-through today; the indirection lives so future
    /// connection-string assembly (sslmode, pool params) has one
    /// natural seam.
    #[must_use]
    pub fn to_url(&self) -> String {
        self.url.clone()
    }

    /// Build a `DbConfig` from the process environment. Requires
    /// `DATABASE_URL`; returns [`DbConfigError::MissingDatabaseUrl`]
    /// if it is unset or empty.
    pub fn from_env() -> Result<Self, DbConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Build a `DbConfig` from any `key -> Option<value>` lookup.
    /// The testable seam — `from_env` is a thin wrapper.
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, DbConfigError> {
        let url = get("DATABASE_URL")
            .filter(|s| !s.is_empty())
            .ok_or(DbConfigError::MissingDatabaseUrl)?;
        Ok(DbConfig { url })
    }
}

#[cfg(test)]
mod tests {
    use super::{DbConfig, DbConfigError};
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn missing_database_url_is_an_error() {
        let err = DbConfig::from_lookup(|_| None).unwrap_err();
        assert_eq!(err, DbConfigError::MissingDatabaseUrl);
    }

    #[test]
    fn empty_database_url_is_an_error() {
        let err = DbConfig::from_lookup(lookup(&[("DATABASE_URL", "")])).unwrap_err();
        assert_eq!(err, DbConfigError::MissingDatabaseUrl);
    }

    #[test]
    fn database_url_set_picks_postgres() {
        let cfg = DbConfig::from_lookup(lookup(&[("DATABASE_URL", "postgres://u:p@host:5432/db")]))
            .unwrap();
        assert_eq!(
            cfg,
            DbConfig {
                url: "postgres://u:p@host:5432/db".into()
            }
        );
    }

    #[test]
    fn db_config_passes_through_postgres_url() {
        let url = DbConfig {
            url: "postgres://u:p@host:5432/db".into(),
        }
        .to_url();
        assert_eq!(url, "postgres://u:p@host:5432/db");
    }
}
