use std::path::PathBuf;

use anyhow::{anyhow, bail};
use live_inquiry::{FakeTranscriptProvider, TranscriptProvider, TranscriptSource};

pub struct CoverArgs {
    pub template: PathBuf,
    pub transcript: Option<PathBuf>,
    pub audio: Option<PathBuf>,
    pub speech_backend: String,
    pub google_project: Option<String>,
    pub google_location: String,
    pub google_language: String,
    pub google_model: String,
    pub pretty: bool,
}

pub async fn cover(args: CoverArgs) -> anyhow::Result<()> {
    let output = match (args.transcript, args.audio) {
        (Some(transcript), None) => {
            live_inquiry::cover_transcript_file(&args.template, &transcript)?
        }
        (None, Some(audio)) => {
            let (provider, provider_label) = build_transcript_provider(
                &args.speech_backend,
                args.google_project,
                args.google_location,
                args.google_language,
                args.google_model,
            )
            .await?;
            let segments = provider.transcribe_file(&audio).await?;
            live_inquiry::cover_transcript_segments(
                &args.template,
                TranscriptSource::Audio {
                    path: audio.display().to_string(),
                    provider: provider_label,
                },
                segments,
            )?
        }
        (None, None) => bail!("pass either --transcript or --audio"),
        (Some(_), Some(_)) => bail!("pass only one of --transcript or --audio"),
    };

    if args.pretty {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", serde_json::to_string(&output)?);
    }
    Ok(())
}

/// Select the speech backend for the `--audio` path. The default is `fake`,
/// which transcribes with no cloud call (see
/// [`live_inquiry::FakeTranscriptProvider`]); `google` opts into real Google
/// Speech-to-Text v2 and therefore requires a project and credentials.
///
/// Returns the provider plus the label recorded in the coverage JSON's
/// `transcript_source.provider`, so the output never claims a real
/// transcription when the fake produced it.
async fn build_transcript_provider(
    backend: &str,
    google_project: Option<String>,
    google_location: String,
    google_language: String,
    google_model: String,
) -> anyhow::Result<(Box<dyn TranscriptProvider>, String)> {
    match backend {
        "fake" => Ok((Box::new(FakeTranscriptProvider::new()), "fake".to_string())),
        "google" | "gcp" => {
            let project_id = google_project
                .or_else(|| std::env::var("GCLOUD_PROJECT").ok())
                .or_else(|| std::env::var("NAVIGATOR_GCP_PROJECT_ID").ok())
                .ok_or_else(|| {
                    anyhow!(
                        "GOOGLE_CLOUD_PROJECT, GCLOUD_PROJECT, NAVIGATOR_GCP_PROJECT_ID, or --google-project is required with --speech-backend google"
                    )
                })?;
            let mut config = cloud::GoogleSpeechConfig::new(project_id);
            config.location = google_location;
            config.language_code = google_language;
            config.model = google_model;
            let provider = cloud::GoogleSpeechTranscriptProvider::new_adc(config).await?;
            Ok((Box::new(provider), "google-speech-to-text-v2".to_string()))
        }
        other => bail!(
            "unknown speech backend {other:?}: expected 'fake' (default) or 'google' \
             (set --speech-backend or NAVIGATOR_SPEECH_BACKEND)"
        ),
    }
}
