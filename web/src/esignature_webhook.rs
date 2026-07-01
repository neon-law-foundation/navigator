//! Inbound e-signature completion webhook.
//!
//! The retainer workflow parks at `sent_for_signature__pending` after
//! the rendered engagement letter is sent for signature
//! (`web::signature::SignatureProvider::send_for_signature`). The
//! signature provider (DocuSign in the reference deploy) calls this
//! endpoint when the client finishes signing; we map that to a
//! `signature_received` signal on the notation's workflow object, which
//! advances it to `END` (the executed retainer).
//!
//! ## Trust model
//!
//! This endpoint *advances workflow state* from the public internet, so
//! it must not be trusted on URL alone — an unauthenticated POST here
//! would be a state-advancing forgery: the firm asserting a client
//! signed when they did not. Two layers gate it:
//!
//! - **Path secret** (`/webhook/esignature/:secret`) — coarse "is this
//!   our endpoint", mirroring the SendGrid webhooks. Optional
//!   defense-in-depth; `None` in dev/tests accepts any token.
//! - **HMAC-SHA256 over the raw body** (`X-DocuSign-Signature-1`) — the
//!   real gate. Verified *before* the JSON is parsed, over the exact
//!   bytes received, so the envelope id the body later names is
//!   trustworthy. When `esignature_hmac_key` is configured the header
//!   must be present and valid or the request is rejected (fail
//!   closed). `None` in dev/tests skips it. Production sets it via
//!   `enforce_prod_invariants`.
//!
//! Only a *completed* event advances state. DocuSign also fires `sent`,
//! `delivered`, `voided`, …; those return 200 without signalling so the
//! provider stops retrying and no half-states leak. An unknown envelope
//! id (no notation matches) is likewise a 200 no-op — the callback is
//! idempotent.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};

use store::entity::{notation, person};
use workflows::{MachineKind, StateMachineRuntime};

use crate::inbound_email::constant_time_eq;
use crate::webhook_auth::verify_hmac_sha256_b64;

/// DocuSign Connect's HMAC header. The "-1" suffix is the configured
/// HMAC key slot; DocuSign supports several, we use the first.
const SIGNATURE_HEADER: &str = "x-docusign-signature-1";

/// The `signature_received` condition the retainer workflow waits on at
/// `sent_for_signature__pending`.
const SIGNATURE_RECEIVED: &str = "signature_received";

/// The `signature_declined` condition — a declined or voided envelope
/// that will never be executed. Advances the same wait state to a
/// terminal end so the matter doesn't dead-end at
/// `sent_for_signature__pending`, and the journal records the decline.
const SIGNATURE_DECLINED: &str = "signature_declined";

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("unauthorized: webhook path secret mismatch")]
    UnauthorizedPath,
    #[error("unauthorized: missing or invalid signature")]
    UnauthorizedSignature,
    #[error("malformed callback payload: {0}")]
    Malformed(String),
    #[error("workflow runtime: {0}")]
    Runtime(String),
    #[error("database: {0}")]
    Database(String),
}

impl IntoResponse for WebhookError {
    fn into_response(self) -> axum::response::Response {
        let code = match &self {
            Self::UnauthorizedPath | Self::UnauthorizedSignature => StatusCode::UNAUTHORIZED,
            Self::Malformed(_) => StatusCode::BAD_REQUEST,
            Self::Runtime(_) | Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, self.to_string()).into_response()
    }
}

/// DocuSign Connect JSON callback. Tolerant subset — we read only the
/// envelope id and its status. The completion signal is either the
/// top-level `event` (`envelope-completed`) or
/// `data.envelopeSummary.status == "completed"`.
#[derive(serde::Deserialize)]
struct ConnectPayload {
    #[serde(default)]
    event: Option<String>,
    data: ConnectData,
}

#[derive(serde::Deserialize)]
struct ConnectData {
    #[serde(rename = "envelopeId")]
    envelope_id: String,
    #[serde(rename = "envelopeSummary", default)]
    envelope_summary: Option<EnvelopeSummary>,
}

#[derive(serde::Deserialize)]
struct EnvelopeSummary {
    #[serde(default)]
    status: Option<String>,
}

impl ConnectPayload {
    /// True when this callback reports the envelope as fully executed.
    fn is_completed(&self) -> bool {
        self.event.as_deref() == Some("envelope-completed")
            || self
                .envelope_summary_status()
                .is_some_and(|s| s.eq_ignore_ascii_case("completed"))
    }

    /// True when the envelope will never be executed — the client
    /// declined, or the envelope was voided. Either way the engagement
    /// did not form; the workflow leaves the wait state on
    /// `signature_declined`.
    fn is_declined(&self) -> bool {
        matches!(
            self.event.as_deref(),
            Some("envelope-declined" | "recipient-declined" | "envelope-voided")
        ) || self
            .envelope_summary_status()
            .is_some_and(|s| s.eq_ignore_ascii_case("declined") || s.eq_ignore_ascii_case("voided"))
    }

    fn envelope_summary_status(&self) -> Option<&str> {
        self.data
            .envelope_summary
            .as_ref()
            .and_then(|s| s.status.as_deref())
    }
}

/// Webhook handler. Verifies the path secret, then the raw-body HMAC,
/// then — only for a `completed` event whose envelope id matches a
/// notation — signals `signature_received`.
pub async fn webhook(
    State(state): State<crate::AppState>,
    Path(provided): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, WebhookError> {
    // 1. Path secret (coarse). Constant-time when configured.
    if let Some(configured) = state.esignature_webhook_secret.as_deref() {
        if !constant_time_eq(&provided, configured) {
            tracing::warn!("esignature webhook: path secret mismatch");
            return Err(WebhookError::UnauthorizedPath);
        }
    }

    // 2. HMAC over the raw body (the real gate). Verify BEFORE parsing
    //    so the digest covers the exact bytes that name the envelope.
    if let Some(key) = state.esignature_hmac_key.as_deref() {
        let provided_sig = headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                tracing::warn!("esignature webhook: signature header absent");
                WebhookError::UnauthorizedSignature
            })?;
        if !verify_hmac_sha256_b64(key.as_bytes(), &body, provided_sig) {
            tracing::warn!("esignature webhook: signature verification failed");
            return Err(WebhookError::UnauthorizedSignature);
        }
    }

    // 3. Parse — now that the bytes are trusted.
    let payload: ConnectPayload =
        serde_json::from_slice(&body).map_err(|e| WebhookError::Malformed(e.to_string()))?;

    // Map the terminal envelope events to workflow conditions. A
    // completion executes the retainer; a decline/void ends the wait
    // without an engagement. Everything else (sent / delivered /
    // viewed / …) is acked so the provider stops retrying, but does not
    // advance state.
    let condition = if payload.is_completed() {
        SIGNATURE_RECEIVED
    } else if payload.is_declined() {
        SIGNATURE_DECLINED
    } else {
        tracing::info!(
            envelope_id = %payload.data.envelope_id,
            event = ?payload.event,
            "esignature webhook: non-terminal event ignored"
        );
        return Ok(StatusCode::OK);
    };

    advance(&state, &payload.data.envelope_id, condition).await?;
    Ok(StatusCode::OK)
}

/// Resolve an envelope id to its notation and signal `condition`
/// (`signature_received` or `signature_declined`). An unknown envelope
/// id is a logged no-op (the callback may arrive for an envelope we
/// never tracked, or twice).
async fn advance(
    state: &crate::AppState,
    envelope_id: &str,
    condition: &str,
) -> Result<(), WebhookError> {
    let provider = store::entity::signature::SignatureProvider::DocuSign;
    let Some(signature) = store::signatures::by_provider(&state.db, provider, envelope_id)
        .await
        .map_err(|e| WebhookError::Database(e.to_string()))?
    else {
        tracing::warn!(%envelope_id, "esignature webhook: no signature for envelope id");
        return Ok(());
    };
    let Some(row) = notation::Entity::find_by_id(signature.notation_id)
        .one(&state.db)
        .await
        .map_err(|e| WebhookError::Database(e.to_string()))?
    else {
        tracing::warn!(%envelope_id, "esignature webhook: signature envelope points at a missing notation");
        return Ok(());
    };

    let notation_id = row.id;
    let project_id = row.project_id;
    let next = StateMachineRuntime::signal(
        state.workflow_runtime.as_ref(),
        MachineKind::Workflow,
        notation_id,
        condition,
        None,
    )
    .await
    .map_err(|e| WebhookError::Runtime(e.to_string()))?;

    // Mirror the workflow state into the `notations` row (the same
    // pattern the retainer walk uses) so the admin UI reflects the end.
    let mut active: notation::ActiveModel = row.into();
    active.state = ActiveValue::Set(next.as_str().to_string());
    active
        .update(&state.db)
        .await
        .map_err(|e| WebhookError::Database(e.to_string()))?;

    tracing::info!(%envelope_id, %condition, next_state = %next.as_str(), "esignature webhook: signature event");

    // On completion, archive the executed document set (signed PDF +
    // Certificate of Completion — the ESIGN record) to object storage.
    // Best-effort: the signature is already recorded, so an archive
    // failure must NOT fail the webhook (DocuSign would retry forever);
    // the GCS source-of-truth can be backfilled. Declines have no
    // executed document, so they skip this.
    if condition == SIGNATURE_RECEIVED {
        // Stamp the completion time on the signature so the executed record
        // carries when it was signed. Best-effort: the workflow already
        // advanced, so a stamp failure must not fail the webhook.
        if let Err(e) = store::signatures::stamp_signed(
            &state.db,
            provider,
            envelope_id,
            &chrono::Utc::now().to_rfc3339(),
        )
        .await
        {
            tracing::error!(
                %envelope_id, error = %e,
                "esignature webhook: stamping signed_at failed (signature still recorded)"
            );
        }

        if let Err(e) = archive_completed_documents(state, envelope_id, notation_id).await {
            tracing::error!(
                %envelope_id, error = %e,
                "esignature webhook: archiving signed documents failed (signature still recorded)"
            );
        }

        // A signed retainer makes the matter live: activate any recurring
        // subscriptions parked `pending` against this project, so the next
        // billing run picks them up. Best-effort — a client is never billed
        // before this fires, only (at worst) a run later if it errors.
        match store::subscriptions::activate_pending_for_project(&state.db, project_id).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(
                %project_id, activated = n,
                "esignature webhook: activated pending subscriptions on signature"
            ),
            Err(e) => tracing::error!(
                %project_id, error = %e,
                "esignature webhook: subscription activation failed (signature still recorded)"
            ),
        }
    }
    Ok(())
}

/// Download the signed PDF + Certificate of Completion from the provider
/// and store both in object storage (the source of truth; the Drive
/// mirror is the existing sync's job). Returns the provider/storage
/// error so the caller can log it without failing the webhook.
async fn archive_completed_documents(
    state: &crate::AppState,
    envelope_id: &str,
    notation_id: uuid::Uuid,
) -> Result<(), crate::signature::SignatureError> {
    use crate::signature::{SignatureError, SignatureRequestId};

    let docs = state
        .signature_provider
        .fetch_completed_documents(&SignatureRequestId(envelope_id.to_string()))
        .await?;

    // Additively file the executed PDFs into the matter repo with
    // attribution (one commit, both files), then capture the commit
    // event to the data lake. This borrows the bytes *before* the
    // fixed-key puts below move them, and is non-fatal: a failure here
    // never blocks archival, since the retrieval path reads the fixed
    // keys, not the repo. See docs/git-project-repos.md §8.
    commit_executed_to_repo(state, notation_id, &docs.signed_pdf, &docs.certificate_pdf).await;

    let put = |key: String, bytes: Vec<u8>| async move {
        state
            .storage
            .put(&key, &bytes, "application/pdf")
            .await
            .map_err(|e| SignatureError::Provider(e.to_string()))
    };
    put(
        crate::retainer_walk::signed_document_storage_key(notation_id),
        docs.signed_pdf,
    )
    .await?;
    put(
        crate::retainer_walk::certificate_of_completion_storage_key(notation_id),
        docs.certificate_pdf,
    )
    .await?;
    tracing::info!(%envelope_id, "esignature webhook: archived signed retainer + certificate");
    Ok(())
}

/// Commit the executed document PDF + Certificate of Completion into the
/// matter's repo, authored as the signing client, so `git log` records
/// the execution. Template-agnostic — the retainer, the trust, and any
/// future signed template land here. Best-effort: the executed documents
/// already live at their fixed storage keys (the retrieval path's source
/// of truth), so any failure here is logged and swallowed.
async fn commit_executed_to_repo(
    state: &crate::AppState,
    notation_id: uuid::Uuid,
    signed_pdf: &[u8],
    certificate_pdf: &[u8],
) {
    let Ok(Some(n)) = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
    else {
        return;
    };
    // Attribute to the client who signed; fall back to the matter itself
    // if the persons row can't be loaded.
    let (name, email) = match person::Entity::find_by_id(n.person_id).one(&state.db).await {
        Ok(Some(p)) => (p.name, p.email),
        _ => (
            "Neon Law Navigator".to_string(),
            "matter@localhost".to_string(),
        ),
    };
    crate::matter_documents::commit_files(
        &state.db,
        &state.storage,
        n.project_id,
        repos::Author {
            name: &name,
            email: &email,
        },
        "esignature",
        "executed",
        "esignature: executed signed document + certificate of completion",
        &[
            ("signed-document.pdf", signed_pdf),
            ("certificate-of-completion.pdf", certificate_pdf),
        ],
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::ConnectPayload;

    fn parse(json: serde_json::Value) -> ConnectPayload {
        serde_json::from_value(json).expect("connect payload parses")
    }

    #[test]
    fn completion_event_classifies_as_completed_only() {
        let p = parse(serde_json::json!({
            "event": "envelope-completed",
            "data": { "envelopeId": "e1" }
        }));
        assert!(p.is_completed());
        assert!(!p.is_declined());
    }

    #[test]
    fn completed_summary_status_classifies_as_completed() {
        let p = parse(serde_json::json!({
            "data": { "envelopeId": "e1", "envelopeSummary": { "status": "completed" } }
        }));
        assert!(p.is_completed());
        assert!(!p.is_declined());
    }

    #[test]
    fn declined_and_voided_events_classify_as_declined() {
        for ev in ["envelope-declined", "recipient-declined", "envelope-voided"] {
            let p = parse(serde_json::json!({ "event": ev, "data": { "envelopeId": "e1" } }));
            assert!(p.is_declined(), "{ev} should be declined");
            assert!(!p.is_completed(), "{ev} is not completed");
        }
    }

    #[test]
    fn declined_or_voided_summary_status_classifies_as_declined() {
        for st in ["declined", "voided", "Declined"] {
            let p = parse(serde_json::json!({
                "data": { "envelopeId": "e1", "envelopeSummary": { "status": st } }
            }));
            assert!(p.is_declined(), "status {st} should be declined");
        }
    }

    #[test]
    fn intermediate_events_classify_as_neither() {
        for ev in ["envelope-sent", "envelope-delivered", "recipient-completed"] {
            let p = parse(serde_json::json!({ "event": ev, "data": { "envelopeId": "e1" } }));
            assert!(!p.is_completed(), "{ev} is not completed");
            assert!(!p.is_declined(), "{ev} is not declined");
        }
    }
}
