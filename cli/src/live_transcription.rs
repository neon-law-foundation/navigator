use std::path::PathBuf;

use anyhow::{anyhow, bail};
use live_inquiry::{TranscriptProvider, TranscriptSource};

pub struct CoverArgs {
    pub template: PathBuf,
    pub transcript: Option<PathBuf>,
    pub audio: Option<PathBuf>,
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
            let project_id = args
                .google_project
                .or_else(|| std::env::var("GCLOUD_PROJECT").ok())
                .or_else(|| std::env::var("NAVIGATOR_GCP_PROJECT_ID").ok())
                .ok_or_else(|| {
                    anyhow!(
                        "GOOGLE_CLOUD_PROJECT, GCLOUD_PROJECT, NAVIGATOR_GCP_PROJECT_ID, or --google-project is required with --audio"
                    )
                })?;
            let mut config = cloud::GoogleSpeechConfig::new(project_id);
            config.location = args.google_location;
            config.language_code = args.google_language;
            config.model = args.google_model;
            let provider = cloud::GoogleSpeechTranscriptProvider::new_adc(config).await?;
            let segments = provider.transcribe_file(&audio).await?;
            live_inquiry::cover_transcript_segments(
                &args.template,
                TranscriptSource::Audio {
                    path: audio.display().to_string(),
                    provider: "google-speech-to-text-v2".to_string(),
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
