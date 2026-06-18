//! Typed error returned by every step of the `devx gcp setup`
//! pipeline. Replaces the bare `anyhow::Error` that the gcp/* modules
//! used to surface; the binary's `main.rs` still uses `anyhow` at the
//! outermost layer and `?` converts `SetupError` into `anyhow::Error`
//! via the blanket `From<E: std::error::Error>` impl.

use std::time::Duration;

use thiserror::Error;

use super::client::ClientError;

/// Anything the setup pipeline can fail with.
#[derive(Debug, Error)]
pub enum SetupError {
    /// The HTTP/auth layer in [`super::client::GcpClient`] surfaced
    /// an error (network, non-2xx the client itself wanted to flag,
    /// auth token acquisition).
    #[error(transparent)]
    Client(#[from] ClientError),

    /// A GCP REST call returned a non-2xx status. `operation` is a
    /// human-readable summary (`"create bucket navigator-prod-assets"`,
    /// `"batchEnable"`, `"create SQL instance"`) and `body` carries
    /// whatever GCP wrote into the response. The numeric status code
    /// stays in the message verbatim so existing tests that grep
    /// `format!("{err}")` for `"403"` / `"409"` keep matching.
    #[error("{operation} failed with status {status}: {body}")]
    BadStatus {
        operation: String,
        status: u16,
        body: String,
    },

    /// JSON parsing failed — usually a response body that we expect
    /// to be a well-formed `Operation` resource.
    #[error("parse {what}: {source}")]
    Json {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// A long-running operation reported its own error. Carries the
    /// raw JSON `error` object as a string so log scrapers and the
    /// `permission denied` test assertion can still match.
    #[error("operation failed: {0}")]
    OperationFailed(String),

    /// Polling for a long-running operation exceeded its budget.
    #[error("operation {name} did not complete within {timeout:?}")]
    OperationTimeout { name: String, timeout: Duration },

    /// A GCP response was structurally invalid — e.g. an `Operation`
    /// resource missing its `name` field.
    #[error("malformed GCP response: {0}")]
    Malformed(&'static str),

    /// A shell-out (gcloud, kubectl) returned non-zero AND the stderr
    /// did not match the "already exists" idempotency pattern. The
    /// numeric exit code stays in the message verbatim so log
    /// scrapers can grep for it.
    #[error("{operation} failed: `{command}` exited {exit}: {stderr}")]
    ShellFailed {
        operation: &'static str,
        command: String,
        exit: i32,
        stderr: String,
    },
}

/// Convenience alias used throughout `devx::gcp`.
pub type SetupResult<T> = Result<T, SetupError>;
