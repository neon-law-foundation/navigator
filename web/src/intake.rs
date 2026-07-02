//! `/portal/projects/:id/intake/:notation_id` — the client self-serve
//! intake surface (the magic link).
//!
//! A client answers (or confirms) the client-facing questions on a
//! notation, one per step, pre-filled with anything staff already entered
//! on their behalf. It is the demand-side mirror of the admin walker
//! (`web::retainer_walk`): same notation, both authorships interleaved.
//!
//! Auth is the same cookie-session + row-scope every other `/portal/*`
//! page uses — no second token scheme. A non-participant gets `404`, never
//! `403`. The client may edit only while the notation is *still in
//! intake*: once it has gone out for signature the answers are frozen, so
//! the page shows the "your part is done" landing instead of a form.
//!
//! Two routes:
//!
//! - `GET …/intake/:notation_id` — the current client step, or the
//!   completion landing.
//! - `POST …/intake/:notation_id` — save one answer (`source = client`)
//!   and advance.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use uuid::Uuid;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use store::entity::{jurisdiction, notation, template};
use store::question_registry::QuestionType;
use store::Db;
use workflows::notation_session::{self, ClientIntakeStep};

use crate::access::can_see_project;
use crate::admin::AdminState;
use crate::session::SessionData;

/// A notation past intake — gone out for signature or finished — no
/// longer takes client edits; its assembled bytes are being signed.
fn is_past_intake(state: &str) -> bool {
    state.starts_with("sent_for_signature") || state == workflows::StateName::END
}

/// Seeded jurisdiction names a question's select offers, per the
/// registry's `jurisdiction_type_filter` (today: `country` questions).
/// Empty for every other `answer_type`, so callers can pass it
/// unconditionally.
pub(crate) async fn jurisdiction_option_names(db: &Db, answer_type: &str) -> Vec<String> {
    let Some(filter) =
        QuestionType::from_token(answer_type).and_then(|t| t.jurisdiction_type_filter())
    else {
        return Vec::new();
    };
    match jurisdiction::Entity::find()
        .filter(jurisdiction::Column::JurisdictionType.eq(filter))
        .order_by_asc(jurisdiction::Column::Name)
        .all(db)
        .await
    {
        Ok(rows) => rows.into_iter().map(|r| r.name).collect(),
        Err(e) => {
            tracing::error!(error = %e, answer_type, "intake: loading jurisdiction options failed");
            Vec::new()
        }
    }
}

/// `Some(error)` when a submitted answer must name a seeded jurisdiction
/// row (per the registry filter) and doesn't — a hand-crafted POST can't
/// smuggle free text past the select.
pub(crate) async fn rejected_reference_answer(
    db: &Db,
    answer_type: &str,
    value: &str,
) -> Option<&'static str> {
    let filter =
        QuestionType::from_token(answer_type).and_then(|t| t.jurisdiction_type_filter())?;
    match jurisdiction::Entity::find()
        .filter(jurisdiction::Column::JurisdictionType.eq(filter))
        .filter(jurisdiction::Column::Name.eq(value))
        .one(db)
        .await
    {
        Ok(Some(_)) => None,
        Ok(None) => Some("Choose a country from the list."),
        Err(e) => {
            // Don't reject a plausible answer on a transient DB error —
            // the write path right after this will surface a real outage.
            tracing::error!(error = %e, answer_type, "intake: option validation lookup failed");
            None
        }
    }
}

/// Resolve `(project_id, notation_id)` to a notation the caller may see,
/// or a `404` response. Enforces, in order: the notation exists, it
/// belongs to *this* project, and the caller may see the project.
async fn visible_notation(
    db: &Db,
    session: &SessionData,
    project_id: Uuid,
    notation_id: Uuid,
) -> Result<notation::Model, Response> {
    let notation = notation::Entity::find_by_id(notation_id)
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
    Ok(notation)
}

/// The bound template's title, for the page chrome. Falls back to a
/// generic label if the template vanished.
async fn flow_label(db: &Db, template_id: Uuid) -> String {
    template::Entity::find_by_id(template_id)
        .one(db)
        .await
        .ok()
        .flatten()
        .map_or_else(|| "intake".to_string(), |t| t.title)
}

/// `GET /portal/projects/:id/intake/:notation_id`.
pub async fn intake_page(
    State(state): State<AdminState>,
    Path((project_id, notation_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let notation = match visible_notation(&state.db, &session, project_id, notation_id).await {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    let label = flow_label(&state.db, notation.template_id).await;

    let step =
        match notation_session::client_intake_step(&state.db, Some(&state.storage), notation_id)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, %notation_id, "intake: client_intake_step failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
            }
        };

    // Frozen once the document has gone out for signature: show the
    // completion landing rather than an editable form.
    if is_past_intake(&notation.state) {
        return views::pages::portal::intake::intake_complete(
            &views::pages::portal::intake::IntakeComplete {
                project_id,
                flow_label: &label,
                total: total_of(&step),
            },
        )
        .into_response();
    }

    match step {
        ClientIntakeStep::NeedsAnswer {
            question,
            prior_value,
            position,
            total,
        } => {
            let country_options = jurisdiction_option_names(&state.db, &question.answer_type).await;
            views::pages::portal::intake::intake_step(&views::pages::portal::intake::IntakeStep {
                project_id,
                notation_id,
                flow_label: &label,
                question_code: question.code.as_str(),
                question_prompt: &question.prompt,
                answer_type: &question.answer_type,
                prior_value: prior_value.as_deref(),
                country_options: &country_options,
                progress: (position, total),
                csrf_token: &session.csrf_token,
                error: None,
            })
            .into_response()
        }
        ClientIntakeStep::Complete { total } => views::pages::portal::intake::intake_complete(
            &views::pages::portal::intake::IntakeComplete {
                project_id,
                flow_label: &label,
                total,
            },
        )
        .into_response(),
    }
}

/// `POST /portal/projects/:id/intake/:notation_id` — save one
/// client-sourced answer and advance. The body is the whole form: one
/// `value` field for scalar questions, or the `people_list` widget's
/// `p{row}_{part}` inputs assembled into a JSON answer.
pub async fn intake_save(
    State(state): State<AdminState>,
    Path((project_id, notation_id)): Path<(Uuid, Uuid)>,
    session: Option<Extension<SessionData>>,
    axum::Form(body): axum::Form<std::collections::BTreeMap<String, String>>,
) -> Response {
    let Some(Extension(session)) = session else {
        return not_found();
    };
    let notation = match visible_notation(&state.db, &session, project_id, notation_id).await {
        Ok(n) => n,
        Err(resp) => return resp,
    };
    // An answer must be attributable to a person; an anonymous session
    // can't author one.
    let Some(person_id) = session.person_id else {
        return not_found();
    };
    let back = format!("/portal/projects/{project_id}/intake/{notation_id}");
    // Frozen: bounce back to GET, which renders the completion landing.
    if is_past_intake(&notation.state) {
        return Redirect::to(&back).into_response();
    }

    // Re-derive which question the client is on so a stale or hand-crafted
    // POST can't write an answer out of order.
    let step =
        match notation_session::client_intake_step(&state.db, Some(&state.storage), notation_id)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, %notation_id, "intake: client_intake_step failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
            }
        };
    let ClientIntakeStep::NeedsAnswer {
        question,
        prior_value,
        position,
        total,
    } = step
    else {
        // Already done — nothing to save.
        return Redirect::to(&back).into_response();
    };
    let value = if store::question_registry::answer_type_is_aggregate(&question.answer_type) {
        crate::people_list_answer::assemble(&body)
    } else {
        body.get("value").cloned().unwrap_or_default()
    };

    if let Some(error) = rejected_reference_answer(&state.db, &question.answer_type, &value).await {
        let label = flow_label(&state.db, notation.template_id).await;
        let country_options = jurisdiction_option_names(&state.db, &question.answer_type).await;
        return views::pages::portal::intake::intake_step(
            &views::pages::portal::intake::IntakeStep {
                project_id,
                notation_id,
                flow_label: &label,
                question_code: question.code.as_str(),
                question_prompt: &question.prompt,
                answer_type: &question.answer_type,
                prior_value: prior_value.as_deref(),
                country_options: &country_options,
                progress: (position, total),
                csrf_token: &session.csrf_token,
                error: Some(error),
            },
        )
        .into_response();
    }

    if let Err(e) = notation_session::record_client_answer(
        &state.db,
        Some(&state.storage),
        notation_id,
        question.code.as_str(),
        value.as_str(),
        person_id,
    )
    .await
    {
        tracing::error!(error = %e, %notation_id, "intake: record_client_answer failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }
    Redirect::to(&back).into_response()
}

/// Pull the `total` out of either step variant.
fn total_of(step: &ClientIntakeStep) -> usize {
    match step {
        ClientIntakeStep::NeedsAnswer { total, .. } | ClientIntakeStep::Complete { total } => {
            *total
        }
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        views::not_found_page_with_auth(views::AuthState::Authenticated),
    )
        .into_response()
}
