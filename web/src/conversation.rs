//! `/portal/projects/:id/conversation` — the matter's single privileged
//! conversation log.
//!
//! One project-scoped thread interleaving every channel (document comments,
//! inbound/outbound email, portal messages) in time. Row-scoped to the matter
//! exactly like the rest of `/portal/*`: a non-participant gets `404`, never
//! `403`. The firm reads the whole thread; a client reads everything except
//! firm-internal notes — the *handler* picks the query
//! ([`store::communications::for_project`] vs `for_project_client_visible`),
//! so a client can never read an internal note even if the template slipped.
//!
//! Two routes:
//!
//! - `GET …/conversation` — the thread + a composer.
//! - `POST …/conversation/messages` — post one portal message (staff may flag
//!   it internal). Returns the refreshed thread fragment for an HTMX request,
//!   or redirects back for a plain form post.

use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use uuid::Uuid;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::entity::person::Role;
use store::entity::{communication, person, project};
use store::Db;
use views::pages::portal::conversation as view;

use crate::access::can_see_project;
use crate::session::SessionData;

/// `GET /portal/projects/:id/conversation`.
pub async fn thread_page(
    State(db): State<Db>,
    Path(project_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    if !can_see_project(&db, session.person_id, session.role, project_id)
        .await
        .unwrap_or(false)
    {
        return not_found();
    }
    let Ok(Some(project)) = project::Entity::find_by_id(project_id).one(&db).await else {
        return not_found();
    };

    let rows = load_messages(&db, project_id, session.role).await;
    let authors = resolve_authors(&db, &rows).await;
    let message_rows = to_message_rows(&rows, &authors);
    view::render(&view::Thread {
        project_id,
        project_name: &project.name,
        messages: &message_rows,
        is_staff: session.role != Role::Client,
        csrf_token: &session.csrf_token,
    })
    .into_response()
}

/// Posted by the composer.
#[derive(Debug, Deserialize)]
pub struct MessageForm {
    /// CSRF token — verified by the middleware; accepted so the body parses.
    #[serde(rename = "_csrf", default)]
    pub csrf: String,
    pub body: String,
    /// Present (`"1"`) only when staff ticked "internal note".
    #[serde(default)]
    pub internal: Option<String>,
}

/// `POST /portal/projects/:id/conversation/messages`.
pub async fn post_message(
    State(db): State<Db>,
    Path(project_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
    headers: HeaderMap,
    axum::Form(form): axum::Form<MessageForm>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    if !can_see_project(&db, session.person_id, session.role, project_id)
        .await
        .unwrap_or(false)
    {
        return not_found();
    }
    let body = form.body.trim();
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty message").into_response();
    }
    // A client's message flows inbound; a staff message is outbound unless it
    // is flagged as an internal note. Only staff may post an internal note —
    // a client's `internal` flag is ignored.
    let is_staff = session.role != Role::Client;
    let direction = if !is_staff {
        store::communications::direction::INBOUND
    } else if form.internal.is_some() {
        store::communications::direction::INTERNAL
    } else {
        store::communications::direction::OUTBOUND
    };

    let res = store::communications::ingest(
        &db,
        &store::communications::IngestArgs {
            project_id,
            channel: store::communications::channel::PORTAL_MESSAGE,
            direction,
            author_person_id: session.person_id,
            counterparty: None,
            subject: None,
            body,
            source_ref: None,
            blob_id: None,
            occurred_at: &chrono::Utc::now().to_rfc3339(),
        },
    )
    .await;
    if let Err(e) = res {
        tracing::error!(error = %e, "conversation: post_message failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }

    // HTMX swaps the refreshed thread fragment in place; a plain form post
    // redirects back to the full page.
    if headers.contains_key("HX-Request") {
        let project_name = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_default();
        let rows = load_messages(&db, project_id, session.role).await;
        let authors = resolve_authors(&db, &rows).await;
        let message_rows = to_message_rows(&rows, &authors);
        return view::render_fragment(&view::Thread {
            project_id,
            project_name: &project_name,
            messages: &message_rows,
            is_staff,
            csrf_token: &session.csrf_token,
        })
        .into_response();
    }
    Redirect::to(&format!("/portal/projects/{project_id}/conversation")).into_response()
}

/// Load the thread the caller is allowed to see: the firm gets every row, a
/// client gets every row except firm-internal notes. The privilege boundary
/// lives here, in the query selection — not in the template.
async fn load_messages(db: &Db, project_id: Uuid, role: Role) -> Vec<communication::Model> {
    let result = if role == Role::Client {
        store::communications::for_project_client_visible(db, project_id).await
    } else {
        store::communications::for_project(db, project_id).await
    };
    result.unwrap_or_default()
}

/// Batch-resolve author display names (no N+1).
async fn resolve_authors(db: &Db, rows: &[communication::Model]) -> HashMap<Uuid, String> {
    let ids: Vec<Uuid> = rows.iter().filter_map(|c| c.author_person_id).collect();
    if ids.is_empty() {
        return HashMap::new();
    }
    person::Entity::find()
        .filter(person::Column::Id.is_in(ids))
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect()
}

fn to_message_rows<'a>(
    rows: &'a [communication::Model],
    authors: &'a HashMap<Uuid, String>,
) -> Vec<view::MessageRow<'a>> {
    rows.iter()
        .map(|c| {
            let author = c
                .author_person_id
                .and_then(|id| authors.get(&id).map(String::as_str))
                .or(c.counterparty.as_deref())
                .unwrap_or("Firm");
            view::MessageRow {
                channel: &c.channel,
                direction: &c.direction,
                author,
                subject: c.subject.as_deref(),
                body: &c.body,
                occurred_at: &c.occurred_at,
            }
        })
        .collect()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}
