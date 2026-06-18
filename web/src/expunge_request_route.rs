//! Client-initiated document deletion — request, then attorney
//! authorization (git-repos surfaces Task 2).
//!
//! The governed-expunge primitive is admin-only; a client can only
//! *ask*. This module wires both halves:
//!
//! - **Client (request-only).** `POST
//!   /portal/projects/:id/documents/:doc_id/request-deletion` records a
//!   `pending` [`store::expunge_requests`] row. Nothing is deleted. The
//!   client UI honestly shows "deletion requested" until an attorney
//!   acts — never "deleted" before the bytes are actually gone.
//! - **Staff/admin (authorize → execute).** `GET
//!   /portal/admin/expunge-requests` is the review queue; `POST
//!   .../:id/authorize` runs the admin-gated [`crate::expunge::expunge`]
//!   (category `client_request`) and links the audit row; `POST
//!   .../:id/deny` resolves it without deleting.
//!
//! No client-facing byte here ever mentions a repository — the surface
//! is about *documents*, not git.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use sea_orm::EntityTrait;
use store::entity::person::Role;
use store::entity::{blob, document, expunge_record, person, project};
use uuid::Uuid;

use crate::access::can_see_project;
use crate::admin::AdminState;
use crate::session::SessionData;
use views::pages::admin::expunge_requests as queue_views;

fn is_admin(session: Option<&SessionData>) -> bool {
    matches!(session.map(|s| s.role), Some(Role::Admin))
}

fn is_staff_tier(session: Option<&SessionData>) -> bool {
    !matches!(session.map(|s| s.role), Some(Role::Client))
}

fn csrf_token(session: Option<&SessionData>) -> &str {
    session.map_or("", |s| s.csrf_token.as_str())
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        views::internal_error_page(),
    )
        .into_response()
}

/// `POST /portal/projects/:id/documents/:doc_id/request-deletion` — a
/// client (or any matter participant) asks for a document to be deleted.
/// Row-scoped like the rest of `/portal`; creates one `pending` request
/// (idempotent — a second ask while one is pending is a no-op).
pub async fn client_request(
    State(state): State<AdminState>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let (person_id, role) = match session.as_deref() {
        Some(s) => (s.person_id, s.role),
        None => (None, Role::Client),
    };
    match can_see_project(&state.db, person_id, role, project_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(e) => {
            tracing::error!(error = %e, %project_id, "request-deletion: can_see_project failed");
            return internal_error();
        }
    }
    // Cross-project guard: the document must belong to this matter.
    let Ok(Some(doc)) = document::Entity::find_by_id(doc_id).one(&state.db).await else {
        return not_found();
    };
    if doc.project_id != project_id {
        return not_found();
    }
    let Some(requester) = person_id else {
        return (StatusCode::FORBIDDEN, "No linked person on the session.").into_response();
    };

    // Idempotent: don't stack duplicate pending requests for one document.
    match store::expunge_requests::pending_for_document(&state.db, doc_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(e) = store::expunge_requests::create(
                &state.db,
                &store::expunge_requests::NewExpungeRequest {
                    project_id,
                    document_id: doc_id,
                    requested_by_person_id: requester,
                    note: None,
                },
            )
            .await
            {
                tracing::error!(error = %e, %doc_id, "request-deletion: create failed");
                return internal_error();
            }
        }
        Err(e) => {
            tracing::error!(error = %e, %doc_id, "request-deletion: pending lookup failed");
            return internal_error();
        }
    }
    Redirect::to(&format!("/portal/projects/{project_id}")).into_response()
}

/// `GET /portal/admin/expunge-requests` — the staff/admin review queue of
/// pending client deletion requests.
pub async fn admin_queue(
    State(state): State<AdminState>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let pending = match store::expunge_requests::list_pending(&state.db).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "expunge queue: list_pending failed");
            return internal_error();
        }
    };

    // Resolve display fields per row (small queue — per-row lookups are
    // fine and avoid a bespoke join).
    let mut rows = Vec::with_capacity(pending.len());
    for req in &pending {
        let filename = document::Entity::find_by_id(req.document_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map_or_else(|| "(unknown document)".to_string(), |d| d.filename);
        let matter = project::Entity::find_by_id(req.project_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map_or_else(|| req.project_id.to_string(), |p| p.name);
        let requester = person::Entity::find_by_id(req.requested_by_person_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map_or_else(|| "(unknown)".to_string(), |p| p.name);
        rows.push(queue_views::Row {
            id: req.id,
            matter,
            filename,
            requester,
            requested_at: req.inserted_at.clone(),
        });
    }

    queue_views::list(
        &rows,
        csrf_token(session.as_deref()),
        is_admin(session.as_deref()),
    )
    .into_response()
}

/// `POST /portal/admin/expunge-requests/:id/authorize` — **admin only**:
/// run the governed expunge for the requested document, then mark the
/// request authorized and link the audit row.
pub async fn admin_authorize(
    State(state): State<AdminState>,
    Path(request_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    if !is_admin(session.as_deref()) {
        return not_found();
    }
    let Some(authorizer) = session.as_deref().and_then(|s| s.person_id) else {
        return (StatusCode::FORBIDDEN, "No linked person on the session.").into_response();
    };
    let Ok(Some(req)) = store::expunge_requests::by_id(&state.db, request_id).await else {
        return not_found();
    };
    if req.status != store::entity::expunge_request::STATUS_PENDING {
        // Already resolved — back to the queue rather than re-running.
        return Redirect::to("/portal/admin/expunge-requests").into_response();
    }
    // Resolve the document → repo path (filename) + storage key (blob).
    let Ok(Some(doc)) = document::Entity::find_by_id(req.document_id)
        .one(&state.db)
        .await
    else {
        return not_found();
    };
    let Ok(Some(blob_row)) = blob::Entity::find_by_id(doc.blob_id).one(&state.db).await else {
        return not_found();
    };

    let record_id = match crate::expunge::expunge(
        &state.db,
        &state.storage,
        crate::expunge::ExpungeRequest {
            project_id: req.project_id,
            path: &doc.filename,
            category: expunge_record::CATEGORY_CLIENT_REQUEST,
            authorized_by: authorizer,
            storage_key: Some(&blob_row.storage_key),
            note: None,
        },
    )
    .await
    {
        Ok(id) => id,
        Err(crate::expunge::ExpungeError::NotAdmin) => return not_found(),
        Err(e) => {
            tracing::error!(error = %e, %request_id, "authorize expunge request failed");
            return internal_error();
        }
    };

    if let Err(e) =
        store::expunge_requests::authorize(&state.db, request_id, authorizer, record_id).await
    {
        tracing::error!(error = %e, %request_id, "expunge executed but request status update failed");
        return internal_error();
    }
    Redirect::to("/portal/admin/expunge-requests").into_response()
}

/// `POST /portal/admin/expunge-requests/:id/deny` — staff or admin
/// resolve a request without deleting anything.
pub async fn admin_deny(
    State(state): State<AdminState>,
    Path(request_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    if !is_staff_tier(session.as_deref()) {
        return not_found();
    }
    let Some(resolver) = session.as_deref().and_then(|s| s.person_id) else {
        return (StatusCode::FORBIDDEN, "No linked person on the session.").into_response();
    };
    match store::expunge_requests::deny(&state.db, request_id, resolver).await {
        Ok(Some(_)) => Redirect::to("/portal/admin/expunge-requests").into_response(),
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!(error = %e, %request_id, "deny expunge request failed");
            internal_error()
        }
    }
}
