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
        Ok(Self::from_parts(
            config,
            base_url,
            google_cloud_token::TokenSourceProvider::token_source(&provider),
        ))
    }

    /// Construct from explicit parts. This is the dependency-injection seam:
    /// [`new_adc`](Self::new_adc) feeds it a real ADC token source and the
    /// production base URL, while tests feed it a fake token source and a
    /// mock server's URL so the request/response path runs with no ADC and
    /// no real Google call.
    fn from_parts(
        config: GoogleSpeechConfig,
        base_url: &str,
        token_source: Arc<dyn google_cloud_token::TokenSource>,
    ) -> Self {
        Self {
            config,
            token_source,
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
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
    explicit_decoding_config: ExplicitDecodingConfig,
    language_codes: Vec<String>,
    model: String,
}

/// We decode the input ourselves (see [`crate::audio`]) and hand Google raw
/// 16-bit mono PCM, so the request always pins `LINEAR16` rather than asking
/// the API to guess the container — which it cannot do for AAC/ALAC `.m4a`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExplicitDecodingConfig {
    encoding: &'static str,
    sample_rate_hertz: u32,
    audio_channel_count: u32,
}

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
        // Decode whatever the caller handed us (m4a/AAC, mp3, flac, wav, ogg,
        // …) into 16-bit mono PCM so the codec never has to be named and the
        // API never has to guess the container.
        let decoded = crate::audio::decode_to_mono_pcm16(audio)?;
        let mut pcm = Vec::with_capacity(decoded.samples.len() * 2);
        for sample in &decoded.samples {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let body = RecognizeRequest {
            config: RecognitionConfig {
                explicit_decoding_config: ExplicitDecodingConfig {
                    encoding: "LINEAR16",
                    sample_rate_hertz: decoded.sample_rate,
                    audio_channel_count: 1,
                },
                language_codes: vec![self.config.language_code.clone()],
                model: self.config.model.clone(),
            },
            content: base64::engine::general_purpose::STANDARD.encode(&pcm),
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

#[cfg(test)]
mod tests {
    use super::{GoogleSpeechConfig, GoogleSpeechTranscriptProvider, SpeechError};
    use live_inquiry::TranscriptProvider;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A token source that returns a fixed dummy bearer string — never a real
    /// credential. Lets the provider build its `Authorization` header offline.
    #[derive(Debug)]
    struct FakeTokenSource;

    #[async_trait::async_trait]
    impl google_cloud_token::TokenSource for FakeTokenSource {
        async fn token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("Bearer test-token".to_string())
        }
    }

    /// Write a minimal 16-bit mono PCM WAV so `transcribe_file` runs the real
    /// Symphonia decode path — the test exercises decode → request, not a stub.
    fn write_temp_wav(sample_rate: u32, frames: u32) -> NamedTempFile {
        let data_len = frames * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let s: i16 = if i % 2 == 0 { 6000 } else { -6000 };
            wav.extend_from_slice(&s.to_le_bytes());
        }
        let mut tmp = tempfile::Builder::new()
            .suffix(".wav")
            .tempfile()
            .expect("tempfile");
        tmp.write_all(&wav).expect("write wav");
        tmp.flush().expect("flush");
        tmp
    }

    fn provider_for(base_url: &str, project: &str) -> GoogleSpeechTranscriptProvider {
        GoogleSpeechTranscriptProvider::from_parts(
            GoogleSpeechConfig::new(project.to_string()),
            base_url,
            Arc::new(FakeTokenSource),
        )
    }

    #[tokio::test]
    async fn builds_linear16_request_and_parses_segments() {
        let server = MockServer::start().await;
        let response = serde_json::json!({
            "results": [
                { "alternatives": [{ "transcript": "hello world" }] },
                { "alternatives": [{ "transcript": "  " }] }, // whitespace → dropped
                { "alternatives": [{ "transcript": "second segment" }] }
            ]
        });
        Mock::given(method("POST"))
            .and(path(
                "/projects/test-proj/locations/global/recognizers/_:recognize",
            ))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;

        let provider = provider_for(&server.uri(), "test-proj");
        let wav = write_temp_wav(8000, 800);
        let segments = provider
            .transcribe_file(wav.path())
            .await
            .expect("transcribe should succeed against the mock");

        // Whitespace-only alternative is filtered; the two real ones survive.
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "hello world");
        assert_eq!(segments[0].provider_sequence, 1);
        assert_eq!(segments[1].text, "second segment");
        // Index 2 in the response (the 3rd result) keeps its original sequence.
        assert_eq!(segments[1].provider_sequence, 3);

        // The request we sent must pin explicit LINEAR16 decoding, the WAV's
        // native rate, mono, the configured language — and carry real PCM.
        let received = &server.received_requests().await.expect("requests")[0];
        let body: serde_json::Value = serde_json::from_slice(&received.body).expect("json body");
        assert_eq!(
            body["config"]["explicitDecodingConfig"]["encoding"],
            "LINEAR16"
        );
        assert_eq!(
            body["config"]["explicitDecodingConfig"]["sampleRateHertz"],
            8000
        );
        assert_eq!(
            body["config"]["explicitDecodingConfig"]["audioChannelCount"],
            1
        );
        assert_eq!(body["config"]["languageCodes"][0], "en-US");
        assert_eq!(body["config"]["model"], "latest_long");
        assert!(
            !body["content"].as_str().expect("content string").is_empty(),
            "request must carry base64 PCM content"
        );
    }

    #[tokio::test]
    async fn maps_non_success_status_to_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400).set_body_string("{\"error\":{\"message\":\"nope\"}}"),
            )
            .mount(&server)
            .await;

        let provider = provider_for(&server.uri(), "p");
        let wav = write_temp_wav(8000, 400);
        let err = provider
            .transcribe_file(wav.path())
            .await
            .expect_err("a 400 must surface as an error");
        let speech_err = err
            .downcast_ref::<SpeechError>()
            .expect("error should be a SpeechError");
        assert!(
            matches!(speech_err, SpeechError::Api { status: 400, .. }),
            "expected SpeechError::Api(400), got {speech_err:?}"
        );
    }

    /// An empty `results` array is a valid response and must yield zero
    /// segments (not an error) — and the request still carries the expected
    /// top-level shape.
    #[tokio::test]
    async fn empty_results_yields_no_segments() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let provider = provider_for(&server.uri(), "p");
        let wav = write_temp_wav(16000, 320);
        let segments = provider
            .transcribe_file(wav.path())
            .await
            .expect("empty results is valid and yields no segments");
        assert!(segments.is_empty());

        let received = &server.received_requests().await.expect("requests")[0];
        let body: serde_json::Value = serde_json::from_slice(&received.body).expect("json body");
        assert!(body["config"]["explicitDecodingConfig"].is_object());
        assert!(body["content"].is_string());
    }
}
