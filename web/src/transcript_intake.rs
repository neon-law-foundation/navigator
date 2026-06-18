//! Northstar transcript-upload surface.
//!
//! `POST /portal/projects/:id/notations/:nid/transcript` — the
//! staff/agent surface that files a sitting's transcript into an estate
//! matter. The sitting is recorded offline and transcribed by Ada on the
//! already-paid Google Gemini Enterprise (no live speech-to-text); the
//! resulting transcript is uploaded here.
//!
//! Phone-friendly capture by design (client council): the form accepts a
//! **text** paste, a **file**, or a **link** — never "scan a PDF". The
//! handler builds a [`workflows::IntakePayload`] and threads it through
//! the workflow's `transcript_uploaded` signal; the durable runtime
//! (Restate in prod, the in-process `DispatchingRuntime` in dev/tests)
//! lands on `document_intake__transcript` and files the artifact into the
//! matter via the reusable document-intake step — the same way the
//! retainer threads a `DocumentPayload` through `approved` rather than
//! rendering inline. Durability stays Restate's; `web` only triggers.
//!
//! Authorization: this sits under the admin sub-router, so CSRF, session,
//! and OPA policy are already enforced before the handler runs. A
//! cross-resource guard then 404s if the notation doesn't belong to the
//! `:id` project, so a stolen notation id can't be tunnelled through
//! another project's URL.

use axum::extract::{Multipart, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity::notation;
use uuid::Uuid;
use workflows::{IntakeArtifact, IntakePayload, MachineKind, StateMachineRuntime};

/// The condition that, fired from `BEGIN`, lands the estate workflow on
/// `document_intake__transcript`. Kept in one place so the handler, the
/// matter-creation flow (which detects a transcript-driven onboarding
/// template by this edge out of `BEGIN`), and its test all agree.
pub(crate) const TRANSCRIPT_UPLOADED: &str = "transcript_uploaded";

/// `POST /portal/projects/:id/notations/:nid/transcript`.
pub async fn upload(
    State(state): State<crate::admin::AdminState>,
    AxumPath((project_id, notation_id)): AxumPath<(Uuid, Uuid)>,
    multipart: Multipart,
) -> Response {
    // Cross-resource guard: the notation must exist and belong to the
    // project named in the URL.
    let Ok(Some(notation_row)) = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if notation_row.project_id != project_id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let redirect_back = format!("/portal/projects/{project_id}");

    let Some(form) = parse_form(multipart).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(artifact) = form.into_artifact() else {
        // Nothing usable was provided — bounce back to the matter rather
        // than firing an empty intake.
        return Redirect::to(&redirect_back).into_response();
    };

    // Capture the transcript text for extraction before the artifact is
    // converted into the workflow payload.
    let transcript_text = artifact.transcript_text();

    let payload = IntakePayload {
        kind: "transcript".to_string(),
        filename: artifact.default_filename(),
        artifact: artifact.into_workflow_artifact(),
    };
    let value = match serde_json::to_string(&payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "serialize transcript IntakePayload");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Thread the artifact through the workflow signal; the runtime's
    // document-intake dispatch files it. The runtime is the source of
    // truth, so we mirror its returned state onto the notation row.
    match StateMachineRuntime::signal(
        state.workflow_runtime.as_ref(),
        MachineKind::Workflow,
        notation_id,
        TRANSCRIPT_UPLOADED,
        Some(&value),
    )
    .await
    {
        Ok(next) => {
            if let Err(e) = sync_notation_state(&state.db, notation_id, next.as_str()).await {
                tracing::warn!(error = %e, %notation_id, "transcript filed but state sync failed");
            }
            // The transcript is filed; now drive the estate pipeline
            // (extract → drafts → staff_review) so the attorney lands on
            // the matter with drafts ready. Best-effort: a pipeline hiccup
            // is logged but never 500s the upload — the transcript is
            // already filed and re-running is safe.
            let extractor = crate::estate::StubEstateExtractor;
            if let Err(e) = crate::estate::drive_estate_pipeline(
                &state,
                notation_id,
                transcript_text.as_deref().unwrap_or(""),
                &extractor,
            )
            .await
            {
                tracing::error!(error = %e, %notation_id, "estate pipeline after transcript upload failed");
            }
            Redirect::to(&redirect_back).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "transcript intake signal failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// One of the three phone-friendly capture modes, after parsing.
enum ParsedArtifact {
    Text(String),
    File {
        bytes: Vec<u8>,
        content_type: String,
        filename: String,
    },
    Link(String),
}

impl ParsedArtifact {
    /// Filename recorded on the `documents` row.
    fn default_filename(&self) -> String {
        match self {
            Self::Text(_) => "sitting-transcript.txt".to_string(),
            Self::File { filename, .. } => filename.clone(),
            Self::Link(_) => "sitting-recording.url".to_string(),
        }
    }

    /// The transcript text the extractor reads, when we have it: a paste
    /// directly, a file decoded as UTF-8, or `None` for a bare link (the
    /// recording lives elsewhere — extraction yields an all-unanswered
    /// coverage report and blank drafts until staff fill them).
    fn transcript_text(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::File { bytes, .. } => String::from_utf8(bytes.clone()).ok(),
            Self::Link(_) => None,
        }
    }

    fn into_workflow_artifact(self) -> IntakeArtifact {
        match self {
            Self::Text(text) => IntakeArtifact::Text { text },
            Self::File {
                bytes,
                content_type,
                ..
            } => IntakeArtifact::File {
                bytes_base64: base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    bytes,
                ),
                content_type,
            },
            Self::Link(url) => IntakeArtifact::Link { url },
        }
    }
}

/// Collected multipart fields before mode selection.
#[derive(Default)]
struct TranscriptForm {
    text: Option<String>,
    link: Option<String>,
    file: Option<(Vec<u8>, String, String)>,
}

impl TranscriptForm {
    /// Pick the provided mode, preferring a file, then a text paste, then
    /// a link — the order of richness. Blank fields are ignored so an
    /// empty text box next to a real link doesn't win.
    fn into_artifact(self) -> Option<ParsedArtifact> {
        if let Some((bytes, content_type, filename)) = self.file {
            if !bytes.is_empty() {
                return Some(ParsedArtifact::File {
                    bytes,
                    content_type,
                    filename,
                });
            }
        }
        if let Some(text) = self
            .text
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            return Some(ParsedArtifact::Text(text));
        }
        if let Some(link) = self
            .link
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
        {
            return Some(ParsedArtifact::Link(link));
        }
        None
    }
}

/// Read every multipart field into a [`TranscriptForm`]. Returns `None`
/// only on a malformed multipart body.
async fn parse_form(mut multipart: Multipart) -> Option<TranscriptForm> {
    let mut form = TranscriptForm::default();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return None,
        };
        match field.name().map(String::from).as_deref() {
            Some("transcript_text") => form.text = field.text().await.ok(),
            Some("link") => form.link = field.text().await.ok(),
            Some("file") => {
                let filename = field
                    .file_name()
                    .map_or_else(|| "sitting-transcript".to_string(), String::from);
                let content_type = field
                    .content_type()
                    .map_or_else(|| "application/octet-stream".to_string(), String::from);
                if let Ok(bytes) = field.bytes().await {
                    form.file = Some((bytes.to_vec(), content_type, filename));
                }
            }
            _ => {}
        }
    }
    Some(form)
}

/// Mirror the runtime's resulting state onto the `notations` row (the
/// runtime is the source of truth; the row is a convenience read).
async fn sync_notation_state(
    db: &store::Db,
    notation_id: Uuid,
    new_state: &str,
) -> Result<(), sea_orm::DbErr> {
    let existing = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("notation {notation_id}")))?;
    let mut active: notation::ActiveModel = existing.into();
    active.state = ActiveValue::Set(new_state.to_string());
    active.update(db).await?;
    Ok(())
}
