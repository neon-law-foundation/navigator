//! Drive authentication backends.
//!
//! See the module docs in [`super`] for the three-doors story. This
//! file ships the two server-side / CLI doors behind a single trait
//! so the higher-level `DriveClient` (commit 2) doesn't care which
//! it was handed.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::{DriveError, DRIVE_READONLY_SCOPE, GOOGLE_TOKEN_URI};

/// Common interface for "give me a Drive bearer token to put in an
/// `Authorization` header." Concrete implementations:
///
/// - [`CliRefreshTokenAuth`] — installed-app refresh token flow, used
///   from the CLI and tests.
/// - [`WorkloadIdentitySaAuth`] — server-side, on GKE under a
///   service account.
#[async_trait]
pub trait DriveAuth: Send + Sync {
    /// Return a Drive access token (no `Bearer ` prefix). Concrete
    /// implementations are expected to cache + refresh transparently
    /// so callers can invoke this on every request without thinking
    /// about expiry.
    async fn access_token(&self) -> Result<String, DriveError>;
}

/// In-memory cache of an access token + its expiry. Held behind a
/// `tokio::Mutex` so concurrent callers serialize through a single
/// refresh.
#[derive(Debug, Default, Clone)]
struct CachedToken {
    token: Option<String>,
    /// Wall-clock time at which `token` becomes unsafe to use. We
    /// subtract a small safety margin from Google's `expires_in`
    /// (see `SKEW`) so a token expiring "right now" never makes it
    /// onto the wire.
    expires_at: Option<DateTime<Utc>>,
}

/// Treat a token as expired this many seconds before its real
/// `expires_at`. Guards against clock skew between us and Google,
/// and against the case where the token is fetched, then the call
/// it was minted for sits in a queue for a few seconds.
const SKEW: i64 = 60;

/// Refresh-token-backed [`DriveAuth`]. Builds a Drive access token
/// by trading the persisted `refresh_token` at Google's token
/// endpoint. Caches the resulting access token in memory; a new
/// process re-mints on first use.
///
/// The persisted refresh token itself is **not** held by this struct
/// — it's passed in at construction. Callers that read from disk
/// should go through [`super::token_store::load_drive_token`] which
/// enforces `0o600` permissions.
pub struct CliRefreshTokenAuth {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    token_uri: String,
    http: reqwest::Client,
    cache: Arc<Mutex<CachedToken>>,
}

impl std::fmt::Debug for CliRefreshTokenAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never leak the client_secret or refresh_token through Debug.
        f.debug_struct("CliRefreshTokenAuth")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("token_uri", &self.token_uri)
            .finish_non_exhaustive()
    }
}

impl CliRefreshTokenAuth {
    /// Construct with the standard Google token endpoint
    /// (`https://oauth2.googleapis.com/token`).
    #[must_use]
    pub fn new(client_id: String, client_secret: String, refresh_token: String) -> Self {
        Self::with_token_uri(
            client_id,
            client_secret,
            refresh_token,
            GOOGLE_TOKEN_URI.to_string(),
        )
    }

    /// Construct with a custom token endpoint — used by tests
    /// pointing at a `wiremock` server.
    #[must_use]
    pub fn with_token_uri(
        client_id: String,
        client_secret: String,
        refresh_token: String,
        token_uri: String,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            refresh_token,
            token_uri,
            http: reqwest::Client::new(),
            cache: Arc::new(Mutex::new(CachedToken::default())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    /// Lifetime in seconds. Google's docs say this is always set on
    /// a refresh-token grant; we still tolerate it being absent by
    /// falling back to a one-hour assumption.
    expires_in: Option<i64>,
}

#[async_trait]
impl DriveAuth for CliRefreshTokenAuth {
    async fn access_token(&self) -> Result<String, DriveError> {
        let mut cache = self.cache.lock().await;
        if let (Some(tok), Some(exp)) = (&cache.token, cache.expires_at) {
            if Utc::now() + Duration::seconds(SKEW) < exp {
                return Ok(tok.clone());
            }
        }

        let resp = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("refresh_token", self.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(DriveError::OAuth {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: GoogleTokenResponse = serde_json::from_str(&body)?;
        let lifetime = parsed.expires_in.unwrap_or(3600);
        cache.token = Some(parsed.access_token.clone());
        cache.expires_at = Some(Utc::now() + Duration::seconds(lifetime));
        Ok(parsed.access_token)
    }
}

/// Workload-Identity-backed [`DriveAuth`]. On GKE pods bound to the
/// `navigator-drive-sync@…` service account, this acquires a token
/// from the metadata server via `google-cloud-auth`'s
/// `DefaultTokenSourceProvider`. No key file on disk.
///
/// Constructing this off-GKE (without ADC configured) will fail at
/// [`WorkloadIdentitySaAuth::new`]. Use [`CliRefreshTokenAuth`] for
/// dev / tests.
pub struct WorkloadIdentitySaAuth {
    source: Arc<dyn google_cloud_token::TokenSource>,
}

impl std::fmt::Debug for WorkloadIdentitySaAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkloadIdentitySaAuth")
            .field("source", &"<token-source>")
            .finish_non_exhaustive()
    }
}

impl WorkloadIdentitySaAuth {
    /// Acquire an ADC-backed token source bound to the Drive
    /// read-only scope. Fails if no Application Default Credentials
    /// can be resolved (no metadata server, no `GOOGLE_APPLICATION_CREDENTIALS`,
    /// no `gcloud auth application-default login`).
    pub async fn new() -> Result<Self, DriveError> {
        let scopes: [&str; 1] = [DRIVE_READONLY_SCOPE];
        let config = google_cloud_auth::project::Config::default().with_scopes(&scopes);
        let provider = google_cloud_auth::token::DefaultTokenSourceProvider::new(config)
            .await
            .map_err(|e| DriveError::WorkloadIdentity(e.to_string()))?;
        Ok(Self {
            source: google_cloud_token::TokenSourceProvider::token_source(&provider),
        })
    }
}

#[async_trait]
impl DriveAuth for WorkloadIdentitySaAuth {
    async fn access_token(&self) -> Result<String, DriveError> {
        // `TokenSource::token()` returns `"Bearer <token>"`; strip the
        // prefix so callers don't double-prefix when building an
        // `Authorization` header.
        let raw = self
            .source
            .token()
            .await
            .map_err(|e| DriveError::WorkloadIdentity(e.to_string()))?;
        Ok(raw.strip_prefix("Bearer ").unwrap_or(&raw).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn refreshes_when_cache_empty() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=rt-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "ya29.first",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let auth = CliRefreshTokenAuth::with_token_uri(
            "cid".into(),
            "csec".into(),
            "rt-abc".into(),
            format!("{}/token", server.uri()),
        );
        let token = auth.access_token().await.unwrap();
        assert_eq!(token, "ya29.first");
    }

    #[tokio::test]
    async fn caches_until_expiry() {
        let server = MockServer::start().await;
        // expect(1) — second call must hit the cache, not the server.
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "ya29.cached",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;

        let auth = CliRefreshTokenAuth::with_token_uri(
            "cid".into(),
            "csec".into(),
            "rt-abc".into(),
            format!("{}/token", server.uri()),
        );
        let a = auth.access_token().await.unwrap();
        let b = auth.access_token().await.unwrap();
        assert_eq!(a, "ya29.cached");
        assert_eq!(b, "ya29.cached");
    }

    #[tokio::test]
    async fn surfaces_oauth_error_with_status_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"error":"invalid_grant","error_description":"Token expired or revoked."}"#,
            ))
            .mount(&server)
            .await;

        let auth = CliRefreshTokenAuth::with_token_uri(
            "cid".into(),
            "csec".into(),
            "rt-dead".into(),
            format!("{}/token", server.uri()),
        );
        match auth.access_token().await {
            Err(DriveError::OAuth { status, body }) => {
                assert_eq!(status, 400);
                assert!(body.contains("invalid_grant"), "body was: {body}");
            }
            other => panic!("expected OAuth error, got {other:?}"),
        }
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        let auth = CliRefreshTokenAuth::with_token_uri(
            "client-id-visible".into(),
            "super-secret-should-not-appear".into(),
            "refresh-token-should-not-appear".into(),
            "https://example/token".into(),
        );
        let s = format!("{auth:?}");
        assert!(s.contains("client-id-visible"));
        assert!(!s.contains("super-secret-should-not-appear"));
        assert!(!s.contains("refresh-token-should-not-appear"));
        assert!(s.contains("<redacted>"));
    }
}
