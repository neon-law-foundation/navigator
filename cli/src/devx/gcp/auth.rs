//! Application Default Credentials (ADC) bridge for [`super::client::GcpClient`].
//!
//! Real `devx gcp setup` runs authenticate via `google-cloud-auth`'s
//! `DefaultTokenSourceProvider`, which handles user-creds /
//! service-account / metadata-server discovery internally, with the
//! `gcloud` CLI as a documented fallback (see [`cloud::gcloud`]).
//! Tests and dry-runs use [`super::client::StaticToken`] instead — see
//! [`super::client`] for the trait.
//!
//! Set `DEVX_GCP_FAKE_TOKEN=1` to skip both and use a placeholder
//! bearer token — useful for exercising the binary against a
//! `wiremock` server without real GCP credentials.

use std::sync::Arc;

use anyhow::{Context, Result};

use super::client::{ClientError, StaticToken, TokenProvider};

/// Build a token provider: ADC first, then the `gcloud` CLI.
///
/// The fallback itself lives in [`cloud::gcloud`], which documents why it
/// is load-bearing and which shares it with the GCS storage client — the
/// same `google-cloud-auth` gap sinks both.
pub async fn adc_token_provider() -> Result<Arc<dyn TokenProvider>> {
    if std::env::var_os("DEVX_GCP_FAKE_TOKEN").is_some() {
        return Ok(Arc::new(StaticToken("unused".into())));
    }
    match AdcToken::new().await {
        Ok(token) => Ok(Arc::new(token)),
        Err(adc_error) => match cloud::gcloud::probe() {
            Ok(()) => {
                eprintln!(
                    "==> Application Default Credentials unavailable ({adc_error:#}); \
                     using `gcloud auth print-access-token` instead"
                );
                Ok(Arc::new(GcloudToken))
            }
            Err(gcloud_error) => Err(adc_error.context(format!(
                "`gcloud auth print-access-token` is not usable either: {gcloud_error}"
            ))),
        },
    }
}

/// ADC-backed [`TokenProvider`]. Wraps `google-cloud-auth`'s
/// `DefaultTokenSourceProvider`, which handles user-creds /
/// service-account / metadata-server discovery internally.
struct AdcToken {
    source: Arc<dyn google_cloud_token::TokenSource>,
}

impl AdcToken {
    async fn new() -> Result<Self> {
        let scopes: [&str; 1] = ["https://www.googleapis.com/auth/cloud-platform"];
        let config = google_cloud_auth::project::Config::default().with_scopes(&scopes);
        let provider = google_cloud_auth::token::DefaultTokenSourceProvider::new(config)
            .await
            .context("acquire Application Default Credentials")?;
        Ok(Self {
            source: google_cloud_token::TokenSourceProvider::token_source(&provider),
        })
    }
}

#[async_trait::async_trait]
impl TokenProvider for AdcToken {
    async fn token(&self) -> std::result::Result<String, ClientError> {
        // `TokenSource::token()` returns `"Bearer <token>"`; strip
        // the prefix so callers can use `reqwest::bearer_auth`.
        let raw = self
            .source
            .token()
            .await
            .map_err(|e| ClientError::Auth(e.to_string()))?;
        Ok(raw.strip_prefix("Bearer ").unwrap_or(&raw).to_string())
    }
}

/// [`TokenProvider`] backed by [`cloud::gcloud::access_token`].
struct GcloudToken;

#[async_trait::async_trait]
impl TokenProvider for GcloudToken {
    async fn token(&self) -> std::result::Result<String, ClientError> {
        // `spawn_blocking` because this runs inside the async client's
        // request path; a subprocess on the reactor thread would stall
        // every other in-flight call.
        tokio::task::spawn_blocking(cloud::gcloud::access_token)
            .await
            .map_err(|e| ClientError::Auth(format!("join gcloud token task: {e}")))?
            .map_err(|e| ClientError::Auth(format!("{e:#}")))
    }
}
