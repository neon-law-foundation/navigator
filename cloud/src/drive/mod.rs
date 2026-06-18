//! Google Drive integration for the Navigator workspace.
//!
//! Drive is **one inbound channel** for matter materials — peer to
//! email, fax, scan, and the web upload form. The canonical store
//! for bytes is `documents` + `blobs` (GCS-backed via
//! [`crate::StorageService`]); this module never holds bytes longer
//! than it takes to push them through `ingest_bytes`. Drive
//! permissions matter for *reading*; once a byte lands in our
//! object storage, Navigator's own DB-role authz governs access.
//!
//! ## Scope
//!
//! Every door defined here requests **read-only** Drive scope
//! (`https://www.googleapis.com/auth/drive.readonly`). Write-back to
//! Drive (signed retainers, generated invoices) is a separate
//! scope-bump decision and would land alongside the specific
//! workflow that needs it — not in this module.
//!
//! ## Three doors, one trait
//!
//! The [`DriveAuth`] trait abstracts how an HTTP call to
//! `https://www.googleapis.com/drive/v3/...` gets its bearer token.
//! Two implementations ship today, matching the auth doors in
//! `CLAUDE.md`:
//!
//! - [`CliRefreshTokenAuth`] — the `cli` binary (and any user-facing
//!   surface that wants to act as the lawyer) reads the installed-app
//!   client config from `~/.config/navigator/oauth_client.json` and
//!   the refresh token from `~/.config/navigator/drive_token.json`
//!   (file mode `0o600`). Trades the refresh token for a short-lived
//!   access token on demand, caches it in memory until expiry.
//! - [`WorkloadIdentitySaAuth`] — the server-side ingestion workflow,
//!   running on GKE under the `navigator-drive-sync@…` service
//!   account via Workload Identity. No key file on disk. Wraps
//!   `google-cloud-auth`'s `DefaultTokenSourceProvider`.
//!
//! The third door named in `CLAUDE.md` — browser-as-lawyer — does
//! not flow through this module; that path uses the lawyer's own
//! Google session cookies in their own browser, never through
//! Navigator's server.

use thiserror::Error;

pub mod auth;
pub mod client;
pub mod oauth_client;
pub mod sync;
pub mod token_store;

pub use auth::{CliRefreshTokenAuth, DriveAuth, WorkloadIdentitySaAuth};
pub use client::{
    export_mime_for, is_google_native, DownloadedBytes, DriveClient, DriveFile, DriveSummary,
    FOLDER_MIME, GOOGLE_DRIVE_BASE_URL,
};
pub use oauth_client::{default_oauth_client_path, load_oauth_client, OauthClientConfig};
pub use token_store::{default_drive_token_path, load_drive_token, save_drive_token, DriveToken};

/// The Drive scope every backend in this module requests. Read-only
/// — sync is one-way pull. Writing back is a separate decision that
/// would happen alongside a specific workflow needing it.
pub const DRIVE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/drive.readonly";

/// Default OAuth token endpoint. Tests override this with a
/// `wiremock` URL.
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Errors surfaced by the Drive module.
#[derive(Debug, Error)]
pub enum DriveError {
    /// Filesystem I/O failed while reading or writing a config /
    /// token file.
    #[error("drive io error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP transport failed (DNS, TLS, broken pipe, timeout).
    #[error("drive http error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parse failed on a config file or an API response body.
    #[error("drive json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A required config file is missing — typically
    /// `oauth_client.json` or `drive_token.json` when the user
    /// hasn't run `cli drive login`.
    #[error("drive config missing: {0}")]
    MissingConfig(String),

    /// A config file was found but its contents are not well-formed
    /// for our use (e.g., missing the `installed` block, no
    /// `refresh_token`).
    #[error("drive config invalid: {0}")]
    InvalidConfig(String),

    /// The OAuth token endpoint returned a non-2xx response. Holds
    /// the HTTP status + raw body so callers can route on Google's
    /// `error` / `error_description` fields.
    #[error("oauth token endpoint returned {status}: {body}")]
    OAuth { status: u16, body: String },

    /// The Drive REST API returned a non-2xx response.
    #[error("drive api returned {status}: {body}")]
    Api { status: u16, body: String },

    /// Drive rate-limited the request and the retry budget was
    /// exhausted (HTTP 429 / `userRateLimitExceeded`).
    #[error("drive rate limited; retries exhausted")]
    RateLimited,

    /// The Workload Identity / ADC path failed to acquire a token.
    /// Distinct from [`DriveError::OAuth`] so callers can tell apart
    /// "your refresh token is bad" from "this pod can't reach the
    /// metadata server".
    #[error("workload identity error: {0}")]
    WorkloadIdentity(String),
}
