//! `/portal/projects/:id/review/:doc_id` — the comment-only client
//! review surface (Northstar Phase A).
//!
//! A client reads one attorney-reviewed draft (a will, a trust, a
//! directive) and leaves comments anchored to a text range. The surface
//! is read-only: a comment is the only thing the client writes. The page
//! is row-scoped to the matter exactly like the rest of `/portal/*` — a
//! non-participant gets `404`, never `403`. A draft is only reachable
//! once an attorney has advanced it past `draft` (the human-in-the-loop
//! gate the marketing copy promises): a `draft`-status row 404s here.
//!
//! Three routes:
//!
//! - `GET …/review/:doc_id` — the read-only document + comment sidebar.
//! - `POST …/review/:doc_id/comments` — create one anchored comment
//!   (form-encoded, CSRF-checked, comes from the viewer element).
//! - `GET …/review/:doc_id/comments` — the comment thread as JSON, so
//!   the viewer can refresh without a full reload.

use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::entity::person::Role;
use store::entity::review_document::STATUS_DRAFT;
use store::entity::{notation, person, review_document};
use store::Db;

use crate::access::can_see_project;
use crate::session::SessionData;

/// Resolve `(project_id, doc_id)` to a client-visible review document,
/// or a `404` response. Enforces, in order: the document exists, it
/// belongs to a notation in *this* project, the caller may see the
/// project, and the draft has been advanced past `draft` so a client
/// never sees an un-reviewed document.
async fn visible_review_document(
    db: &Db,
    session: &SessionData,
    project_id: Uuid,
    doc_id: Uuid,
) -> Result<review_document::Model, Response> {
    let doc = review_document::Entity::find_by_id(doc_id)
        .one(db)
        .await
        .ok()
        .flatten()
        .ok_or_else(not_found)?;

    let notation = notation::Entity::find_by_id(doc.notation_id)
        .one(db)
        .await
        .ok()
        .flatten()
        .ok_or_else(not_found)?;
    if notation.project_id != project_id {
        return Err(not_found());
    }

    let visible = can_see_project(db, session.person_id, session.role, project_id)
        .await
        .unwrap_or(false);
    if !visible {
        return Err(not_found());
    }

    if doc.status == STATUS_DRAFT {
        return Err(not_found());
    }
    Ok(doc)
}

/// `GET /portal/projects/:id/review/:doc_id`.
pub async fn review_page(
    State(db): State<Db>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let doc = match visible_review_document(&db, &session, project_id, doc_id).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };
    let comments = load_comments(&db, doc.id).await;
    views::pages::portal::review::render(&views::pages::portal::review::ReviewPage {
        project_id,
        doc_id: doc.id,
        title: &doc.title,
        kind: &doc.kind,
        status: &doc.status,
        body_html: &doc.body_html,
        comments_json: &serde_json::to_string(&comments).unwrap_or_else(|_| "[]".into()),
        csrf_token: &session.csrf_token,
    })
    .into_response()
}

/// One comment, shaped for the viewer's JSON contract.
#[derive(Debug, Serialize)]
pub struct CommentJson {
    pub id: Uuid,
    pub anchor_start: i32,
    pub anchor_end: i32,
    pub quoted_text: String,
    pub body: String,
    pub resolved: bool,
    pub author: String,
    pub inserted_at: String,
}

/// Load a document's comments with author display names resolved in one
/// batched query (no N+1).
async fn load_comments(db: &Db, doc_id: Uuid) -> Vec<CommentJson> {
    let rows = store::document_comments::for_review_document(db, doc_id)
        .await
        .unwrap_or_default();
    let author_ids: Vec<Uuid> = rows.iter().map(|c| c.person_id).collect();
    let names: HashMap<Uuid, String> = if author_ids.is_empty() {
        HashMap::new()
    } else {
        person::Entity::find()
            .filter(person::Column::Id.is_in(author_ids))
            .all(db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect()
    };
    rows.into_iter()
        .map(|c| CommentJson {
            author: names.get(&c.person_id).cloned().unwrap_or_default(),
            id: c.id,
            anchor_start: c.anchor_start,
            anchor_end: c.anchor_end,
            quoted_text: c.quoted_text,
            body: c.body,
            resolved: c.resolved,
            inserted_at: c.inserted_at,
        })
        .collect()
}

/// `GET /portal/projects/:id/review/:doc_id/comments` — the thread as
/// JSON.
pub async fn list_comments(
    State(db): State<Db>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let doc = match visible_review_document(&db, &session, project_id, doc_id).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };
    Json(load_comments(&db, doc.id).await).into_response()
}

/// Posted by the viewer when a reader anchors a comment to a selection.
#[derive(Debug, Deserialize)]
pub struct CommentForm {
    /// CSRF token — verified by the middleware before the handler runs;
    /// accepted here only so the form body parses.
    #[serde(rename = "_csrf", default)]
    pub csrf: String,
    pub anchor_start: i32,
    pub anchor_end: i32,
    pub quoted_text: String,
    pub body: String,
}

/// `POST /portal/projects/:id/review/:doc_id/comments` — create one
/// anchored comment and return the refreshed thread as JSON.
pub async fn create_comment(
    State(db): State<Db>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
    axum::Form(form): axum::Form<CommentForm>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let doc = match visible_review_document(&db, &session, project_id, doc_id).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };
    // A comment must be attributable to a person; an anonymous session
    // can't author one.
    let Some(person_id) = session.person_id else {
        return not_found();
    };
    let body = form.body.trim();
    if body.is_empty() || form.anchor_end <= form.anchor_start {
        return (StatusCode::BAD_REQUEST, "empty comment or invalid range").into_response();
    }
    // Fold the comment into the matter's privileged conversation log. A
    // client's comment flows inbound; a staff comment the client reads in the
    // sidebar flows outbound.
    let direction = match session.role {
        Role::Client => store::communications::direction::INBOUND,
        Role::Staff | Role::Admin => store::communications::direction::OUTBOUND,
    };
    let res = store::document_comments::create_with_communication(
        &db,
        &store::document_comments::NewLinkedComment {
            project_id,
            review_document_id: doc.id,
            person_id,
            direction,
            anchor_start: form.anchor_start,
            anchor_end: form.anchor_end,
            quoted_text: form.quoted_text.trim(),
            body,
        },
    )
    .await;
    if let Err(e) = res {
        tracing::error!(error = %e, "review: create_comment failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    Json(load_comments(&db, doc.id).await).into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}
