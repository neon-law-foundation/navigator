//! Application Default Credentials (ADC) bridge for [`super::client::GcpClient`].
//!
//! Real `devx gcp setup` runs authenticate via `google-cloud-auth`'s
//! `DefaultTokenSourceProvider`, which handles user-creds /
//! service-account / metadata-server discovery internally. Tests
//! and dry-runs use [`super::client::StaticToken`] instead — see
//! [`super::client`] for the trait.
//!
//! Set `DEVX_GCP_FAKE_TOKEN=1` to skip ADC and use a placeholder
//! bearer token — useful for exercising the binary against a
//! `wiremock` server without real GCP credentials.

use std::sync::Arc;

use anyhow::{Context, Result};

use super::client::{ClientError, StaticToken, TokenProvider};

/// Build a token provider backed by Application Default Credentials.
pub async fn adc_token_provider() -> Result<Arc<dyn TokenProvider>> {
    if std::env::var_os("DEVX_GCP_FAKE_TOKEN").is_some() {
        return Ok(Arc::new(StaticToken("unused".into())));
    }
    Ok(Arc::new(AdcToken::new().await?))
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
