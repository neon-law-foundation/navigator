//! Admin governed-expunge HTTP surface (git-repos surfaces Task 1).
//!
//! - `GET  /portal/admin/documents/:doc_id/expunge` — the confirmation
//!   screen, naming the document + its storage key and demanding a
//!   category before the irreversible act.
//! - `POST /portal/admin/documents/:doc_id/expunge` — drives
//!   [`crate::expunge::expunge`] for the chosen document and shows the
//!   resulting audit-row id.
//!
//! # Authorization
//!
//! The surface is **admin-only**. Although the expunge primitive itself
//! re-checks that the authorizer is an admin (the gate lives in the
//! primitive, not the caller), this handler also 404s any non-admin
//! session *before* the dangerous screen renders, so the route's
//! existence isn't disclosed to staff or clients. In production the
//! admin sub-router's OPA layer already blocks unauthenticated traffic;
//! this is the role-tier check on top.
//!
//! The chosen `documents` row resolves to the repo path (its filename)
//! and the object-storage key (the joined `blobs` row's `storage_key`);
//! the authorizer is the acting admin's `persons` id from the session.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Form;
use sea_orm::EntityTrait;
use serde::Deserialize;
use store::entity::person::Role;
use store::entity::{blob, document, expunge_record};
use uuid::Uuid;

use crate::admin::AdminState;
use crate::session::SessionData;
use views::pages::admin::expunge as expunge_views;

/// True only for an `admin` session. Staff and clients are treated as
/// if the route did not exist.
fn is_admin(session: Option<&SessionData>) -> bool {
    matches!(session.map(|s| s.role), Some(Role::Admin))
}

fn csrf_token(session: Option<&SessionData>) -> &str {
    session.map_or("", |s| s.csrf_token.as_str())
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, views::not_found_page()).into_response()
}

/// Map a posted category string to one of the canonical
/// `expunge_record::CATEGORY_*` constants, or `None` if unrecognized.
fn canonical_category(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        expunge_record::CATEGORY_PRIVILEGE => Some(expunge_record::CATEGORY_PRIVILEGE),
        expunge_record::CATEGORY_SEALING => Some(expunge_record::CATEGORY_SEALING),
        expunge_record::CATEGORY_CLIENT_REQUEST => Some(expunge_record::CATEGORY_CLIENT_REQUEST),
        _ => None,
    }
}

/// Resolve a document to `(document, blob)`, where the document's
/// `filename` is the repo path to expunge and the blob's `storage_key`
/// is the object-storage key to delete. `None` if either row is missing.
async fn load_doc(state: &AdminState, doc_id: Uuid) -> Option<(document::Model, blob::Model)> {
    let doc = document::Entity::find_by_id(doc_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    let blob_row = blob::Entity::find_by_id(doc.blob_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    Some((doc, blob_row))
}

/// `GET /portal/admin/documents/:doc_id/expunge`.
pub async fn confirm(
    State(state): State<AdminState>,
    Path(doc_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    if !is_admin(session.as_deref()) {
        return not_found();
    }
    let Some((doc, blob_row)) = load_doc(&state, doc_id).await else {
        return not_found();
    };
    expunge_views::confirm(&expunge_views::Confirm {
        doc_id,
        project_id: doc.project_id,
        filename: &doc.filename,
        storage_key: &blob_row.storage_key,
        csrf_token: csrf_token(session.as_deref()),
        error: None,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct ExpungeForm {
    category: String,
    #[serde(default)]
    note: String,
}

/// `POST /portal/admin/documents/:doc_id/expunge`.
pub async fn run(
    State(state): State<AdminState>,
    Path(doc_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
    Form(input): Form<ExpungeForm>,
) -> Response {
    if !is_admin(session.as_deref()) {
        return not_found();
    }
    let Some((doc, blob_row)) = load_doc(&state, doc_id).await else {
        return not_found();
    };

    // Validate the category at the edge so a bad value re-renders the
    // form instead of bubbling a primitive error up as a 500.
    let Some(category) = canonical_category(&input.category) else {
        return (
            StatusCode::BAD_REQUEST,
            expunge_views::confirm(&expunge_views::Confirm {
                doc_id,
                project_id: doc.project_id,
                filename: &doc.filename,
                storage_key: &blob_row.storage_key,
                csrf_token: csrf_token(session.as_deref()),
                error: Some("Choose one of the listed expunge categories."),
            }),
        )
            .into_response();
    };

    // The authorizer must be a known person — the audit row records who.
    let Some(authorized_by) = session.as_deref().and_then(|s| s.person_id) else {
        return (
            StatusCode::FORBIDDEN,
            "No linked person on the session; cannot attribute the expunge.",
        )
            .into_response();
    };

    let note = input.note.trim();
    let note = (!note.is_empty()).then_some(note);

    match crate::expunge::expunge(
        &state.db,
        &state.storage,
        crate::expunge::ExpungeRequest {
            project_id: doc.project_id,
            path: &doc.filename,
            category,
            authorized_by,
            storage_key: Some(&blob_row.storage_key),
            note,
        },
    )
    .await
    {
        Ok(record_id) => expunge_views::result(&expunge_views::Result {
            record_id,
            project_id: doc.project_id,
            filename: &doc.filename,
            category,
        })
        .into_response(),
        // The primitive's own admin gate — should be unreachable behind
        // `is_admin`, but map it honestly rather than as a 500.
        Err(crate::expunge::ExpungeError::NotAdmin) => not_found(),
        Err(e) => {
            tracing::error!(error = %e, %doc_id, project_id = %doc.project_id,
                "governed expunge failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response()
        }
    }
}
