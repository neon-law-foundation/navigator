//! Database connection configuration.
//!
//! The `store` crate owns this so non-`web` consumers (`cli`, `mcp`)
//! can read the deployment profile without pulling in the rest of the
//! HTTP server. `AppConfig` lives in `web`.
//!
//! The store's own connection contract is `NAVIGATOR_SURREAL_ENDPOINT` /
//! `_NAMESPACE` / `_DATABASE`, and it lives in [`crate::surreal`].

use thiserror::Error;

/// The single deployment-profile selector understood by Navigator.
pub const NAVIGATOR_ENVIRONMENT: &str = "NAVIGATOR_ENVIRONMENT";

/// Infrastructure profile for a running Navigator binary.
///
/// This is deliberately narrower than an application runtime mode: it
/// selects only development versus production deployment wiring. The
/// one `Dev` profile backs both local KIND and the cloud staging lane;
/// "staging" survives only as a Kubernetes deployment-lane name, never
/// as an application environment value. `test` is not a variant — it is
/// the `Dev` profile with `NAVIGATOR_CI_HARNESS=1` layered on top.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DeploymentEnvironment {
    Dev,
    #[default]
    Production,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeploymentEnvironmentError {
    #[error(
        "NAVIGATOR_ENVIRONMENT must be unset, empty, or exactly `dev` or `production`; got `{0}`"
    )]
    Invalid(String),
}

impl DeploymentEnvironment {
    pub fn from_env() -> Result<Self, DeploymentEnvironmentError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup<F: Fn(&str) -> Option<String>>(
        get: F,
    ) -> Result<Self, DeploymentEnvironmentError> {
        match get(NAVIGATOR_ENVIRONMENT).as_deref() {
            None | Some("" | "production") => Ok(Self::Production),
            Some("dev") => Ok(Self::Dev),
            Some(other) => Err(DeploymentEnvironmentError::Invalid(other.to_owned())),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Production => "production",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeploymentEnvironment, DeploymentEnvironmentError, NAVIGATOR_ENVIRONMENT};

    #[test]
    fn deployment_environment_parser_is_exact_and_production_safe() {
        let invalid = |value: &str| Err(DeploymentEnvironmentError::Invalid(value.to_owned()));
        let cases = [
            // Unset, empty, and exact `production` all select the
            // production-safe default.
            (None, Ok(DeploymentEnvironment::Production)),
            (Some(""), Ok(DeploymentEnvironment::Production)),
            (Some("production"), Ok(DeploymentEnvironment::Production)),
            // Exact `dev` is the single development profile shared by
            // local KIND and the cloud staging lane.
            (Some("dev"), Ok(DeploymentEnvironment::Dev)),
            // `staging` is a deployment-lane name, never an application
            // environment value, so it is now rejected.
            (Some("staging"), invalid("staging")),
            // `test` is the `dev` profile plus the CI harness, not a
            // runtime deployment value.
            (Some("test"), invalid("test")),
            // Case and whitespace variants of every accepted value are
            // rejected — the parser is exact.
            (Some("Dev"), invalid("Dev")),
            (Some("DEV"), invalid("DEV")),
            (Some(" dev"), invalid(" dev")),
            (Some("dev "), invalid("dev ")),
            (Some("Production"), invalid("Production")),
            (Some(" production"), invalid(" production")),
            (Some("development"), invalid("development")),
        ];

        for (raw, expected) in cases {
            let actual = DeploymentEnvironment::from_lookup(|key| {
                assert_eq!(key, NAVIGATOR_ENVIRONMENT);
                raw.map(str::to_owned)
            });
            assert_eq!(actual, expected, "raw value: {raw:?}");
        }
    }

    #[test]
    fn deployment_environment_as_str_and_default() {
        assert_eq!(DeploymentEnvironment::Dev.as_str(), "dev");
        assert_eq!(DeploymentEnvironment::Production.as_str(), "production");
        assert_eq!(
            DeploymentEnvironment::default(),
            DeploymentEnvironment::Production
        );
    }
}
