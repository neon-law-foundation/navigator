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

/// Whether this deployment announces that its matters are simulated.
///
/// This selector renders a site-wide banner telling every visitor that nothing
/// they are looking at is a real client's file. **That is all it does.** It
/// seeds no row and writes no object; the seed keys its own fixture layer on
/// [`DeploymentEnvironment`] alone.
///
/// The two are separate because they answer different questions. Whether to
/// *write* fixture data is a question about authority, and the answer is that a
/// production-profile deployment is never written to. Whether to *say* the
/// matters are simulated is a question about disclosure, and a deployment can
/// need the disclosure while holding a portfolio it was given once — which is
/// exactly the persistent staging deployment, running the production runtime
/// profile over data no boot re-asserts.
pub const NAVIGATOR_SIMULATED_MATTERS: &str = "NAVIGATOR_SIMULATED_MATTERS";

/// Why a `NAVIGATOR_SIMULATED_MATTERS` value could not be read.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SampleMattersError {
    #[error(
        "NAVIGATOR_SIMULATED_MATTERS must be unset, empty, or exactly `true` or `false`; got `{0}`"
    )]
    Invalid(String),
}

/// Whether this deployment carries sample matters, from the environment.
///
/// # Errors
///
/// Returns [`SampleMattersError::Invalid`] for any value that is not
/// exactly `true` or `false`.
pub fn sample_matters(environment: DeploymentEnvironment) -> Result<bool, SampleMattersError> {
    sample_matters_from(environment, |key| std::env::var(key).ok())
}

/// [`sample_matters`] with the environment read through `get`, so the
/// decision is testable without mutating process state.
///
/// Unset or empty follows the deployment profile: a `dev` boot announces
/// simulated matters because that is the only thing a `dev` boot has, and a
/// `production` boot does not because production is where the real files are.
///
/// An explicit value overrides that in **both** directions, and the direction
/// that matters is `true` under a `production` profile. That combination is not
/// a mistake to guard against here — it is exactly what the persistent staging
/// deployment is. Staging runs the production runtime profile deliberately, so
/// that the application it proves is the application production runs; its data
/// plane is the only thing about it that is synthetic. Nothing in this process
/// can tell that apart from a real production deployment, so the value is
/// trusted and the guard is the deployment's own `config.toml`.
///
/// The parser is exact for the same reason [`DeploymentEnvironment::from_lookup`]
/// is: a typo that silently resolved to `false` would drop the disclosure from a
/// deployment whose matters are invented, and a reader would have nothing
/// telling them so.
pub fn sample_matters_from<F: Fn(&str) -> Option<String>>(
    environment: DeploymentEnvironment,
    get: F,
) -> Result<bool, SampleMattersError> {
    match get(NAVIGATOR_SIMULATED_MATTERS).as_deref() {
        None | Some("") => Ok(environment != DeploymentEnvironment::Production),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(other) => Err(SampleMattersError::Invalid(other.to_owned())),
    }
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
    use super::{
        sample_matters_from, DeploymentEnvironment, DeploymentEnvironmentError, SampleMattersError,
        NAVIGATOR_ENVIRONMENT, NAVIGATOR_SIMULATED_MATTERS,
    };

    /// Unset follows the profile, an explicit value overrides it in both
    /// directions, and anything else is refused.
    ///
    /// The two rows that carry the design are `(Production, None) -> false`
    /// and `(Production, "true") -> true`: the first is why an unconfigured
    /// production deployment cannot grow invented clients, and the second is
    /// the persistent staging deployment, which runs the production profile
    /// over a synthetic data plane and has to say so.
    #[test]
    fn sample_matters_defaults_to_the_profile_and_is_overridable() {
        use DeploymentEnvironment::{Dev, Production};

        let cases = [
            // Unconfigured: a dev boot has nothing but fixtures; production
            // has nothing but real files.
            ((Dev, None), Ok(true)),
            ((Dev, Some("")), Ok(true)),
            ((Production, None), Ok(false)),
            ((Production, Some("")), Ok(false)),
            // Explicit, both directions. `(Production, "true")` is staging.
            ((Production, Some("true")), Ok(true)),
            ((Dev, Some("false")), Ok(false)),
            ((Dev, Some("true")), Ok(true)),
            ((Production, Some("false")), Ok(false)),
        ];
        for ((environment, value), expected) in cases {
            assert_eq!(
                sample_matters_from(environment, |key| {
                    assert_eq!(key, NAVIGATOR_SIMULATED_MATTERS);
                    value.map(str::to_owned)
                }),
                expected,
                "{environment:?} with {value:?}"
            );
        }
    }

    /// Every near-miss is refused rather than resolved to the permissive
    /// answer. A `TRUE` that quietly read as "no sample matters" would be
    /// a staging deployment serving an empty portfolio with no banner; a
    /// `yes` that quietly read as "sample" would be production seeding
    /// invented clients beside real ones.
    #[test]
    fn sample_matters_parser_is_exact() {
        for value in ["True", "TRUE", "1", "yes", "on", " true", "true ", "False"] {
            assert_eq!(
                sample_matters_from(DeploymentEnvironment::Production, |_| Some(
                    value.to_owned()
                )),
                Err(SampleMattersError::Invalid(value.to_owned())),
                "`{value}` must not parse"
            );
        }
    }

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
