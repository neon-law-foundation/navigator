//! Stepwise retainer flow: create a Notation, walk the
//! questionnaire one question per request, then hand off to the
//! post-intake workflow.
//!
//! Routes:
//!
//! - `GET /portal/admin/retainers/new` — the small "start a walk" form
//!   (template code + client email).
//! - `POST /portal/admin/retainers/new` — find-or-insert the Person,
//!   insert Project + role + Notation in one txn, redirect to
//!   `/portal/admin/notations/:id/step`.
//! - `GET /portal/admin/notations/:id/step` — render the current
//!   question (read from the journal + spec) or redirect when the
//!   questionnaire reaches END.
//! - `POST /portal/admin/notations/:id/step` — write the respondent's
//!   answer (`answers` row + journal entry), signal the runtime,
//!   and either redirect for the next question or — on END —
//!   drive the post-intake workflow.

use std::collections::BTreeMap;
use uuid::Uuid;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use maud::Markup;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};
use serde::Deserialize;

use crate::admin::AdminState;
use crate::session::SessionData;
use store::entity::{answer, notation, person, person_project_role, project, question, template};
use workflows::{
    notation_session, MachineKind, NextStep, NotationSessionError, StateMachineRuntime, StateName,
};

fn csrf_token(session: Option<&SessionData>) -> &str {
    session.map_or("", |s| s.csrf_token.as_str())
}

/// POST body for `/portal/admin/retainers/new`. Two fields — everything
/// else the walker collects.
#[derive(Debug, Clone, Deserialize)]
pub struct StartWalkBody {
    pub client_email: String,
    pub retainer_template_code: String,
}

/// The seeded onboarding templates (`onboarding__*`), as `(code, label)`
/// pairs sorted by title, for the matter-open dropdown. Restricting the
/// picker to this family is what makes "opening a matter" always start it
/// with a retainer-type notation.
pub(crate) async fn onboarding_templates(db: &store::Db) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = template::Entity::find()
        .filter(template::Column::ProjectId.is_null())
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.code.starts_with("onboarding__"))
        .map(|t| {
            let label = format!("{} — {}", t.title, t.code);
            (t.code, label)
        })
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    rows
}

/// GET `/portal/admin/retainers/new` — the create form.
pub async fn start_get(
    State(state): State<AdminState>,
    session: Option<Extension<SessionData>>,
) -> Markup {
    let token = csrf_token(session.as_deref());
    let templates = onboarding_templates(&state.db).await;
    views::pages::admin::retainers::start_walk(&views::pages::admin::retainers::StartWalk {
        templates: &templates,
        csrf_token: token,
        ..Default::default()
    })
}

/// POST `/portal/admin/retainers/new` — create the four rows the
/// retainer lifecycle needs, then redirect to the walker.
#[allow(clippy::too_many_lines)]
pub async fn start_post(
    State(state): State<AdminState>,
    session: Option<Extension<SessionData>>,
    Form(body): Form<StartWalkBody>,
) -> Response {
    let token = csrf_token(session.as_deref());
    let client_email = body.client_email.trim();
    let code = body.retainer_template_code.trim();
    let templates = onboarding_templates(&state.db).await;

    if !client_email.contains('@') {
        // Catalog-sourced validation error; held so the borrow into
        // `StartWalk` outlives the render.
        let email_error = views::i18n::t(views::Locale::En, "portal.retainer_client_email_at");
        return views::pages::admin::retainers::start_walk(
            &views::pages::admin::retainers::StartWalk {
                client_email: &body.client_email,
                retainer_template_code: &body.retainer_template_code,
                templates: &templates,
                csrf_token: token,
                error: Some(&email_error),
            },
        )
        .into_response();
    }
    if code.is_empty() {
        return views::pages::admin::retainers::start_walk(
            &views::pages::admin::retainers::StartWalk {
                client_email: &body.client_email,
                retainer_template_code: &body.retainer_template_code,
                templates: &templates,
                csrf_token: token,
                error: Some("choose an onboarding template"),
            },
        )
        .into_response();
    }

    let txn = match state.db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "start_post: txn begin failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    let template_row = match template::Entity::find()
        .filter(template::Column::Code.eq(code))
        .one(&txn)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return views::pages::admin::retainers::start_walk(
                &views::pages::admin::retainers::StartWalk {
                    client_email: &body.client_email,
                    retainer_template_code: &body.retainer_template_code,
                    templates: &templates,
                    csrf_token: token,
                    error: Some("that onboarding template was not found"),
                },
            )
            .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "start_post: template lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // `projects.entity_id` is NOT NULL, but a self-serve intake has no
    // staffer to designate a pre-existing entity. Open the matter against
    // a fresh `Human` entity for this natural person, created in the same
    // transaction.
    let entity_id = match create_human_entity(&txn, client_email).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "start_post: human entity create failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Both DRI columns are NOT NULL. A self-serve intake has no staffer in
    // the room, so the staff DRI falls back to the seeded firm principal
    // (`nick@neonlaw.com`) — a real person, no sentinel. The client side is
    // refined to the self-serve client below, once `link_retainer_rows`
    // creates them; until then the firm principal holds both sides.
    let staff_dri_id = if let Some(id) = session.as_deref().and_then(|s| s.person_id) {
        id
    } else if let Ok(Some(id)) = store::persons::default_firm_dri(&txn).await {
        id
    } else {
        tracing::error!("start_post: no staff DRI resolvable (unseeded db?)");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    };

    // The matter the walk opens is brand-new, so the project name is a
    // placeholder until the `project_name` question lands.
    let project_id = match (project::ActiveModel {
        name: ActiveValue::Set(format!("(pending) {client_email}")),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff_dri_id)),
        client_dri_person_id: ActiveValue::Set(Some(staff_dri_id)),
        ..Default::default()
    })
    .insert(&txn)
    .await
    {
        Ok(p) => p.id,
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "start_post: project uniqueness conflict");
            return (StatusCode::CONFLICT, "That project key already exists.").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "start_post: project insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Find-or-create the client, attach the `client` role, and create the
    // retainer Notation — the shared "hang a retainer on a matter" helper
    // the matter-open form (`crate::admin`) also calls. The walk collects
    // the client name later in the questionnaire and the client signs
    // *embedded* (the historical default), so name is `None` and delivery
    // is `embedded`.
    let rows = match link_retainer_rows(
        &txn,
        template_row.id,
        project_id,
        client_email,
        None,
        store::entity::notation::DELIVERY_EMBEDDED,
    )
    .await
    {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let notation_id = rows.notation_id;

    // Refine the authoritative client-DRI column to the self-serve client
    // `link_retainer_rows` just created. The column is the source of truth;
    // the `client` participation row it also wrote stays for the ledger.
    let mut client_dri_update = project::ActiveModel {
        id: ActiveValue::Unchanged(project_id),
        ..Default::default()
    };
    client_dri_update.client_dri_person_id = ActiveValue::Set(Some(rows.person_id));
    if let Err(e) = client_dri_update.update(&txn).await {
        tracing::error!(error = %e, "start_post: client DRI column update failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }

    // Disclose the staffer who opened this matter as its staff DRI — a
    // `person_project_roles` row — so the project-scoped matter page is
    // visible to them. The estate flow lands staff on that page directly
    // (below), and `can_see_project` 404s a staff member who isn't on the
    // matter; without this the opener can't see the matter they just
    // created. Mirrors the matter-open form
    // (`admin::projects_create_staff_only`). A session with no linked
    // Person (the dev bypass) opens the matter without a staff DRI.
    if let Some(staff_dri) = session.as_deref().and_then(|s| s.person_id) {
        match (person_project_role::ActiveModel {
            person_id: ActiveValue::Set(staff_dri),
            project_id: ActiveValue::Set(project_id),
            participation: ActiveValue::Set(
                person_project_role::PARTICIPATION_STAFF_DRI.to_string(),
            ),
            ..Default::default()
        })
        .insert(&txn)
        .await
        {
            Ok(_) => {}
            Err(e) if store::is_unique_violation(&e) => {
                tracing::warn!(error = %e, "start_post: staff DRI already disclosed");
            }
            Err(e) => {
                tracing::error!(error = %e, "start_post: staff DRI disclosure failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
            }
        }
    }

    if let Err(e) = txn.commit().await {
        tracing::error!(error = %e, "start_post: txn commit failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
    }

    // Transcript-driven onboarding (Northstar estate) has no questionnaire
    // to walk before intake — the recorded sitting's transcript fills the
    // answers via extraction. Detect it by the `transcript_uploaded` edge
    // out of `BEGIN` (data-driven, never a hard-coded template code), start
    // the workflow machine so the transcript-upload surface has a live
    // timeline to signal, and land staff on the matter page where that form
    // lives. Questionnaire-first onboarding (the retainer) keeps the walker
    // redirect below.
    if let Some(spec) = workflows::bundled_spec_yaml(code)
        .and_then(|yaml| workflows::workflow_spec_from_yaml(yaml).ok())
    {
        let transcript_driven = spec.transitions_from(&StateName::begin()).is_some_and(|t| {
            t.lookup(crate::transcript_intake::TRANSCRIPT_UPLOADED)
                .is_some()
        });
        if transcript_driven {
            if let Err(e) = StateMachineRuntime::start(
                state.workflow_runtime.as_ref(),
                MachineKind::Workflow,
                notation_id,
                &spec,
            )
            .await
            {
                tracing::error!(error = %e, %notation_id, "start_post: estate workflow start failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
            }
            return Redirect::to(&format!("/portal/projects/{project_id}")).into_response();
        }
    }

    Redirect::to(&format!("/portal/admin/notations/{notation_id}/step")).into_response()
}

/// The client Person + Notation a retainer-type matter hangs off of,
/// created by [`link_retainer_rows`].
pub(crate) struct RetainerRows {
    pub person_id: Uuid,
    pub notation_id: Uuid,
}

/// Create a fresh `Human` entity for a solo natural person, in the
/// caller's transaction, returning its id. Used by the self-serve intake
/// walk, which (unlike the admin matter-open) has no staffer to pick a
/// pre-existing entity. Find-or-creates the `Human` entity type and a
/// jurisdiction so a self-serve intake never fails on missing reference
/// data (the canonical seed normally supplies both; this is the fallback).
async fn create_human_entity(
    txn: &DatabaseTransaction,
    label: &str,
) -> Result<Uuid, sea_orm::DbErr> {
    use store::entity::{entity as entities, entity_type, jurisdiction};

    let type_id = match entity_type::Entity::find()
        .filter(entity_type::Column::Name.eq("Human"))
        .one(txn)
        .await?
    {
        Some(t) => t.id,
        None => {
            entity_type::ActiveModel {
                name: ActiveValue::Set("Human".into()),
                ..Default::default()
            }
            .insert(txn)
            .await?
            .id
        }
    };
    let jurisdiction_id = match jurisdiction::Entity::find().one(txn).await? {
        Some(j) => j.id,
        None => {
            jurisdiction::ActiveModel {
                name: ActiveValue::Set("United States".into()),
                code: ActiveValue::Set("US".into()),
                ..Default::default()
            }
            .insert(txn)
            .await?
            .id
        }
    };
    Ok(entities::ActiveModel {
        name: ActiveValue::Set(label.to_string()),
        entity_type_id: ActiveValue::Set(type_id),
        jurisdiction_id: ActiveValue::Set(jurisdiction_id),
        ..Default::default()
    }
    .insert(txn)
    .await?
    .id)
}

/// In the caller's transaction: find-or-create the client Person by
/// `client_email` (role `client`), attach a `client` participation role
/// to `project_id`, and create the retainer Notation bound to
/// `template_id` at `BEGIN` with the given `delivery`.
///
/// The one code path for "hang a retainer on a matter," shared by the
/// standalone retainer walk ([`start_post`]) and the matter-open form
/// (`crate::admin::projects_create_staff_only`). The caller owns project
/// creation — the walk inserts a pending project, the matter-open form
/// already inserted the real one. A conflict short-circuits to the same
/// status responses the walk historically produced (returned as the
/// `Err` so the caller can `return` it and let the transaction roll
/// back).
pub(crate) async fn link_retainer_rows(
    txn: &DatabaseTransaction,
    template_id: Uuid,
    project_id: Uuid,
    client_email: &str,
    client_name: Option<&str>,
    delivery: &str,
) -> Result<RetainerRows, Response> {
    let person_id = match person::Entity::find()
        .filter(person::Column::Email.eq(client_email))
        .one(txn)
        .await
    {
        Ok(Some(p)) => p.id,
        Ok(None) => {
            // A new client: name from the form when given (the matter-open
            // signer field), else the email as a stand-in (the walk asks
            // for the name later in the questionnaire).
            let name = client_name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(client_email);
            match (person::ActiveModel {
                name: ActiveValue::Set(name.into()),
                email: ActiveValue::Set(client_email.into()),
                role: ActiveValue::Set(store::entity::person::Role::Client),
                ..Default::default()
            })
            .insert(txn)
            .await
            {
                Ok(p) => p.id,
                Err(e) if store::is_unique_violation(&e) => {
                    tracing::warn!(error = %e, "link_retainer_rows: person email conflict");
                    return Err((
                        StatusCode::CONFLICT,
                        "That client email already belongs to another person.",
                    )
                        .into_response());
                }
                Err(e) => {
                    tracing::error!(error = %e, "link_retainer_rows: person insert failed");
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response());
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "link_retainer_rows: person lookup failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response());
        }
    };

    match (person_project_role::ActiveModel {
        person_id: ActiveValue::Set(person_id),
        project_id: ActiveValue::Set(project_id),
        participation: ActiveValue::Set("client".into()),
        ..Default::default()
    })
    .insert(txn)
    .await
    {
        Ok(_) => {}
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "link_retainer_rows: role uniqueness conflict");
            return Err((
                StatusCode::CONFLICT,
                "That person already has this role on the project.",
            )
                .into_response());
        }
        Err(e) => {
            tracing::error!(error = %e, "link_retainer_rows: role insert failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response());
        }
    }

    let notation_id = match (notation::ActiveModel {
        template_id: ActiveValue::Set(template_id),
        person_id: ActiveValue::Set(person_id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set(StateName::BEGIN.into()),
        delivery: ActiveValue::Set(delivery.into()),
        ..Default::default()
    })
    .insert(txn)
    .await
    {
        Ok(n) => n.id,
        Err(e) if store::is_unique_violation(&e) => {
            tracing::warn!(error = %e, "link_retainer_rows: notation uniqueness conflict");
            return Err((StatusCode::CONFLICT, "That notation already exists.").into_response());
        }
        Err(e) => {
            tracing::error!(error = %e, "link_retainer_rows: notation insert failed");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response());
        }
    };

    Ok(RetainerRows {
        person_id,
        notation_id,
    })
}

/// Seed staff-entered answers for `person_id` from `(question_code,
/// value)` pairs, inside the caller's transaction. Mirrors what the
/// per-step walker writes (`source = staff`, `authored_by` the logged-in
/// staffer) so the matter-open form fills the retainer questionnaire in
/// one shot. Unknown codes are skipped; a blank value is still written so
/// the template placeholder renders empty rather than as a literal
/// `{{code}}`.
pub(crate) async fn seed_staff_answers(
    txn: &DatabaseTransaction,
    person_id: Uuid,
    authored_by: Option<Uuid>,
    answers: &[(&str, &str)],
) -> Result<(), sea_orm::DbErr> {
    for (code, value) in answers {
        let Some(q) = question::Entity::find()
            .filter(question::Column::Code.eq(*code))
            .one(txn)
            .await?
        else {
            continue;
        };
        answer::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            person_id: ActiveValue::Set(person_id),
            value: ActiveValue::Set((*value).to_string()),
            source: ActiveValue::Set(store::entity::answer::SOURCE_STAFF.to_string()),
            authored_by_person_id: ActiveValue::Set(authored_by),
            ..Default::default()
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

/// Template code for the firm-signed matter-close letter.
const CLOSING_TEMPLATE_CODE: &str = "closing__letter";

/// POST `/portal/admin/projects/:id/close` — open the closing-letter
/// walk for an existing matter.
///
/// The mirror of [`start_post`]: where the retainer *opens* a matter
/// (creating Person + Project + role + Notation), the close acts on a
/// matter that already exists, so it creates only the `closing__letter`
/// Notation — bound to the project and addressed to the project's
/// client — then redirects into the generic walker. The status flip to
/// `closed` is the worker's job when the close workflow completes (see
/// `workflows-service`), not this handler's; this only starts the walk.
pub async fn close_matter_post(
    State(state): State<AdminState>,
    AxumPath(project_id): AxumPath<Uuid>,
) -> Response {
    let template_row = match template::Entity::find()
        .filter(template::Column::Code.eq(CLOSING_TEMPLATE_CODE))
        .one(&state.db)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            tracing::error!("close_matter_post: closing__letter template not seeded");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "closing template missing",
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "close_matter_post: template lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    let project_row = match project::Entity::find_by_id(project_id).one(&state.db).await {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::NOT_FOUND, "matter not found").into_response(),
        Err(e) => {
            tracing::error!(error = %e, %project_id, "close_matter_post: project lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // The matter's client is the closing letter's respondent.
    let client_role = match person_project_role::Entity::find()
        .filter(person_project_role::Column::ProjectId.eq(project_id))
        .filter(person_project_role::Column::Participation.eq("client"))
        .one(&state.db)
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::CONFLICT,
                "this matter has no client to address the closing letter to",
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, %project_id, "close_matter_post: client lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    let notation_id = match (notation::ActiveModel {
        template_id: ActiveValue::Set(template_row.id),
        person_id: ActiveValue::Set(client_role.person_id),
        entity_id: ActiveValue::Set(Some(project_row.entity_id)),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set(StateName::BEGIN.into()),
        ..Default::default()
    })
    .insert(&state.db)
    .await
    {
        Ok(n) => n.id,
        Err(e) => {
            tracing::error!(error = %e, %project_id, "close_matter_post: notation insert failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    Redirect::to(&format!("/portal/admin/notations/{notation_id}/step")).into_response()
}

/// POST `/portal/admin/notations/:id/send-intake` — hand the matter's
/// client their self-serve intake link.
///
/// The client signs into the portal and answers the client-facing
/// questions on this notation themselves (see [`crate::intake`]) — the
/// demand-side mirror of this admin walker. There is no second token
/// scheme: the link is gated by the same cookie-session + project ACL as
/// every other `/portal/*` page, so this handler just ensures the client
/// carries a participation row for the matter (idempotent) and emails the
/// URL.
pub async fn send_intake_post(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
) -> Response {
    let notation_row = match notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return (StatusCode::NOT_FOUND, "notation not found").into_response(),
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "send_intake: notation lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };
    let client = match person::Entity::find_by_id(notation_row.person_id)
        .one(&state.db)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => return (StatusCode::CONFLICT, "notation has no client").into_response(),
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "send_intake: client lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Ensure the client can see the matter (they already should from
    // matter-open; this is the find-or-create that backs the magic link).
    let participation = person_project_role::Entity::find()
        .filter(person_project_role::Column::PersonId.eq(client.id))
        .filter(person_project_role::Column::ProjectId.eq(notation_row.project_id))
        .one(&state.db)
        .await;
    if let Ok(None) = participation {
        if let Err(e) = (person_project_role::ActiveModel {
            person_id: ActiveValue::Set(client.id),
            project_id: ActiveValue::Set(notation_row.project_id),
            participation: ActiveValue::Set("client".into()),
            ..Default::default()
        })
        .insert(&state.db)
        .await
        {
            if !store::is_unique_violation(&e) {
                tracing::error!(error = %e, %notation_id, "send_intake: participation upsert failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
            }
        }
    }

    let base_url = workflows::email::base_url_from_env();
    let link = format!(
        "{base_url}/portal/projects/{}/intake/{notation_id}",
        notation_row.project_id
    );
    let body = format!(
        "Your legal team has started your paperwork and needs you to confirm a few \
         details. Open your secure intake here and finish your part:\n\n{link}\n\n\
         Your answers save as you go, so you can stop and pick up where you left off. \
         Nothing is sent for signature until an attorney has reviewed it."
    );
    let html = workflows::email::render_email_html(
        &body,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    let msg = crate::email::OutboundEmail::new(
        client.email.clone(),
        "Finish your Neon Law Navigator intake",
        body,
    )
    .with_html(html)
    .with_person(client.id.to_string());
    if let Err(e) = state.email.send(msg).await {
        tracing::warn!(error = %e, %notation_id, recipient = %client.email, "send_intake: email send failed");
    } else {
        tracing::info!(%notation_id, recipient = %client.email, "send_intake: intake link sent");
    }

    Redirect::to(&format!("/portal/admin/notations/{notation_id}/step")).into_response()
}

/// Query string for `GET /portal/admin/notations/:id/review`. The lone
/// `format=json` knob flips the review screen to the machine-readable
/// status body the `navigator notation status` CLI command consumes.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviewQuery {
    #[serde(default)]
    pub format: Option<String>,
}

/// Query string for `GET /portal/admin/notations/:id/step`. `format=json`
/// flips the walker to the machine-readable step body the `navigator
/// intake answer` CLI command consumes — the current question's code,
/// prompt, `answer_type`, and `radio` choices — so the CLI drives the
/// questionnaire over the same route the browser walks without scraping
/// HTML. Mirrors [`ReviewQuery`] on the review screen.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StepQuery {
    #[serde(default)]
    pub format: Option<String>,
}

/// GET `/portal/admin/notations/:id/step` — render the current question
/// or redirect once the questionnaire reaches END. With `?format=json`,
/// answer the question metadata as JSON for the CLI walker instead.
pub async fn step_get(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    axum::extract::Query(q): axum::extract::Query<StepQuery>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let token = session
        .as_ref()
        .map(|s| s.csrf_token.as_str())
        .unwrap_or_default();

    // The runtime — not the Postgres journal — is the source of
    // truth for state; the worker writes `notation_events` rows
    // via `ctx.run` as a projection (see `docs/glossary.md` →
    // `ctx.run`). `notation_session::current_step` reads from the
    // runtime and resolves the question row in one call.
    let step = notation_session::current_step(
        &state.db,
        state.questionnaire_runtime.as_ref(),
        Some(&state.storage),
        notation_id,
    )
    .await;

    // The CLI walker reads the step as JSON: which question is next (with
    // its `radio` choices from the canonical seed), or that the
    // questionnaire is complete. HTML scraping is brittle, so the machine
    // surface is a narrow branch on this same handler, like `review_get`.
    if q.format.as_deref() == Some("json") {
        return step_json(notation_id, step);
    }

    let question = match step {
        Ok(NextStep::NeedsAnswer { question }) => question,
        Ok(NextStep::QuestionnaireComplete) => {
            return Redirect::to("/portal/admin").into_response()
        }
        Err(NotationSessionError::NotationNotFound(_)) => {
            return (StatusCode::NOT_FOUND, "notation not found").into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "walker: current_step failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    // Resolve the notation's bound template once. The walker is
    // generic over any notation, so both the prior-answer lookup
    // (needs person_id) and the chrome (title + progress total) must
    // follow the actual template — not assume the retainer.
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
        .ok()
        .flatten();
    let person_id = notation_row
        .as_ref()
        .map_or_else(Uuid::nil, |n| n.person_id);
    let template_row = match notation_row.as_ref() {
        Some(n) => template::Entity::find_by_id(n.template_id)
            .one(&state.db)
            .await
            .ok()
            .flatten(),
        None => None,
    };

    // Pre-fill any prior answer for this (question, person) pair
    // so navigating back re-displays without mutating durable
    // state.
    let prior_answer = answer::Entity::find()
        .filter(answer::Column::QuestionId.eq(question.id))
        .filter(answer::Column::PersonId.eq(person_id))
        .order_by_desc(answer::Column::Id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|a| a.value);

    // Progress = (1-based index of the question being asked, total
    // question states), computed from the bound template's
    // questionnaire when it's a bundled spec. Falls back to the
    // retainer spec held in AppState if the template isn't resolvable,
    // so the shipped retainer flow is unchanged.
    let template_spec = template_row
        .as_ref()
        .and_then(|t| workflows::bundled_spec_yaml(&t.code))
        .and_then(|yaml| workflows::questionnaire_spec_from_yaml(yaml).ok());
    let spec = template_spec
        .as_ref()
        .unwrap_or(&state.retainer_intake_questionnaire);
    // The chrome names the actual template (e.g. "Retainer Agreement",
    // "Closing Letter") rather than hard-coding the retainer.
    let flow_label = template_row
        .as_ref()
        .map_or("Retainer intake", |t| t.title.as_str());
    let current_state = StateMachineRuntime::current_state(
        state.questionnaire_runtime.as_ref(),
        MachineKind::Questionnaire,
        notation_id,
    )
    .await
    .unwrap_or_else(StateName::begin);
    let progress = progress_for(spec, &current_state);
    tracing::info!(
        %notation_id,
        rendered_question = %question.code,
        current_state = %current_state.as_str(),
        progress_current = progress.0,
        progress_total = progress.1,
        "step_get: rendering question",
    );

    views::pages::admin::retainers::question_step(&views::pages::admin::retainers::QuestionStep {
        notation_id,
        flow_label,
        question_code: question.code.as_str(),
        question_prompt: &question.prompt,
        answer_type: &question.answer_type,
        prior_answer: prior_answer.as_deref(),
        progress,
        csrf_token: token,
        error: None,
    })
    .into_response()
}

/// Render the current questionnaire step as the JSON body the `navigator
/// intake answer` CLI command walks: either the next question (code,
/// prompt, `answer_type`, and any `radio` choices) or `complete: true`
/// once the machine reaches END. A `people_list` question's `choices` is
/// empty — the CLI assembles its `p{row}_{part}` rows from `--person`
/// flags / interactive prompts, not from a fixed list.
fn step_json(notation_id: Uuid, step: Result<NextStep, NotationSessionError>) -> Response {
    match step {
        Ok(NextStep::NeedsAnswer { question }) => {
            let choices: Vec<serde_json::Value> = store::seed::question_choices(&question.code)
                .into_iter()
                .map(|(value, label)| serde_json::json!({ "value": value, "label": label }))
                .collect();
            axum::Json(serde_json::json!({
                "notation_id": notation_id,
                "complete": false,
                "question": {
                    "code": question.code,
                    "prompt": question.prompt,
                    "answer_type": question.answer_type,
                    "choices": choices,
                },
            }))
            .into_response()
        }
        Ok(NextStep::QuestionnaireComplete) => axum::Json(serde_json::json!({
            "notation_id": notation_id,
            "complete": true,
            "question": serde_json::Value::Null,
        }))
        .into_response(),
        Err(NotationSessionError::NotationNotFound(_)) => {
            (StatusCode::NOT_FOUND, "notation not found").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "walker: current_step (json) failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response()
        }
    }
}

/// POST `/portal/admin/notations/:id/step` — capture one answer and
/// advance the questionnaire.
///
/// The runtime is the sole writer of `notation_events`: in
/// production, the `workflows-service` worker handler journals each
/// transition inside `ctx.run("append-…", …)` (see
/// `workflows-service::notation_service::questionnaire_signal`); in
/// tests, the in-memory runtime records the transition in its own
/// `Vec<WorkflowEvent>`. The shared `notation_session` service
/// does *not* write the journal itself, so a production deploy
/// sees exactly one row per signal and replays don't
/// double-insert.
/// Resolve the question the runtime currently expects an answer for,
/// mapping every non-answerable state to its HTTP response.
async fn expected_question(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<notation_session::QuestionDescriptor, Response> {
    match notation_session::current_step(
        &state.db,
        state.questionnaire_runtime.as_ref(),
        Some(&state.storage),
        notation_id,
    )
    .await
    {
        Ok(NextStep::NeedsAnswer { question }) => {
            tracing::info!(%notation_id, code = %question.code, "step_post: current_step → NeedsAnswer");
            Ok(question)
        }
        Ok(NextStep::QuestionnaireComplete) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "questionnaire is already complete",
        )
            .into_response()),
        Err(NotationSessionError::NotationNotFound(_)) => {
            Err((StatusCode::NOT_FOUND, "notation not found").into_response())
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "walker: current_step failed");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response())
        }
    }
}

pub async fn step_post(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
    Form(body): Form<std::collections::BTreeMap<String, String>>,
) -> Response {
    tracing::info!(%notation_id, field_count = body.len(), "step_post: enter");
    // The admin walker is staff entering the answer on the client's
    // behalf: the typist is the logged-in staff/admin person, the source
    // is `staff`. The respondent stays the notation's bound client.
    let author =
        notation_session::AnswerAuthor::staff(session.as_deref().and_then(|s| s.person_id));
    // The HTML form submits `value` (or the `people_list` widget's
    // `p{row}_{part}` inputs); ask the service which question the
    // runtime is currently expecting so we can pass the right code —
    // and assemble the right value shape — into `answer_step`.
    let question = match expected_question(&state, notation_id).await {
        Ok(question) => question,
        Err(resp) => return resp,
    };
    let value = if question.answer_type == "people_list" {
        crate::people_list_answer::assemble(&body)
    } else {
        body.get("value").cloned().unwrap_or_default()
    };

    let next = match notation_session::answer_step(
        &state.db,
        state.questionnaire_runtime.as_ref(),
        Some(&state.storage),
        notation_id,
        &question.code,
        value.as_str(),
        author,
    )
    .await
    {
        Ok(n) => n,
        Err(NotationSessionError::AlreadyComplete) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                "questionnaire is already complete",
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "walker: answer_step failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal").into_response();
        }
    };

    match next {
        NextStep::NeedsAnswer { .. } => {
            // Round-trip back to GET so the user sees the next
            // question.
            Redirect::to(&format!("/portal/admin/notations/{notation_id}/step")).into_response()
        }
        NextStep::QuestionnaireComplete => {
            // The closing letter is firm-signed and ends the matter, so
            // it drives a different post-questionnaire workflow than the
            // client-signed retainer. Branch on the bound template.
            if notation_template_code(&state.db, notation_id)
                .await
                .as_deref()
                == Some("closing__letter")
            {
                return match drive_closing_workflow(&state, notation_id).await {
                    // The matter is now closed and the letter firm-signed;
                    // raise the flat matter-close fee, then back to the
                    // admin surface. The fee is best-effort: a billing-seam
                    // failure must not leave the matter wedged open.
                    Ok(_end) => {
                        if let Err(e) = raise_matter_close_fee(&state, notation_id).await {
                            tracing::error!(error = %e, %notation_id, "matter-close fee failed (matter is closed)");
                        }
                        Redirect::to("/portal/admin").into_response()
                    }
                    Err(e) => {
                        tracing::error!(error = %e, %notation_id, "walker: closing drive failed");
                        (StatusCode::INTERNAL_SERVER_ERROR, "closing failed").into_response()
                    }
                };
            }
            // Hand off to the post-intake workflow: intake →
            // retainer_rendered → sent_for_signature. The
            // rendering context comes from the Answer rows the
            // walker just landed, so the workflow drive is
            // self-contained.
            match drive_post_questionnaire_workflow(&state, notation_id).await {
                Ok(out) => views::pages::admin::retainers::result(
                    &views::pages::admin::retainers::IntakeResult {
                        notation_id,
                        workflow_state: out.final_state.as_str(),
                        signature_request_id: out
                            .signature_request_id
                            .as_ref()
                            .map(|id| id.0.as_str()),
                        rendered: out.rendered,
                        csrf_token: csrf_token(session.as_deref()),
                    },
                )
                .into_response(),
                Err(e) => {
                    tracing::error!(error = %e, %notation_id, "walker: workflow drive failed");
                    (StatusCode::INTERNAL_SERVER_ERROR, "workflow failed").into_response()
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WorkflowDriveError {
    #[error("workflow runtime: {0}")]
    Runtime(#[from] workflows::WorkflowRuntimeError),
    #[error("signature provider: {0}")]
    Signature(#[from] crate::signature::SignatureError),
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("template `{0}` vanished mid-flight")]
    TemplateMissing(Uuid),
    #[error("serialize document payload: {0}")]
    Payload(serde_json::Error),
    #[error("closing workflow spec: {0}")]
    Spec(String),
    #[error("storage: {0}")]
    Storage(#[from] cloud::StorageError),
    #[error("template body: {0}")]
    TemplateBody(#[from] store::templates::TemplateBodyError),
    /// The worker has not yet rendered + persisted the notation's PDF, so
    /// there is nothing to send. Distinguished from a hard failure: the
    /// send route maps this to `409` + a "retry" reason, never a 500.
    #[error("document not ready: the retainer PDF for notation {0} has not been rendered yet")]
    DocumentNotReady(Uuid),
    /// The template's `form:` binding or its field map failed to
    /// resolve — a vendoring or mapping defect, never silently skipped.
    #[error("government form `{form_code}`: {reason}")]
    Form { form_code: String, reason: String },
}

struct WorkflowOutput {
    final_state: StateName,
    /// `Some` once the assembled document is sent for signature; `None`
    /// when the notation parked at `staff_review` awaiting attorney
    /// approval because it carries custom content (a custom clause or a
    /// client-entered answer).
    signature_request_id: Option<crate::signature::SignatureRequestId>,
    rendered: Markup,
}

/// Storage-key convention for the rendered document PDF of a given
/// notation — the version sent out for signature. Per-notation and
/// template-agnostic (the retainer, the trust, and any future signed
/// template share this scheme); lives in one place so the
/// document-download route stays in sync.
#[must_use]
pub fn document_pdf_storage_key(notation_id: Uuid) -> String {
    format!("notations/{notation_id}/document.pdf")
}

/// Storage key for the executed (signed) document PDF — the version the
/// provider returns once every party has signed.
#[must_use]
pub fn signed_document_storage_key(notation_id: Uuid) -> String {
    format!("notations/{notation_id}/signed-document.pdf")
}

/// Storage key for the Certificate of Completion — the ESIGN
/// evidentiary record archived alongside the signed retainer.
#[must_use]
pub fn certificate_of_completion_storage_key(notation_id: Uuid) -> String {
    format!("notations/{notation_id}/certificate-of-completion.pdf")
}

/// Run the post-questionnaire signing workflow against an
/// already-walked Notation. Template-agnostic — the retainer, the
/// Nevada trust, and any future signed template walk the same path:
///
///   intake_submitted  → intake_persisted__<respondent>
///   <doc>_rendered     → staff_review
///   approved           → document_open__<doc>_pdf
///   pdf_persisted      → sent_for_signature__pending
///
/// The workflow spec is resolved from the notation's bound template
/// **code** (not a cached retainer spec), and the only per-template
/// condition — the `*_rendered` edge out of `intake_persisted__*` — is
/// read straight from that spec, so adding a signed template needs no
/// code change here. The rendering context is built from the Answer
/// rows the walker just persisted; the PDF lands in
/// `cloud::StorageService` keyed by [`document_pdf_storage_key`].
/// Start the workflow machine for `notation_id` and advance it from
/// `BEGIN` to the `staff_review` gate: fire `intake_submitted`, then the
/// template's single `*_rendered` edge out of `intake_persisted__*`,
/// syncing `notation.state` at each step. Returns the resulting
/// `staff_review` state.
///
/// It never sends — the human approve step renders + parks
/// ([`approve_send_post`] → [`render_and_park`]) and the deliberate send
/// ([`send_post`] → [`dispatch_signature`]) is the only thing that emits an
/// envelope. The `*_rendered` condition is read from the spec, not
/// hard-coded, so a new signed template needs no change here. Shared by the
/// questionnaire walker's post-intake drive and the matter-open form, so
/// both reach the gate by one code path.
pub(crate) async fn advance_to_staff_review(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<StateName, WorkflowDriveError> {
    let runtime = state.workflow_runtime.as_ref();

    // The send path is keyed off the template code, so the spec follows
    // the actual bound template.
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let yaml = workflows::bundled_spec_yaml(&template_row.code)
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let spec = workflows::workflow_spec_from_yaml(yaml)
        .map_err(|e| WorkflowDriveError::Spec(e.to_string()))?;

    StateMachineRuntime::start(runtime, MachineKind::Workflow, notation_id, &spec).await?;
    let intake_state = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "intake_submitted",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, intake_state.as_str()).await?;

    // The condition that advances the persisted intake into staff review
    // names the rendered document (`retainer_rendered`, `trust_rendered`,
    // …). It is the single edge out of the `intake_persisted__*` state, so
    // read it from the spec rather than hard-coding the retainer's name.
    let rendered_condition = spec
        .transitions_from(&intake_state)
        .and_then(|t| t.conditions().next())
        .map(ToString::to_string)
        .ok_or_else(|| {
            WorkflowDriveError::Spec(format!(
                "no rendered transition out of `{}`",
                intake_state.as_str()
            ))
        })?;
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        &rendered_condition,
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;
    Ok(s)
}

async fn drive_post_questionnaire_workflow(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<WorkflowOutput, WorkflowDriveError> {
    // Drive to the `staff_review` gate (shared with the matter-open
    // form), then render the assembled document for the result page.
    let s = advance_to_staff_review(state, notation_id).await?;
    let rendered = render_assembled_document(state, notation_id).await?;
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;

    // The review gate (Scorpio/Capricorn): a notation carrying custom
    // content — a custom clause or an answer the client entered themselves
    // — parks here at `staff_review` for a human. The attorney's approve
    // (`approve_send_post` → `render_and_park`) renders + parks that exact
    // PDF, and the deliberate send (`send_post` → `dispatch_signature`)
    // sends those bytes — so the bytes the attorney approved are the bytes
    // that get signed. A clean, machine-only intake (staff-entered answers,
    // no clause) keeps the dev-loop auto-approve so the shipped
    // retainer/trust demos are unchanged.
    if notation_has_custom_content(&state.db, notation_id, notation_row.person_id).await? {
        return Ok(WorkflowOutput {
            final_state: s,
            signature_request_id: None,
            rendered,
        });
    }

    // Clean machine-only intake (staff-entered answers, no clause): the
    // dev-loop auto-approve renders + parks, then dispatches in sequence.
    // Under the in-process `DispatchingRuntime` the `approved` signal
    // renders + persists synchronously, so the readiness probe passes
    // immediately and the demos/tests are unchanged; the two-call shape
    // matches the durable prod pipeline.
    render_and_park(state, notation_id).await?;
    let (final_state, signature_request_id) = dispatch_signature(state, notation_id).await?;
    Ok(WorkflowOutput {
        final_state,
        signature_request_id: Some(signature_request_id),
        rendered,
    })
}

/// Whether a notation carries content that must cross attorney review
/// before signature: any custom clause, or any answer the client entered
/// themselves (`answers.source = client`). The respondent is the
/// notation's bound Person.
async fn notation_has_custom_content(
    db: &store::Db,
    notation_id: Uuid,
    respondent_id: Uuid,
) -> Result<bool, WorkflowDriveError> {
    if store::notation_clauses::exists_for(db, notation_id).await? {
        return Ok(true);
    }
    let client_answer = answer::Entity::find()
        .filter(answer::Column::PersonId.eq(respondent_id))
        .filter(answer::Column::Source.eq(store::entity::answer::SOURCE_CLIENT))
        .one(db)
        .await?;
    Ok(client_answer.is_some())
}

/// Render the reviewed document on the worker and PARK — the durable
/// first half of the send. From `staff_review`, fire `approved`
/// (threading the Typst `DocumentPayload` the worker renders + persists on
/// entering `document_open__*`), sync `notation.state` to the
/// `document_open__*` step, and return. It does **not** fire
/// `pdf_persisted`, read the PDF back, or send: the worker durably owns
/// render+persist, and the workflow waits at the document step for an
/// explicit [`dispatch_signature`].
///
/// Splitting render-and-park from the send is what makes the pipeline
/// durable against real Restate Cloud, where the worker's render+persist
/// is a separate invocation from the `web` request that fired `approved`
/// — synchronously reading the PDF back in the same request (the old
/// `assemble_and_send`) raced that invocation and 500'd. The bytes are
/// assembled from the *current* answers + clauses, so what the attorney
/// reviewed is what renders.
///
/// Self-contained so both the auto-path
/// ([`drive_post_questionnaire_workflow`]) and the attorney's explicit
/// [`approve_send_post`] reach it identically.
async fn render_and_park(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<StateName, WorkflowDriveError> {
    let runtime = state.workflow_runtime.as_ref();
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;

    // Re-assemble the body — template + custom clauses — and the answers
    // context, so the PDF reflects the latest reviewed content.
    let raw_template_body =
        store::templates::body(&state.db, &state.storage, &template_row).await?;
    let clauses = store::notation_clauses::for_notation(&state.db, notation_id).await?;
    let template_body = store::notation_clauses::splice(&raw_template_body, &clauses);
    let ctx = render_context_from_answers(
        &state.db,
        notation_row.person_id,
        &template_row,
        &raw_template_body,
    )
    .await?;

    // Two rendering paths, declared by the template: a `form:` binding
    // fills the vendored government packet's AcroForm from the answers
    // (the artifact the SoS receives is the state's own form); without
    // one, the body renders through Typst as before.
    let document_payload = if let Some(form_code) = template_row.form_code.as_deref() {
        acroform_payload(state, notation_id, form_code, &ctx).await?
    } else {
        // Expand signature placeholders into anchored Typst blocks. Only
        // the Typst source matters here; the placed fields are rebuilt at
        // send time from the same deterministic expansion.
        let (typst_source, _signature_fields) = crate::signature_render::expand_signatures(
            &substitute_template_body(&template_body, &ctx),
        );
        serde_json::to_string(&workflows::DocumentPayload::Typst {
            storage_key: document_pdf_storage_key(notation_id),
            typst_source,
        })
        .map_err(WorkflowDriveError::Payload)?
    };

    // Fire `approved`: the worker renders + persists the PDF on entering
    // `document_open__retainer_pdf`. We do NOT advance past it — the send
    // is a separate, deliberate command that first confirms the PDF
    // landed.
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "approved",
        Some(&document_payload),
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;
    Ok(s)
}

/// Build the `DocumentPayload::Acroform` JSON for a template with a
/// `form:` binding: resolve the field map against the answers, ensure
/// the vendored blank bytes exist in documents storage (an idempotent
/// put — same path in FsStorage, KIND, and prod), and point the worker
/// at the per-notation output key. Every resolution failure is loud:
/// a mis-mapped form must park the matter, never fill a blank.
async fn acroform_payload(
    state: &AdminState,
    notation_id: Uuid,
    form_code: &str,
    ctx: &BTreeMap<String, String>,
) -> Result<String, WorkflowDriveError> {
    let form_err = |reason: String| WorkflowDriveError::Form {
        form_code: form_code.to_string(),
        reason,
    };
    let form = forms::get(form_code)
        .map_err(|e| form_err(e.to_string()))?
        .ok_or_else(|| form_err("not in the vendored forms registry".into()))?;
    let map = forms::field_map(form_code)
        .map_err(|e| form_err(e.to_string()))?
        .ok_or_else(|| form_err("no field map vendored for this form".into()))?;
    let fields = forms::resolve(&map, ctx).map_err(|e| form_err(e.to_string()))?;

    let blank_form_key = form.meta.object_path.to_string();
    let legacy_blank_form_key = format!("templates/{}", form.meta.object_path);
    let blank_form_key = if state.storage.exists(&blank_form_key).await? {
        blank_form_key
    } else if state.storage.exists(&legacy_blank_form_key).await? {
        legacy_blank_form_key
    } else {
        state
            .storage
            .put(&blank_form_key, form.bytes, "application/pdf")
            .await?;
        blank_form_key
    };
    serde_json::to_string(&workflows::DocumentPayload::Acroform {
        storage_key: document_pdf_storage_key(notation_id),
        blank_form_key,
        fields,
    })
    .map_err(WorkflowDriveError::Payload)
}

/// Whether the worker has rendered + persisted the notation's document
/// PDF — a cheap existence probe on [`document_pdf_storage_key`] (a
/// metadata-only HEAD on GCS). [`dispatch_signature`] gates on this before
/// advancing the workflow and sending the envelope, and `notation status`
/// surfaces it as `document_ready`, so a misconfigured worker that never
/// wrote the PDF is visible rather than an opaque 500 at send time.
pub(crate) async fn document_pdf_ready(
    storage: &dyn cloud::StorageService,
    notation_id: Uuid,
) -> Result<bool, cloud::StorageError> {
    storage.exists(&document_pdf_storage_key(notation_id)).await
}

/// Dispatch the rendered document for signature — the deliberate,
/// authenticated "send" half of the pipeline. Confirms the worker's PDF
/// is present, fires `pdf_persisted` (→ `sent_for_signature__pending`),
/// reads the persisted PDF back, builds the manifest, and sends exactly
/// one envelope, persisting the `signature_request_id`.
///
/// Idempotent (find-or-create keyed on `notation_id`): a notation that
/// already carries a `signature_request_id` reuses it and neither
/// re-fires the transition nor re-sends. When the PDF isn't present yet
/// (the worker hasn't rendered, or its storage is misconfigured), returns
/// [`WorkflowDriveError::DocumentNotReady`] so the caller can answer
/// "not yet — retry" (a `409`) instead of looping or 500-ing. The
/// provider ALSO sends an X-DocuSign-Idempotency-Key so a concurrent
/// double-send dedupes at DocuSign, not just on the id check here.
async fn dispatch_signature(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<(StateName, crate::signature::SignatureRequestId), WorkflowDriveError> {
    let runtime = state.workflow_runtime.as_ref();
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;

    // Idempotency: this notation already has an envelope out. Reuse the
    // persisted id, fire nothing, send nothing — the post-state is
    // whatever the notation already records.
    if let Some(existing) = notation_row.signature_request_id.clone() {
        return Ok((
            StateName::from(notation_row.state.as_str()),
            crate::signature::SignatureRequestId(existing),
        ));
    }

    // Readiness gate: the worker durably renders + persists the PDF on
    // entering `document_open__*`. Confirm it landed before advancing —
    // against real Restate Cloud that render is a separate invocation, so
    // "approved fired" does not imply "PDF written."
    if !document_pdf_ready(state.storage.as_ref(), notation_id).await? {
        return Err(WorkflowDriveError::DocumentNotReady(notation_id));
    }

    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "pdf_persisted",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    // Now at sent_for_signature__pending; fire the signature seam.
    //
    // Re-resolve the template + answers so the manifest's recipients and
    // the placed signature fields match the bytes the worker rendered: the
    // client signs first (routing 1), the firm countersigns (routing 2) so
    // the engagement forms on the firm's signature. The captive client's
    // identity comes from the questionnaire answers when present (the
    // retainer asks `client_name`/`client_email`) and otherwise from the
    // notation's bound Person row — never hardcoded in the provider.
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let raw_template_body =
        store::templates::body(&state.db, &state.storage, &template_row).await?;
    let clauses = store::notation_clauses::for_notation(&state.db, notation_id).await?;
    let template_body = store::notation_clauses::splice(&raw_template_body, &clauses);
    let ctx = render_context_from_answers(
        &state.db,
        notation_row.person_id,
        &template_row,
        &raw_template_body,
    )
    .await?;
    let (_typst_source, signature_fields) =
        crate::signature_render::expand_signatures(&substitute_template_body(&template_body, &ctx));

    // Read the PDF the worker persisted back from storage so the bytes
    // sent are exactly the bytes stored (one renderer, no second
    // in-process copy to drift).
    let pdf_bytes = state
        .storage
        .get(&document_pdf_storage_key(notation_id))
        .await?
        .bytes;
    let client = person::Entity::find_by_id(notation_row.person_id)
        .one(&state.db)
        .await?;
    // `emailed` delivery → non-captive client (DocuSign emails the signing
    // link); anything else (`embedded`, the default) keeps the captive
    // embedded-signing recipient. Read off the notation so the single send
    // path serves both without a second route.
    let captive = notation_row.delivery != store::entity::notation::DELIVERY_EMAILED;
    let manifest = build_signature_manifest(
        notation_id,
        &signature_fields,
        &ctx,
        client.as_ref(),
        captive,
    );
    let id = state
        .signature_provider
        .send_for_signature(notation_id, &pdf_bytes, &manifest)
        .await?;
    // Persist the request id so the inbound completion webhook
    // (`crate::esignature_webhook`) can resolve its callback back to this
    // notation.
    persist_signature_request_id(&state.db, notation_id, &id.0).await?;

    Ok((s, id))
}

/// POST `/portal/admin/notations/:id/approve-send` — the attorney
/// approves a notation parked at `staff_review` (it carried custom
/// content). This now renders + parks only: it fires `approved` so the
/// worker durably renders + persists the reviewed bytes and the workflow
/// waits at `document_open__retainer_pdf`. The binding send is a separate,
/// deliberate command ([`send_post`] / `navigator retainer send`) that
/// first confirms the PDF landed — so a real Restate Cloud worker's
/// render never races the send.
pub async fn approve_send_post(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let token = csrf_token(session.as_deref()).to_string();

    // Idempotent approve: if the worker has already rendered + persisted
    // this notation's PDF — a prior approve, or the auto-approve a clean
    // machine-only intake takes when it walks straight through to
    // signature (`drive_post_questionnaire_workflow`) — approving again is
    // a no-op success. The bytes the attorney would approve already exist,
    // and re-firing `approved` from a state with no such edge (e.g.
    // `sent_for_signature__pending`) would otherwise 500 with NoTransition.
    // The matter-open retainer path parks at `staff_review` with no PDF
    // yet, so this guard never short-circuits a genuine first approve.
    if document_pdf_ready(state.storage.as_ref(), notation_id)
        .await
        .unwrap_or(false)
    {
        let notation_row = notation::Entity::find_by_id(notation_id)
            .one(&state.db)
            .await
            .ok()
            .flatten();
        let workflow_state = notation_row
            .as_ref()
            .map_or_else(String::new, |n| n.state.clone());
        let signature_request_id = notation_row
            .as_ref()
            .and_then(|n| n.signature_request_id.clone());
        let rendered = render_assembled_document(&state, notation_id)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, %notation_id, "approve_send: re-render (already-ready) failed");
                maud::html! {}
            });
        return views::pages::admin::retainers::result(
            &views::pages::admin::retainers::IntakeResult {
                notation_id,
                workflow_state: &workflow_state,
                signature_request_id: signature_request_id.as_deref(),
                rendered,
                csrf_token: &token,
            },
        )
        .into_response();
    }

    match render_and_park(&state, notation_id).await {
        Ok(final_state) => {
            // Re-render the assembled document for the result page so the
            // attorney sees exactly what is being rendered for signature.
            let rendered = render_assembled_document(&state, notation_id)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, %notation_id, "approve_send: re-render failed");
                    maud::html! {}
                });
            // No signature_request_id yet — the result view shows the
            // "Send for signature" action for the parked document.
            views::pages::admin::retainers::result(&views::pages::admin::retainers::IntakeResult {
                notation_id,
                workflow_state: final_state.as_str(),
                signature_request_id: None,
                rendered,
                csrf_token: &token,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "approve_send: render_and_park failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "approve failed").into_response()
        }
    }
}

/// POST `/portal/admin/notations/:id/send` — dispatch the rendered
/// document for signature. The deliberate, authenticated "send" half of
/// the pipeline, reached from the browser's "Send for signature" button
/// and the `navigator retainer send` CLI command.
///
/// Confirms the worker's PDF is present, then sends exactly one envelope
/// (see [`dispatch_signature`]). When the PDF isn't ready yet — the worker
/// hasn't rendered, or its storage is misconfigured — it returns `409`
/// with a JSON `{error, reason}` body so the operator gets an actionable
/// "not yet, retry" instead of an opaque 500 or a silent retry loop.
pub async fn send_post(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let token = csrf_token(session.as_deref()).to_string();
    match dispatch_signature(&state, notation_id).await {
        Ok((final_state, signature_request_id)) => {
            let rendered = render_assembled_document(&state, notation_id)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, %notation_id, "send: re-render failed");
                    maud::html! {}
                });
            views::pages::admin::retainers::result(&views::pages::admin::retainers::IntakeResult {
                notation_id,
                workflow_state: final_state.as_str(),
                signature_request_id: Some(&signature_request_id.0),
                rendered,
                csrf_token: &token,
            })
            .into_response()
        }
        Err(WorkflowDriveError::DocumentNotReady(_)) => {
            tracing::info!(%notation_id, "send: document not ready yet");
            (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "error": "document_not_ready",
                    "reason": "the retainer PDF has not been rendered yet — \
                               the worker is still rendering, or its storage is \
                               misconfigured. Re-run send in a moment; check \
                               `notation status` for document_ready.",
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %notation_id, "send: dispatch_signature failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "send failed").into_response()
        }
    }
}

/// GET `/portal/admin/notations/:id/review` — the review/approve screen
/// for a notation parked at `staff_review`. Renders the exact assembled
/// document plus an "Approve and send for signature" button (the result
/// view's awaiting state). The matter-open form lands staff here after
/// opening a matter with a retainer; the attorney reviews, then approves,
/// which is the only thing that emits the envelope. Idempotent to
/// revisit: once the envelope is out, the page reflects the sent state
/// (no approve button).
pub async fn review_get(
    State(state): State<AdminState>,
    AxumPath(notation_id): AxumPath<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ReviewQuery>,
    session: Option<Extension<SessionData>>,
) -> Response {
    let token = csrf_token(session.as_deref()).to_string();
    let Some(notation_row) = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()
    else {
        return (StatusCode::NOT_FOUND, "notation not found").into_response();
    };

    // `?format=json` is the machine-readable view the `navigator notation
    // status` CLI command reads — the workflow state, the signature
    // request id (present once an envelope has gone out), and
    // `document_ready` (whether the worker has rendered + persisted the
    // PDF, the gate the `send` command honors). HTML scraping of the
    // review page is brittle, so the CLI gets a narrow JSON branch on this
    // same handler rather than a parallel API tree.
    if q.format.as_deref() == Some("json") {
        // Per-matter pipeline state: a `StorageService` existence probe on
        // the document PDF key. A storage error here is non-fatal to the
        // status read — report `document_ready:false` and let the operator
        // retry rather than 500 the status call.
        let document_ready = document_pdf_ready(state.storage.as_ref(), notation_id)
            .await
            .unwrap_or(false);
        return axum::Json(serde_json::json!({
            "notation_id": notation_id,
            "state": notation_row.state,
            "signature_request_id": notation_row.signature_request_id,
            "delivery": notation_row.delivery,
            "document_ready": document_ready,
        }))
        .into_response();
    }

    let rendered = render_assembled_document(&state, notation_id)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, %notation_id, "review_get: render failed");
            maud::html! {}
        });
    views::pages::admin::retainers::result(&views::pages::admin::retainers::IntakeResult {
        notation_id,
        workflow_state: notation_row.state.as_str(),
        signature_request_id: notation_row.signature_request_id.as_deref(),
        rendered,
        csrf_token: &token,
    })
    .into_response()
}

/// Re-render a notation's assembled document (template body + custom
/// clauses + answers) to the HTML preview, for the result page.
async fn render_assembled_document(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<Markup, WorkflowDriveError> {
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let raw_template_body =
        store::templates::body(&state.db, &state.storage, &template_row).await?;
    let clauses = store::notation_clauses::for_notation(&state.db, notation_id).await?;
    let template_body = store::notation_clauses::splice(&raw_template_body, &clauses);
    let ctx = render_context_from_answers(
        &state.db,
        notation_row.person_id,
        &template_row,
        &raw_template_body,
    )
    .await?;
    Ok(views::notation::render_filled_in(&template_body, &ctx))
}

/// The product's flat matter-close fee, in cents, keyed on the
/// *originating* template code (the work the matter did, not the closing
/// letter). `None` for matters with no flat close fee. This is the one
/// place the firm's flat prices meet the accounting seam.
///
/// The amounts are no longer hand-written here: they come from the
/// product catalog (`store::products::matter_close_fee_cents`), the
/// single source of truth for each product's list price. Only a product
/// whose `billing_kind` is `matter_close_flat` resolves to a fee, so
/// Nautilus (recurring) and 1337 (hourly) correctly raise nothing.
pub async fn flat_fee_cents(db: &store::Db, template_code: &str) -> anyhow::Result<Option<i64>> {
    Ok(store::products::matter_close_fee_cents(db, template_code).await?)
}

/// Convert a notation's recorded admin-discretion discount into the
/// billing-seam [`billing::LineDiscount`]. A percent takes precedence if
/// both columns are somehow set (they shouldn't be — `store::notations`
/// enforces exactly-one when recording). `None` when the notation carries
/// no discount, which is the common case.
fn notation_line_discount(n: &notation::Model) -> Option<billing::LineDiscount> {
    if let Some(pct) = n.discount_pct {
        return Some(billing::LineDiscount::Percent(
            u32::try_from(pct.max(0)).unwrap_or(0),
        ));
    }
    n.discount_amount_cents
        .map(billing::LineDiscount::AmountCents)
}

/// Raise the flat matter-close fee through the billing seam when the firm
/// signs the closing letter. The amount follows the matter's *originating*
/// work notation (the estate, the entity, the engagement), not the closing
/// letter itself; the payer is the closing letter's respondent (the
/// client). Idempotent on the project id at the provider, so a replay or a
/// double-close never double-bills. A matter whose product carries no flat
/// close fee is a no-op.
pub async fn raise_matter_close_fee(
    state: &AdminState,
    closing_notation_id: Uuid,
) -> anyhow::Result<()> {
    let closing = notation::Entity::find_by_id(closing_notation_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("closing notation {closing_notation_id} not found"))?;

    // Find the originating work notation in the same project — anything but
    // the closing letter — and resolve its flat fee.
    let siblings = notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(closing.project_id))
        .all(&state.db)
        .await?;
    let mut fee_origin: Option<(i64, String, notation::Model)> = None;
    for n in &siblings {
        let Some(t) = template::Entity::find_by_id(n.template_id)
            .one(&state.db)
            .await?
        else {
            continue;
        };
        if t.code == CLOSING_TEMPLATE_CODE {
            continue;
        }
        if let Some(cents) = flat_fee_cents(&state.db, &t.code).await? {
            fee_origin = Some((cents, t.code, n.clone()));
            break;
        }
    }
    let Some((cents, code, origin)) = fee_origin else {
        return Ok(()); // no flat close fee for this matter
    };

    let client = person::Entity::find_by_id(closing.person_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("closing notation has no client person"))?;

    let payload = billing::MatterCloseInvoiceRequest {
        project_id: closing.project_id,
        person_id: client.id,
        contact_name: client.name,
        contact_email: client.email,
        reference: format!("Matter {}", closing.project_id),
        description: format!("{code} flat fee"),
        amount_cents: cents,
        currency: "USD".into(),
        account_code: "200".into(),
        // An admin-discretion discount recorded on the originating
        // notation flows onto the single invoice line as a Xero discount.
        discount: notation_line_discount(&origin),
    };
    // Below-only guardrail (RPC 7.1): a discount may never raise the
    // charge above the catalog list price. Reject before any provider
    // call so a bad discount never reaches Xero.
    payload.validate_discount()?;

    // Durable path: when a Restate broker is configured, fire the
    // `MatterCloseInvoice` workflow (keyed on the project id) and let it
    // retry the Xero call until it lands, then persist. This replaces the
    // old best-effort inline raise where a Xero outage silently dropped
    // the fee. The trigger is one-way; the workflow owns the retries.
    if let Some(broker) = std::env::var("RESTATE_BROKER_URL")
        .ok()
        .filter(|b| !b.is_empty())
    {
        let token = std::env::var("RESTATE_AUTH_TOKEN").ok();
        match workflows::start_workflow(
            &broker,
            token.as_deref(),
            "MatterCloseInvoice",
            &closing.project_id.to_string(),
            "run",
            &payload,
            true,
        )
        .await
        {
            Ok(resp) => {
                tracing::info!(project_id = %closing.project_id, response = %resp, "matter-close invoice workflow triggered");
                return Ok(());
            }
            // Trigger couldn't reach the broker — fall back to the inline
            // raise so the fee still lands. Both Xero calls are idempotent
            // on the same keys, so a later workflow run never double-bills.
            Err(e) => {
                tracing::error!(error = %e, project_id = %closing.project_id, "matter-close workflow trigger failed; raising inline");
            }
        }
    }

    raise_matter_close_fee_inline(state, &payload).await
}

/// Inline raise + persist used in dev/KIND/tests (no Restate broker) and
/// as the trigger-failure fallback. Mirrors the `MatterCloseInvoice`
/// workflow's two steps through the shared `billing` seam + `store`
/// helpers: resolve the contact, raise the `ACCREC` invoice, mirror it,
/// and cache the payer's `xero_contact_id`. Idempotent on `project_id`
/// (invoice) and email (contact), so it is safe to re-run.
async fn raise_matter_close_fee_inline(
    state: &AdminState,
    payload: &billing::MatterCloseInvoiceRequest,
) -> anyhow::Result<()> {
    let contact = state
        .billing_provider
        .ensure_contact(&billing::ContactRequest {
            name: payload.contact_name.clone(),
            email: payload.contact_email.clone(),
        })
        .await?;
    let request = billing::InvoiceRequest {
        contact_name: payload.contact_name.clone(),
        contact_email: payload.contact_email.clone(),
        reference: payload.reference.clone(),
        line_items: vec![billing::InvoiceLine {
            description: payload.description.clone(),
            quantity: 1,
            unit_amount_cents: payload.amount_cents,
            account_code: payload.account_code.clone(),
            discount: payload.discount.clone(),
        }],
    };
    let invoice_id = state
        .billing_provider
        .create_invoice(payload.project_id, &request)
        .await?;
    store::xero_invoices::upsert(
        &state.db,
        &store::xero_invoices::UpsertXeroInvoice {
            project_id: payload.project_id,
            xero_invoice_id: invoice_id.0,
            reference: payload.reference.clone(),
            status: "AUTHORISED".into(),
            // Mirror the *net* (list − discount), matching the client view.
            amount_cents: payload.net_amount_cents(),
            currency: payload.currency.clone(),
        },
    )
    .await?;
    store::persons::set_xero_contact_id(&state.db, payload.person_id, &contact.0).await?;
    Ok(())
}

/// Storage-key convention for the closing letter PDF of a notation.
#[must_use]
pub fn closing_letter_storage_key(notation_id: Uuid) -> String {
    format!("notations/{notation_id}/closing-letter.pdf")
}

/// Fetch the template `code` bound to a notation, if resolvable. Used
/// to pick the post-questionnaire workflow drive.
async fn notation_template_code(db: &store::Db, notation_id: Uuid) -> Option<String> {
    let n = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await
        .ok()
        .flatten()?;
    let t = template::Entity::find_by_id(n.template_id)
        .one(db)
        .await
        .ok()
        .flatten()?;
    Some(t.code)
}

/// Drive the closing-letter workflow for an already-walked closing
/// notation:
///
///   close_requested → staff_review
///   approved        → document_open__closing_letter  (render + persist)
///   pdf_persisted   → firm_signature__closing_letter
///   signed          → END
///
/// The mirror of [`drive_post_questionnaire_workflow`], but the closing
/// letter is signed by the *firm*, not the client — there is no
/// e-signature send. The status flip `open` → `closed` is the runtime's
/// `close_matter` side effect on the firm-signature transition. Returns
/// the terminal state (END).
async fn drive_closing_workflow(
    state: &AdminState,
    notation_id: Uuid,
) -> Result<StateName, WorkflowDriveError> {
    let yaml = workflows::bundled_spec_yaml("closing__letter")
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let spec = workflows::workflow_spec_from_yaml(yaml)
        .map_err(|e| WorkflowDriveError::Spec(e.to_string()))?;
    let runtime = state.workflow_runtime.as_ref();

    StateMachineRuntime::start(runtime, MachineKind::Workflow, notation_id, &spec).await?;
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "close_requested",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    // Render the closing letter from the answers the walker just landed.
    let notation_row = notation::Entity::find_by_id(notation_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let template_row = template::Entity::find_by_id(notation_row.template_id)
        .one(&state.db)
        .await?
        .ok_or(WorkflowDriveError::TemplateMissing(notation_id))?;
    let template_body = store::templates::body(&state.db, &state.storage, &template_row).await?;
    let ctx = render_context_from_answers(
        &state.db,
        notation_row.person_id,
        &template_row,
        &template_body,
    )
    .await?;

    // Staff review short-circuits to `approved` in the dev loop (a real
    // staff-review handler swaps in for prod). The `approved` signal
    // threads the closing letter's Typst source + storage key; the
    // worker renders and persists the PDF on entering
    // `document_open__closing_letter`.
    let document_payload = serde_json::to_string(&workflows::DocumentPayload::Typst {
        storage_key: closing_letter_storage_key(notation_id),
        typst_source: substitute_template_body(&template_body, &ctx),
    })
    .map_err(WorkflowDriveError::Payload)?;
    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "approved",
        Some(&document_payload),
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    let s = StateMachineRuntime::signal(
        runtime,
        MachineKind::Workflow,
        notation_id,
        "pdf_persisted",
        None,
    )
    .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    // The firm signs the closing letter; this transition lands on END
    // and closes the matter (the runtime's `close_matter` side effect).
    let s =
        StateMachineRuntime::signal(runtime, MachineKind::Workflow, notation_id, "signed", None)
            .await?;
    sync_notation_state(&state.db, notation_id, s.as_str()).await?;

    Ok(s)
}

/// Substitute `{{question_code}}` placeholders in the markdown body
/// the same way `views::notation::render_filled_in` does, but return
/// plain text suitable for feeding into the Typst compiler.
fn substitute_template_body(body: &str, ctx: &BTreeMap<String, String>) -> String {
    let mut out = body.to_string();
    for (code, value) in ctx {
        out = out.replace(&format!("{{{{{code}}}}}"), value);
    }
    out
}

/// The captive `clientUserId` for the client recipient of `notation_id`.
/// It must be replayed verbatim when requesting the embedded recipient
/// view (see [`crate::esign_view`]), so both sides derive it from the
/// notation id rather than storing it.
#[must_use]
pub fn client_user_id(notation_id: Uuid) -> String {
    format!("client-{notation_id}")
}

/// Assemble the signature manifest from the placed fields. Only roles
/// that actually anchor a field become recipients, in routing order:
/// the client (routing 1) signs from their questionnaire answers; the
/// firm (routing 2) countersigns from the `DOCUSIGN_SIGNER_*` config
/// (defaulting to the firm support inbox, mirroring
/// `DocuSignSignatureProvider::from_env`). Empty fields → empty manifest
/// (the provider's single-signer fallback).
///
/// The captive client's name/email come from the questionnaire answers
/// when the template captured them (the retainer asks
/// `client_name`/`client_email`) and otherwise from the notation's bound
/// Person row in `client` (the trust questionnaire never asks for an
/// email). That same Person is what [`crate::esign_view`] resolves the
/// embedded recipient against, so envelope creation and the recipient
/// view agree.
///
/// The client is **captive** (a `client_user_id` derived from the
/// notation): they sign embedded inside Neon Law Navigator, so DocuSign does not
/// email them. The firm is left non-captive — it countersigns from the
/// support inbox via the usual emailed link.
fn build_signature_manifest(
    notation_id: Uuid,
    fields: &[crate::signature::SignatureField],
    ctx: &BTreeMap<String, String>,
    client: Option<&person::Model>,
    captive: bool,
) -> crate::signature::SignatureManifest {
    use crate::signature::{SignatureManifest, SignatureRecipient};
    if fields.is_empty() {
        return SignatureManifest::default();
    }
    let role_present = |role: &str| fields.iter().any(|f| f.recipient_role == role);
    let env = |k: &str, default: &str| {
        std::env::var(k)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    };
    // Prefer the answered value; fall back to the bound Person when the
    // questionnaire didn't capture it (empty answers fall back too).
    let answered = |key: &str| ctx.get(key).filter(|s| !s.is_empty()).cloned();

    let mut recipients = Vec::new();
    if role_present("client") {
        recipients.push(SignatureRecipient {
            role: "client".into(),
            email: answered("client_email")
                .or_else(|| client.map(|c| c.email.clone()))
                .unwrap_or_default(),
            name: answered("client_name")
                .or_else(|| client.map(|c| c.name.clone()))
                .unwrap_or_default(),
            routing_order: 1,
            // Captive (`embedded` delivery): a `client_user_id` makes the
            // client an embedded recipient DocuSign does NOT email — they
            // sign inside Neon Law Navigator (`crate::esign_view`). Non-captive
            // (`emailed` delivery): `None`, so DocuSign emails them a
            // signing link they open from their own inbox.
            client_user_id: captive.then(|| client_user_id(notation_id)),
        });
    }
    if role_present("firm") {
        recipients.push(SignatureRecipient {
            role: "firm".into(),
            email: env("DOCUSIGN_SIGNER_EMAIL", "support@neonlaw.com"),
            name: env("DOCUSIGN_SIGNER_NAME", "Neon Law"),
            routing_order: 2,
            client_user_id: None,
        });
    }
    SignatureManifest {
        recipients,
        fields: fields.to_vec(),
    }
}

/// Build the `{{question_code}} → answer` context map for `person_id`,
/// keyed by question code with the latest answer per code winning.
/// Template-agnostic: it surfaces whatever codes the bound
/// questionnaire collected (the retainer's `client_name`/…, the trust's
/// `trustee_name`/`trust_property`), so a template body interpolates
/// only its own placeholders and any extra keys are inert.
async fn render_context_from_answers(
    db: &store::Db,
    person_id: Uuid,
    template_row: &template::Model,
    template_body: &str,
) -> Result<BTreeMap<String, String>, sea_orm::DbErr> {
    let answers = answer::Entity::find()
        .filter(answer::Column::PersonId.eq(person_id))
        .order_by_asc(answer::Column::Id)
        .all(db)
        .await?;
    if answers.is_empty() {
        return Ok(BTreeMap::new());
    }
    // Resolve the question codes for the answered questions in one query.
    let question_ids: Vec<Uuid> = answers.iter().map(|a| a.question_id).collect();
    let code_by_id: BTreeMap<Uuid, String> = question::Entity::find()
        .filter(question::Column::Id.is_in(question_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|q| (q.id, q.code))
        .collect();
    // Collect every answer per canonical code in ascending-id (answer)
    // order. A canonical code can carry more than one answer: a re-answered
    // single state, or — the case typed prefixes intend — several distinct
    // template states that share one canonical question (e.g. two
    // `custom_text__*` fields both seeded as `custom_text`). The ordered
    // vector lets `add_template_state_aliases` map each answer back to the
    // state that produced it instead of collapsing them to one value.
    let mut values_by_code: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for a in answers {
        if let Some(code) = code_by_id.get(&a.question_id) {
            values_by_code
                .entry(code.clone())
                .or_default()
                .push(a.value);
        }
    }
    // The bare canonical key resolves to the latest answer — both for a
    // direct `{{code}}` placeholder and as the single-state default.
    let mut ctx = BTreeMap::new();
    for (code, values) in &values_by_code {
        if let Some(latest) = values.last() {
            ctx.insert(code.clone(), latest.clone());
        }
    }
    add_template_state_aliases(&mut ctx, &values_by_code, template_row, template_body);
    Ok(ctx)
}

/// Map each answered value onto the template state that produced it.
///
/// Answers for a canonical question are stored under one `question_id`, so
/// two states sharing a canonical code (`custom_text__a`, `custom_text__b`)
/// are distinguishable only by insertion order. We recover the mapping the
/// same way intake's `answered_client_states` does: walk the states in
/// questionnaire order and align them, front to front, against that code's
/// answers in answer order. When a code has more answers than states (a
/// re-answered single state is the common case), the oldest extras are
/// dropped so the freshest answer wins — preserving the prior
/// latest-answer-wins behaviour for the single-state path.
fn add_template_state_aliases(
    ctx: &mut BTreeMap<String, String>,
    values_by_code: &BTreeMap<String, Vec<String>>,
    template_row: &template::Model,
    template_body: &str,
) {
    let spec = workflows::bundled_spec_yaml(&template_row.code).map_or_else(
        || workflows::questionnaire_spec_from_template(template_body),
        workflows::questionnaire_spec_from_yaml,
    );
    let Ok(spec) = spec else {
        return;
    };
    let states = ordered_question_states(&spec);
    // How many states share each canonical prefix, so we know how many of a
    // code's answers belong to one document.
    let mut group_size: BTreeMap<&str, usize> = BTreeMap::new();
    for state in &states {
        *group_size.entry(canonical_prefix(state)).or_default() += 1;
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for state in &states {
        let prefix = canonical_prefix(state);
        let idx = seen.entry(prefix).or_default();
        let position = *idx;
        *idx += 1;
        let Some(values) = values_by_code.get(prefix) else {
            continue;
        };
        let k = group_size.get(prefix).copied().unwrap_or(1);
        // Align the last `k` answers to the `k` states, front to front.
        let start = values.len().saturating_sub(k);
        if let Some(value) = values.get(start + position) {
            ctx.entry(state.clone()).or_insert_with(|| value.clone());
        }
    }
}

/// Canonical question code for a state — the prefix before the first `__`.
fn canonical_prefix(state: &str) -> &str {
    state.split_once("__").map_or(state, |(prefix, _)| prefix)
}

/// The questionnaire's non-sentinel states in BEGIN-first order. Walks the
/// linear `_` transition chain (the only shape questionnaires take), then
/// appends any state the walk didn't reach so none is silently dropped.
fn ordered_question_states(spec: &workflows::QuestionnaireSpec) -> Vec<String> {
    let mut ordered: Vec<String> = Vec::new();
    let mut here = StateName::begin();
    while let Some(next) = spec
        .transitions_from(&here)
        .and_then(|t| t.lookup("_"))
        .cloned()
    {
        if next == StateName::end() {
            break;
        }
        ordered.push(next.as_str().to_string());
        here = next;
    }
    for state in spec.inner().states.keys() {
        let state = state.as_str();
        if state == StateName::BEGIN || state == StateName::END {
            continue;
        }
        if !ordered.iter().any(|s| s == state) {
            ordered.push(state.to_string());
        }
    }
    ordered
}

/// Stamp the e-signature provider's request id onto the notation so a
/// later completion webhook can find it. See
/// [`crate::esignature_webhook`].
async fn persist_signature_request_id(
    db: &store::Db,
    notation_id: Uuid,
    request_id: &str,
) -> Result<(), sea_orm::DbErr> {
    let existing = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("notation {notation_id}")))?;
    let mut active: notation::ActiveModel = existing.into();
    active.signature_request_id = ActiveValue::Set(Some(request_id.to_string()));
    active.update(db).await?;
    Ok(())
}

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

/// `(current, total)` for the progress indicator.
///
/// `total` is the count of *question* states in the spec — every
/// state name except `BEGIN` and `END`. `current` is `1 + index of
/// the next question after `current_state` among the question
/// states, ordered by walking the spec from BEGIN. If
/// `current_state` is `BEGIN`, we're on question 1.
fn progress_for(spec: &workflows::QuestionnaireSpec, current_state: &StateName) -> (usize, usize) {
    let mut order: Vec<StateName> = Vec::new();
    let mut here = StateName::begin();
    while let Some(next) = spec
        .transitions_from(&here)
        .and_then(|t| t.lookup("_"))
        .cloned()
    {
        if next == StateName::end() {
            break;
        }
        order.push(next.clone());
        here = next;
    }
    let total = order.len();
    let current = if current_state == &StateName::begin() {
        1
    } else {
        order
            .iter()
            .position(|s| s == current_state)
            .map_or(total, |i| i + 2)
            .min(total)
    };
    (current, total)
}

#[cfg(test)]
mod tests {
    use super::{add_template_state_aliases, progress_for};
    use std::collections::BTreeMap;
    use store::entity::template;
    use uuid::Uuid;
    use workflows::{retainer_intake_questionnaire, StateName};

    #[test]
    fn progress_for_begin_is_step_1() {
        let spec = retainer_intake_questionnaire();
        assert_eq!(progress_for(&spec, &StateName::begin()), (1, 4));
    }

    #[test]
    fn progress_for_client_name_state_is_step_2() {
        // After answering client_name (state machine moved to
        // `client_name`), the next question is client_email — the
        // walker should display "step 2 of 4."
        let spec = retainer_intake_questionnaire();
        assert_eq!(progress_for(&spec, &StateName::from("client_name")), (2, 4));
    }

    #[test]
    fn progress_for_last_answered_question_caps_at_total() {
        let spec = retainer_intake_questionnaire();
        assert_eq!(
            progress_for(&spec, &StateName::from("product_description")),
            (4, 4)
        );
    }

    #[test]
    fn state_aliases_parse_non_bundled_template_frontmatter() {
        let template_row = template::Model {
            id: Uuid::nil(),
            code: "project_scoped__custom".into(),
            title: "Project custom".into(),
            respondent_type: "entity".into(),
            project_id: None,
            blob_id: None,
            form_code: None,
            inserted_at: String::new(),
            updated_at: String::new(),
        };
        let body = "---
questionnaire:
  BEGIN:
    _: entity__company
  entity__company:
    _: END
  END: {}
workflow:
  BEGIN:
    _: staff_review
  staff_review:
    _: END
  END: {}
---
# {{entity__company}}
";
        let mut ctx = BTreeMap::from([("entity".into(), "Libra LLC".into())]);
        let values_by_code = BTreeMap::from([("entity".into(), vec!["Libra LLC".into()])]);

        add_template_state_aliases(&mut ctx, &values_by_code, &template_row, body);

        assert_eq!(
            ctx.get("entity__company").map(String::as_str),
            Some("Libra LLC")
        );
    }

    #[test]
    fn state_aliases_do_not_collapse_duplicate_typed_prefixes() {
        // Two `custom_text__*` states share the canonical `custom_text`
        // question, so both answers are stored under one question_id. Each
        // placeholder must render the answer the client gave for *that*
        // state, not the latest answer for the shared code.
        let template_row = template::Model {
            id: Uuid::nil(),
            code: "project_scoped__two_custom".into(),
            title: "Two custom fields".into(),
            respondent_type: "entity".into(),
            project_id: None,
            blob_id: None,
            form_code: None,
            inserted_at: String::new(),
            updated_at: String::new(),
        };
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__mission_statement
  custom_text__mission_statement:
    _: custom_text__revenue_strategy
  custom_text__revenue_strategy:
    _: END
  END: {}
prompts:
  mission_statement: Mission?
  revenue_strategy: Revenue?
workflow:
  BEGIN:
    _: staff_review
  staff_review:
    _: END
  END: {}
---
# {{custom_text__mission_statement}} / {{custom_text__revenue_strategy}}
";
        // Answer order matches questionnaire order: mission first, revenue
        // second (the state machine enforces this ordering at intake).
        let values_by_code = BTreeMap::from([(
            "custom_text".to_string(),
            vec![
                "Expand legal access".to_string(),
                "Flat-fee retainers".to_string(),
            ],
        )]);
        let mut ctx = BTreeMap::from([("custom_text".into(), "Flat-fee retainers".into())]);

        add_template_state_aliases(&mut ctx, &values_by_code, &template_row, body);

        assert_eq!(
            ctx.get("custom_text__mission_statement")
                .map(String::as_str),
            Some("Expand legal access"),
            "first typed state must keep its own answer, not the latest"
        );
        assert_eq!(
            ctx.get("custom_text__revenue_strategy").map(String::as_str),
            Some("Flat-fee retainers")
        );
    }

    #[test]
    fn state_aliases_single_state_uses_latest_answer() {
        // A re-answered single state keeps latest-answer-wins: two answers
        // for one `custom_text__*` state resolve to the freshest value.
        let template_row = template::Model {
            id: Uuid::nil(),
            code: "project_scoped__one_custom".into(),
            title: "One custom field".into(),
            respondent_type: "entity".into(),
            project_id: None,
            blob_id: None,
            form_code: None,
            inserted_at: String::new(),
            updated_at: String::new(),
        };
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__mission_statement
  custom_text__mission_statement:
    _: END
  END: {}
prompts:
  mission_statement: Mission?
workflow:
  BEGIN:
    _: staff_review
  staff_review:
    _: END
  END: {}
---
# {{custom_text__mission_statement}}
";
        let values_by_code = BTreeMap::from([(
            "custom_text".to_string(),
            vec!["First draft".to_string(), "Final answer".to_string()],
        )]);
        let mut ctx = BTreeMap::from([("custom_text".into(), "Final answer".into())]);

        add_template_state_aliases(&mut ctx, &values_by_code, &template_row, body);

        assert_eq!(
            ctx.get("custom_text__mission_statement")
                .map(String::as_str),
            Some("Final answer")
        );
    }
}
