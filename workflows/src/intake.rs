//! `document_intake__<slug>` step dispatch — file a provided artifact
//! into the matter.
//!
//! The inbound mirror of [`crate::document`]'s `document_open__*` step:
//! where `document_open` *renders* a document and persists it,
//! `document_intake` takes an artifact a human or agent already
//! *provides* and files it. The caller threads an [`IntakePayload`]
//! through the signal `value`, and the worker (the `workflows-service`
//! `NotationService` in prod, the in-process [`crate::DispatchingRuntime`]
//! in dev/tests) writes a content-addressed blob + `documents` row via
//! the shared [`store::documents::ingest_bytes`] seam — the same write
//! the e-sign and inbound-email intake lanes use. One
//! abstraction, many instances:
//!
//! - **transcript** (Northstar estate) — the first instance. The sitting
//!   is recorded offline and transcribed by AIDA on the already-paid
//!   Google Gemini Enterprise (~$0 marginal cost, the access-to-justice
//!   lever); the transcript text is then uploaded here.
//! - **executed e-sign PDF** — the signed instrument filed back into the
//!   matter repo (folds the existing matter-document write onto one
//!   path).
//! - future: ID scans, evidence uploads, collector letters (Nautilus
//!   inbound).
//!
//! ## Phone-friendly capture
//!
//! [`IntakeArtifact`] accepts what a phone has — a **text** paste, a
//! **file**, or a **link** — never "scan a PDF" (the client council's
//! Pisces bails there). A link is recorded as a `text/uri-list` pointer
//! document; we store what was provided, we do not fetch it.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::spec::StateName;
use crate::step::{step_kind_for, StepKind};

/// The artifact a human or agent provides for a document-intake step.
/// Internally tagged on `kind` so `web` and the worker share one wire
/// shape, exactly like [`crate::DocumentPayload`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntakeArtifact {
    /// A text paste — the transcript text, a typed note. Stored as
    /// UTF-8 `text/plain`.
    Text { text: String },
    /// Raw file bytes (base64-encoded for JSON transport) with their
    /// content type — an audio voice memo, a `.txt`/`.docx` export.
    File {
        bytes_base64: String,
        content_type: String,
    },
    /// An external link (a Zoom recording URL, a shared-drive link).
    /// Stored as a `text/uri-list` pointer; we record what was provided.
    Link { url: String },
}

impl IntakeArtifact {
    /// Resolve the artifact to the bytes + content type that land in
    /// storage. The only fallible arm is [`IntakeArtifact::File`], whose
    /// base64 may be malformed.
    fn to_bytes(&self) -> Result<(Vec<u8>, String), IntakeError> {
        match self {
            Self::Text { text } => Ok((text.clone().into_bytes(), "text/plain".to_string())),
            Self::File {
                bytes_base64,
                content_type,
            } => {
                let bytes = BASE64.decode(bytes_base64)?;
                Ok((bytes, content_type.clone()))
            }
            Self::Link { url } => Ok((url.clone().into_bytes(), "text/uri-list".to_string())),
        }
    }
}

/// Everything the worker needs to file one provided artifact into the
/// matter. Threaded (JSON) as the `value` of the signal that lands on a
/// `document_intake__<slug>` state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakePayload {
    /// Document kind within the matter (`transcript`, `executed_pdf`, …)
    /// — the slug after `document_intake__`. Goes into `documents.kind`.
    pub kind: String,
    /// Caller-visible filename / title. Goes into `documents.filename`.
    pub filename: String,
    /// The provided artifact.
    pub artifact: IntakeArtifact,
}

/// Errors from filing a document-intake artifact.
#[derive(Debug, thiserror::Error)]
pub enum IntakeError {
    #[error("decode base64 file bytes: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("notation {0} not found — cannot resolve its project")]
    NotationNotFound(Uuid),
    #[error("ingest: {0}")]
    Ingest(#[from] store::documents::IngestError),
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// True for the document-intake step family
/// (`document_intake__<slug>`), the reusable provided-artifact step.
#[must_use]
pub fn is_document_intake(state: &StateName) -> bool {
    matches!(step_kind_for(state), Some(StepKind::DocumentIntake))
}

/// File the provided artifact into the matter. The single side effect of
/// a document-intake step; callers wrap it in `ctx.run` (worker) or call
/// it inline ([`crate::DispatchingRuntime`]). Resolves the notation's
/// project, turns the artifact into bytes, and writes the
/// content-addressed blob + `documents` row through
/// [`store::documents::ingest_bytes`]. Idempotent by construction: the
/// same bytes dedup to the same blob, so a Restate replay is safe.
pub async fn dispatch_document_intake(
    db: &store::Db,
    storage: &Arc<dyn cloud::StorageService>,
    notation_id: Uuid,
    payload: &IntakePayload,
) -> Result<store::documents::IngestedDocument, IntakeError> {
    let project_id = notation_project_id(db, notation_id).await?;
    let (bytes, content_type) = payload.artifact.to_bytes()?;
    let args = store::documents::IngestArgs {
        project_id,
        source: store::documents::source::UPLOAD,
        filename: &payload.filename,
        kind: &payload.kind,
        content_type: &content_type,
        description: None,
        source_revision_id: None,
    };
    Ok(store::documents::ingest_bytes(db, storage, &args, &bytes).await?)
}

/// Resolve a notation to the project it belongs to (every notation
/// belongs to exactly one project). [`store::documents::ingest_bytes`]
/// is project-scoped, but step dispatch only carries the notation id.
async fn notation_project_id(db: &store::Db, notation_id: Uuid) -> Result<Uuid, IntakeError> {
    store::entity::notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .map(|n| n.project_id)
        .ok_or(IntakeError::NotationNotFound(notation_id))
}

#[cfg(test)]
mod tests {
    use super::{IntakeArtifact, IntakePayload};

    #[test]
    fn artifact_is_internally_tagged_on_kind() {
        // Pin the wire shape so web and the worker stay in sync, exactly
        // like the DocumentPayload tag test.
        let text = serde_json::to_value(IntakeArtifact::Text { text: "hi".into() }).unwrap();
        assert_eq!(text["kind"], "text");
        let file = serde_json::to_value(IntakeArtifact::File {
            bytes_base64: "AA==".into(),
            content_type: "audio/m4a".into(),
        })
        .unwrap();
        assert_eq!(file["kind"], "file");
        let link = serde_json::to_value(IntakeArtifact::Link {
            url: "https://zoom.example/rec/123".into(),
        })
        .unwrap();
        assert_eq!(link["kind"], "link");
    }

    #[test]
    fn text_artifact_resolves_to_utf8_plain_bytes() {
        let (bytes, ct) = IntakeArtifact::Text {
            text: "the sitting transcript".into(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(bytes, b"the sitting transcript");
        assert_eq!(ct, "text/plain");
    }

    #[test]
    fn file_artifact_base64_round_trips_to_raw_bytes() {
        let (bytes, ct) = IntakeArtifact::File {
            // base64("hello") = aGVsbG8=
            bytes_base64: "aGVsbG8=".into(),
            content_type: "application/octet-stream".into(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(bytes, b"hello");
        assert_eq!(ct, "application/octet-stream");
    }

    #[test]
    fn link_artifact_resolves_to_a_uri_list_pointer() {
        let (bytes, ct) = IntakeArtifact::Link {
            url: "https://zoom.example/rec/123".into(),
        }
        .to_bytes()
        .unwrap();
        assert_eq!(bytes, b"https://zoom.example/rec/123");
        assert_eq!(ct, "text/uri-list");
    }

    #[test]
    fn malformed_base64_file_artifact_errors() {
        let err = IntakeArtifact::File {
            bytes_base64: "not base64 !!!".into(),
            content_type: "text/plain".into(),
        }
        .to_bytes()
        .unwrap_err();
        assert!(matches!(err, super::IntakeError::Base64(_)));
    }

    #[test]
    fn payload_round_trips_through_json() {
        let payload = IntakePayload {
            kind: "transcript".into(),
            filename: "sitting-transcript.txt".into(),
            artifact: IntakeArtifact::Text {
                text: "consent given; people named".into(),
            },
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: IntakePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back, payload);
    }
}
