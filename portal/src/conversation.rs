//! `/app/projects/{project_code}/conversation` — the matter's single privileged
//! conversation log.
//!
//! One project-scoped thread interleaving every channel (document comments,
//! inbound/outbound email, portal messages) in time. Row-scoped to the matter
//! exactly like the rest of `/app/projects/*`: a non-participant gets `404`,
//! never `403`. The firm reads the whole thread; a client reads everything except
//! firm-internal notes — the *handler* picks the query
//! ([`store::communications::for_project`] vs `for_project_client_visible`),
//! so a client can never read an internal note even if the template slipped.
//!
//! One route: `POST …/conversation/messages` — post one portal message (lawyer
//! may flag it internal), then redirect back to the thread. The thread itself
//! renders through Dioxus ([`webapp::conversation`]); this module owns only the
//! write.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use uuid::Uuid;

use crate::session::SessionData;
use store::access::{can_see_project, ProjectLens};

/// Posted by the composer.
#[derive(Debug, Deserialize)]
pub struct MessageForm {
    /// CSRF token — verified by the middleware; accepted so the body parses.
    #[serde(rename = "_csrf", default)]
    pub csrf: String,
    pub body: String,
    /// Present (`"1"`) only when a lawyer ticked "internal note".
    #[serde(default)]
    pub internal: Option<String>,
}

/// `POST /app/projects/{project_code}/conversation/messages`.
/// Why posting a matter conversation message failed. Shared by the
/// `/app/projects/{project_code}/conversation/messages` form and the
/// `/app/api/projects/{id}/conversation/messages` door.
#[derive(Debug)]
pub enum PostMessageError {
    /// The caller does not participate in the matter (either lens).
    NotAuthorized,
    /// The message body is empty.
    EmptyBody,
    /// The communication could not be ingested.
    Db(String),
}

impl std::fmt::Display for PostMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "not a participant of this matter"),
            Self::EmptyBody => write!(f, "empty message"),
            Self::Db(e) => write!(f, "database: {e}"),
        }
    }
}

impl std::error::Error for PostMessageError {}

/// Post one message to a matter's conversation. The one command behind both the
/// portal form and the REST door. The tier decides the side: a portal-lens
/// message flows inbound; a lawyer-lens message is outbound, or an internal note
/// when `internal` is set — a portal `internal` flag is ignored, since only the
/// lawyer lens produces an internal note.
pub async fn post_conversation_message(
    surreal: &store::surreal::SurrealDb,
    person_id: Option<Uuid>,
    role: store::persons::Role,
    project_id: Uuid,
    body: &str,
    internal: bool,
) -> Result<(), PostMessageError> {
    if !can_see_project(surreal, person_id, role, project_id)
        .await
        .unwrap_or(false)
    {
        return Err(PostMessageError::NotAuthorized);
    }
    let body = body.trim();
    if body.is_empty() {
        return Err(PostMessageError::EmptyBody);
    }
    let is_lawyer = ProjectLens::for_role(role) == ProjectLens::Lawyer;
    let direction = if !is_lawyer {
        store::communications::direction::INBOUND
    } else if internal {
        store::communications::direction::INTERNAL
    } else {
        store::communications::direction::OUTBOUND
    };
    let args = store::communications::IngestArgs {
        project_id,
        channel: store::communications::channel::PORTAL_MESSAGE,
        direction,
        author_person_id: person_id,
        counterparty: None,
        subject: None,
        body,
        source_ref: None,
        asset_id: None,
        occurred_at: &chrono::Utc::now().to_rfc3339(),
    };
    store::communications::ingest(surreal, &args)
        .await
        .map_err(|e| PostMessageError::Db(e.to_string()))?;
    Ok(())
}

pub async fn post_message(
    State(surreal): State<store::surreal::SurrealDb>,
    Path(project_code): Path<String>,
    session: Option<Extension<SessionData>>,
    axum::Form(form): axum::Form<MessageForm>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let Some(project_id) = store::projects::id_for_code(&surreal, &project_code).await else {
        return not_found();
    };
    match post_conversation_message(
        &surreal,
        session.person_id,
        session.role,
        project_id,
        &form.body,
        form.internal.is_some(),
    )
    .await
    {
        Ok(()) => {
            Redirect::to(&format!("/app/projects/{project_code}/conversation")).into_response()
        }
        Err(PostMessageError::NotAuthorized) => not_found(),
        Err(PostMessageError::EmptyBody) => {
            (StatusCode::BAD_REQUEST, "empty message").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "conversation: post_message failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response()
        }
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        webapp::error_pages::not_found_signed_in(),
    )
        .into_response()
}
