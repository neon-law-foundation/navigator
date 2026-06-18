//! `/portal/admin/notations/:id/clauses` — the admin clause editor.
//!
//! Staff add, edit, reorder, and remove the custom paragraphs spliced
//! into a single notation's assembled document (at the template body's
//! `{{custom_clauses}}` marker) before it is sent. Per-matter prose
//! without forking the shared template.
//!
//! Any clause is half of the review gate: a notation carrying custom
//! prose is routed back through `staff_review` before signature (see
//! `web::retainer_walk`), so the bytes the attorney approves are the
//! bytes that get signed.

use axum::extract::{Extension, Form, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use uuid::Uuid;

use sea_orm::EntityTrait;
use store::entity::{notation, template};

use crate::admin::AdminState;
use crate::session::SessionData;

/// The bound template's title, for the page chrome.
async fn flow_label(state: &AdminState, notation_id: Uuid) -> Option<String> {
    let n = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    let t = template::Entity::find_by_id(n.template_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    Some(t.title)
}

fn redirect_to_clauses(notation_id: Uuid) -> Response {
    Redirect::to(&format!("/portal/admin/notations/{notation_id}/clauses")).into_response()
}

/// Query string for [`clauses_page`]: `?format=json` makes it a thin
/// JSON surface the `navigator retainer clause list` CLI consumes (the
/// same `format=json` convention as the notation review route).
#[derive(Debug, Default, Deserialize)]
pub struct ClausesQuery {
    #[serde(default)]
    pub format: String,
}

/// `GET /portal/admin/notations/:id/clauses[?format=json]`.
pub async fn clauses_page(
    State(state): State<AdminState>,
    Path(notation_id): Path<Uuid>,
    Query(q): Query<ClausesQuery>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(label) = flow_label(&state, notation_id).await else {
        return (StatusCode::NOT_FOUND, "notation not found").into_response();
    };
    if q.format == "json" {
        let clauses = store::notation_clauses::for_notation(&state.db, notation_id)
            .await
            .unwrap_or_default();
        let json: Vec<_> = clauses
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "position": c.position,
                    "body": c.body_markdown,
                    "system_authored": c.authored_by_person_id.is_none(),
                })
            })
            .collect();
        return axum::Json(json).into_response();
    }
    let token = session
        .as_deref()
        .map(|s| s.csrf_token.as_str())
        .unwrap_or_default();
    let clauses = store::notation_clauses::for_notation(&state.db, notation_id)
        .await
        .unwrap_or_default();
    let rows: Vec<views::pages::admin::clauses::ClauseRow<'_>> = clauses
        .iter()
        .map(|c| views::pages::admin::clauses::ClauseRow {
            id: c.id,
            position: c.position,
            body: c.body_markdown.as_str(),
        })
        .collect();
    views::pages::admin::clauses::clauses_page(&views::pages::admin::clauses::ClausesPage {
        notation_id,
        flow_label: &label,
        clauses: &rows,
        csrf_token: token,
    })
    .into_response()
}

/// POST body for adding / editing a clause.
#[derive(Debug, Deserialize)]
pub struct ClauseBody {
    pub body: String,
}

/// `POST /portal/admin/notations/:id/clauses` — append one clause.
pub async fn clause_add(
    State(state): State<AdminState>,
    Path(notation_id): Path<Uuid>,
    session: Option<Extension<SessionData>>,
    Form(form): Form<ClauseBody>,
) -> Response {
    let body = form.body.trim();
    if body.is_empty() {
        return redirect_to_clauses(notation_id);
    }
    let author = session.as_deref().and_then(|s| s.person_id);
    if let Err(e) = store::notation_clauses::append(&state.db, notation_id, body, author).await {
        tracing::error!(error = %e, %notation_id, "clauses: append failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    redirect_to_clauses(notation_id)
}

/// `POST /portal/admin/notations/:id/clauses/:cid/edit` — replace a
/// clause's body.
pub async fn clause_edit(
    State(state): State<AdminState>,
    Path((notation_id, clause_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ClauseBody>,
) -> Response {
    let body = form.body.trim();
    if body.is_empty() {
        return redirect_to_clauses(notation_id);
    }
    if let Err(e) = store::notation_clauses::update_body(&state.db, clause_id, body).await {
        tracing::error!(error = %e, %clause_id, "clauses: update failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    redirect_to_clauses(notation_id)
}

/// `POST /portal/admin/notations/:id/clauses/:cid/delete`.
pub async fn clause_delete(
    State(state): State<AdminState>,
    Path((notation_id, clause_id)): Path<(Uuid, Uuid)>,
) -> Response {
    if let Err(e) = store::notation_clauses::delete(&state.db, clause_id).await {
        tracing::error!(error = %e, %clause_id, "clauses: delete failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    redirect_to_clauses(notation_id)
}

/// POST body for reordering a clause.
#[derive(Debug, Deserialize)]
pub struct MoveBody {
    pub direction: String,
}

/// `POST /portal/admin/notations/:id/clauses/:cid/move` — swap a clause
/// with its neighbour (`direction=up|down`).
pub async fn clause_move(
    State(state): State<AdminState>,
    Path((notation_id, clause_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<MoveBody>,
) -> Response {
    let up = form.direction == "up";
    if let Err(e) = store::notation_clauses::move_clause(&state.db, clause_id, up).await {
        tracing::error!(error = %e, %clause_id, "clauses: move failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    redirect_to_clauses(notation_id)
}
