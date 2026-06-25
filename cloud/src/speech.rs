//! Google Speech-to-Text provider for the Live Inquiry demo.
//!
//! The shared `live-inquiry` crate owns transcript and coverage types;
//! this module only knows how to turn audio bytes into transcript
//! segments through Google Cloud Speech-to-Text v2.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use live_inquiry::{TranscriptProvider, TranscriptSegment};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const SPEECH_BASE_URL: &str = "https://speech.googleapis.com/v2";

#[derive(Debug, Clone)]
pub struct GoogleSpeechConfig {
    pub project_id: String,
    pub location: String,
    pub language_code: String,
    pub model: String,
}

impl GoogleSpeechConfig {
    #[must_use]
    pub fn new(project_id: String) -> Self {
        Self {
            project_id,
            location: "global".to_string(),
            language_code: "en-US".to_string(),
            model: "latest_long".to_string(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SpeechError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("auth error: {0}")]
    Auth(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("speech api returned {status}: {body}")]
    Api { status: u16, body: String },
}

pub struct GoogleSpeechTranscriptProvider {
    config: GoogleSpeechConfig,
    token_source: Arc<dyn google_cloud_token::TokenSource>,
    http: reqwest::Client,
    base_url: String,
}

impl GoogleSpeechTranscriptProvider {
    pub async fn new_adc(config: GoogleSpeechConfig) -> Result<Self, SpeechError> {
        Self::with_base_url(config, SPEECH_BASE_URL).await
    }

    async fn with_base_url(
        config: GoogleSpeechConfig,
        base_url: &str,
    ) -> Result<Self, SpeechError> {
        let scopes = [CLOUD_PLATFORM_SCOPE];
        let auth_config = google_cloud_auth::project::Config::default().with_scopes(&scopes);
        let provider = google_cloud_auth::token::DefaultTokenSourceProvider::new(auth_config)
            .await
            .map_err(|e| SpeechError::Auth(e.to_string()))?;
        Ok(Self {
            config,
            token_source: google_cloud_token::TokenSourceProvider::token_source(&provider),
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    async fn token(&self) -> Result<String, SpeechError> {
        let raw = self
            .token_source
            .token()
            .await
            .map_err(|e| SpeechError::Auth(e.to_string()))?;
        Ok(raw.strip_prefix("Bearer ").unwrap_or(&raw).to_string())
    }

    fn recognize_path(&self) -> String {
        format!(
            "/projects/{}/locations/{}/recognizers/_:recognize",
            self.config.project_id, self.config.location
        )
    }
}

#[derive(Debug, Serialize)]
struct RecognizeRequest {
    config: RecognitionConfig,
    content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecognitionConfig {
    auto_decoding_config: AutoDecodingConfig,
    language_codes: Vec<String>,
    model: String,
}

#[derive(Debug, Serialize)]
struct AutoDecodingConfig {}

#[derive(Debug, Deserialize)]
struct RecognizeResponse {
    #[serde(default)]
    results: Vec<SpeechRecognitionResult>,
}

#[derive(Debug, Deserialize)]
struct SpeechRecognitionResult {
    #[serde(default)]
    alternatives: Vec<SpeechRecognitionAlternative>,
}

#[derive(Debug, Deserialize)]
struct SpeechRecognitionAlternative {
    transcript: String,
}

#[async_trait]
impl TranscriptProvider for GoogleSpeechTranscriptProvider {
    async fn transcribe_file(&self, audio: &Path) -> anyhow::Result<Vec<TranscriptSegment>> {
        let bytes = std::fs::read(audio).map_err(|source| SpeechError::Io {
            path: audio.display().to_string(),
            source,
        })?;
        let body = RecognizeRequest {
            config: RecognitionConfig {
                auto_decoding_config: AutoDecodingConfig {},
                language_codes: vec![self.config.language_code.clone()],
                model: self.config.model.clone(),
            },
            content: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        let token = self.token().await?;
        let response = self
            .http
            .post(format!("{}{}", self.base_url, self.recognize_path()))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(SpeechError::Api {
                status: status.as_u16(),
                body: text,
            }
            .into());
        }
        let parsed: RecognizeResponse = serde_json::from_str(&text)?;
        let segments = parsed
            .results
            .into_iter()
            .enumerate()
            .filter_map(|(idx, result)| {
                let transcript = result.alternatives.into_iter().next()?.transcript;
                let text = transcript.trim();
                (!text.is_empty()).then(|| TranscriptSegment {
                    id: format!("segment_{}", idx + 1),
                    provider_sequence: idx + 1,
                    text: text.to_string(),
                })
            })
            .collect();
        Ok(segments)
    }
}
