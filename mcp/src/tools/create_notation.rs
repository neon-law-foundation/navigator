//! `aida_create_notation` MCP tool.
//!
//! Kick off a conversational notation from a template. The tool
//! creates the Notation row, starts the questionnaire runtime,
//! and returns the first question so the LLM can ask the user
//! and then call `aida_answer_notation` with the answer. The
//! server is the sole owner of questionnaire state; the LLM is
//! the UI.
//!
//! Acting principal: when the MCP boundary has populated a
//! [`crate::Principal`] (production / authenticated dev), this
//! tool uses the principal's email to resolve the respondent —
//! the `person_email` argument is ignored if supplied, so the
//! LLM can't act on behalf of someone else. In pass-through mode
//! (no OAuth, no JWT — `OIDC_DISABLED=true`), the principal is
//! absent and the tool falls back to the `person_email` argument
//! so KIND/test paths keep working. Either way, the resolved
//! email MUST match an existing `persons` row.

use sea_orm::sea_query::{Expr, Func};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::{entity as ent, person, project};
use store::Db;
use uuid::Uuid;
use workflows::{notation_session, NextStep, NotationSessionError, StateMachineRuntime};

use crate::principal::Principal;

use super::ToolError;

/// Tool descriptor advertised by `tools/list`.
#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_create_notation",
        "description":
            "Start a conversational notation from a template. Looks up the \
             person by email (case-insensitive), creates a Notation, starts \
             the questionnaire state machine, and returns the first question \
             to ask. Reply to the user with the returned `prompt` verbatim; \
             once they answer, call `aida_answer_notation` with \
             `notation_id`, `question_code`, and `value` to advance. \
             Returns `next_question` (with `code`, `prompt`, `answer_type`) \
             while the questionnaire is in progress, or `status: \"complete\"` \
             if the template has no questions. Errors with `not found` if the \
             template code or person email don't resolve.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "template_code": {
                    "type": "string",
                    "description":
                        "Stable template code, e.g. `onboarding__retainer`, \
                         `ca__llc_operating_agreement`. Required."
                },
                "person_email": {
                    "type": "string",
                    "description":
                        "Email of the respondent. Case-insensitive exact \
                         match against an existing `persons` row. \
                         IGNORED when the MCP boundary has authenticated \
                         the caller — in that case the respondent is the \
                         signed-in user. Required only in pass-through \
                         mode (no OAuth)."
                },
                "entity_id": {
                    "type": "string",
                    "description":
                        "Optional UUID of the legal entity the notation is \
                         for (LLC, trust, etc.). Omit for person-only \
                         templates like the retainer."
                }
            },
            "required": ["template_code"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    template_code: String,
    #[serde(default)]
    person_email: Option<String>,
    #[serde(default)]
    entity_id: Option<Uuid>,
}

pub async fn call(
    db: &Db,
    runtime: &dyn StateMachineRuntime,
    storage: Option<&std::sync::Arc<dyn cloud::StorageService>>,
    principal: Option<&Principal>,
    arguments: &Value,
) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let template_code = args.template_code.trim();
    if template_code.is_empty() {
        return Err(ToolError::InvalidArguments(
            "`template_code` must not be blank".into(),
        ));
    }

    // Trust order: authenticated principal first, fall back to the
    // caller-supplied `person_email` only when the MCP boundary
    // has no auth context (pass-through dev mode). This is the
    // line between "trusted email" and "model-supplied email".
    let email = if let Some(p) = principal {
        p.email.trim().to_string()
    } else {
        let raw = args
            .person_email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "`person_email` is required when the MCP boundary is unauthenticated".into(),
                )
            })?;
        raw.to_string()
    };

    let person_id = resolve_person_id(db, &email).await?;

    // A matter always opens against a pre-existing entity
    // (projects.entity_id is NOT NULL). Require it and validate it exists.
    let entity_id = args.entity_id.ok_or_else(|| {
        ToolError::InvalidArguments(
            "entity_id is required — open the engagement against an existing entity".into(),
        )
    })?;
    if ent::Entity::find_by_id(entity_id).one(db).await?.is_none() {
        return Err(ToolError::NotFound(format!("entity_id={entity_id}")));
    }

    // An Engagement opens a Project alongside its Notation. The
    // glossary's rule — every Notation belongs to exactly one
    // Project — is enforced by the schema; this is where the MCP
    // surface satisfies it.
    let project_id = open_project_for_engagement(db, &email, template_code, entity_id).await?;

    let outcome = notation_session::start_notation(
        db,
        runtime,
        storage,
        template_code,
        person_id,
        project_id,
        args.entity_id,
    )
    .await
    .map_err(map_notation_err)?;

    let payload = match outcome.next {
        NextStep::NeedsAnswer { question } => json!({
            "notation_id": outcome.notation_id,
            "status": "needs_answer",
            "next_question": {
                "code": question.code,
                "prompt": question.prompt,
                "answer_type": question.answer_type,
            }
        }),
        NextStep::QuestionnaireComplete => json!({
            "notation_id": outcome.notation_id,
            "status": "complete",
        }),
    };

    let summary = match payload["status"].as_str() {
        Some("needs_answer") => format!(
            "Started notation {}. Ask the user: {}",
            outcome.notation_id, payload["next_question"]["prompt"]
        ),
        _ => format!(
            "Started notation {} (template has no questions; ready for workflow).",
            outcome.notation_id
        ),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": payload,
    }))
}

/// Look up the unique person matching `email` case-insensitively.
/// Returns `NotFound` when zero rows match; if more than one row
/// matches (the dataset has duplicates), we pick the
/// alphabetically-first by id to keep behavior deterministic.
/// Insert a fresh Project to host this Engagement. Named after the
/// respondent + template so a staff member browsing the projects
/// list can see what the matter is at a glance.
async fn open_project_for_engagement(
    db: &Db,
    email: &str,
    template_code: &str,
    entity_id: Uuid,
) -> Result<Uuid, ToolError> {
    // Both DRI columns are NOT NULL. The engagement's respondent (resolved
    // by email) is the client-side DRI and must be a real `Role::Client`
    // person — the client of record is a client, never a firm attorney. The
    // staff side defaults to the firm principal (resolved by role).
    let client = person::Entity::find()
        .filter(Expr::expr(Func::lower(Expr::col(person::Column::Email))).eq(email.to_lowercase()))
        .order_by_asc(person::Column::Id)
        .one(db)
        .await?
        .ok_or_else(|| ToolError::NotFound(format!("person with email `{email}`")))?;
    if client.role != store::entity::person::Role::Client {
        return Err(ToolError::InvalidArguments(format!(
            "the matter's client `{email}` must be a client person, not {}",
            client.role.as_str()
        )));
    }
    let client_dri = client.id;
    let staff_dri = store::persons::default_firm_dri(db).await?.ok_or_else(|| {
        ToolError::InvalidArguments(
            "no firm principal to assign as staff DRI — seed a staff/admin person first".into(),
        )
    })?;
    let row = project::ActiveModel {
        name: ActiveValue::Set(format!("{template_code} for {email}")),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff_dri)),
        client_dri_person_id: ActiveValue::Set(Some(client_dri)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

async fn resolve_person_id(db: &Db, email: &str) -> Result<Uuid, ToolError> {
    let needle = email.to_lowercase();
    let row = person::Entity::find()
        .filter(Expr::expr(Func::lower(Expr::col(person::Column::Email))).eq(needle))
        .order_by_asc(person::Column::Id)
        .one(db)
        .await?
        .ok_or_else(|| ToolError::NotFound(format!("person with email `{email}`")))?;
    Ok(row.id)
}

fn map_notation_err(err: NotationSessionError) -> ToolError {
    match err {
        NotationSessionError::TemplateNotFound(c) => ToolError::NotFound(format!("template `{c}`")),
        NotationSessionError::TemplateHasNoQuestionnaire(c) => {
            ToolError::InvalidArguments(format!("template `{c}` has no questionnaire to walk"))
        }
        NotationSessionError::NotationNotFound(id) => {
            ToolError::NotFound(format!("notation `{id}`"))
        }
        NotationSessionError::QuestionMismatch { expected, got } => ToolError::InvalidArguments(
            format!("questionnaire is currently asking `{expected}`, not `{got}`"),
        ),
        NotationSessionError::AlreadyComplete => {
            ToolError::InvalidArguments("questionnaire is already complete".into())
        }
        NotationSessionError::Db(e) => e.into(),
        NotationSessionError::QuestionNotSeeded(c) => {
            ToolError::Internal(format!("question `{c}` not seeded in store"))
        }
        NotationSessionError::QuestionNotClientFacing(c) => {
            ToolError::InvalidArguments(format!("question `{c}` is not a client-facing question"))
        }
        NotationSessionError::Runtime(e) => ToolError::Internal(format!("workflow runtime: {e}")),
        NotationSessionError::Spec(e) => ToolError::Internal(format!("spec parse: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use crate::principal::Principal;
    use crate::tools::ToolError;
    use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
    use serde_json::json;
    use store::entity::{person, question, template};
    use uuid::Uuid;
    use workflows::InMemoryRuntime;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    async fn seed_retainer(db: &store::Db) {
        template::ActiveModel {
            code: ActiveValue::Set("onboarding__retainer".into()),
            title: ActiveValue::Set("Retainer".into()),
            respondent_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        for code in [
            "client_name",
            "client_email",
            "project_name",
            "product_description",
        ] {
            question::ActiveModel {
                code: ActiveValue::Set(code.into()),
                prompt: ActiveValue::Set(format!("Prompt for {code}")),
                answer_type: ActiveValue::Set("string".into()),
                ..Default::default()
            }
            .insert(db)
            .await
            .unwrap();
        }
        seed_firm_principal(db).await;
    }

    /// Seed a `Role::Admin` person so `default_firm_dri` can resolve a
    /// staff-side DRI when the engagement opens its project.
    async fn seed_firm_principal(db: &store::Db) {
        person::ActiveModel {
            name: ActiveValue::Set("Firm Principal".into()),
            email: ActiveValue::Set("principal@example.com".into()),
            role: ActiveValue::Set(store::entity::person::Role::Admin),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn seed_person(db: &store::Db, email: &str) {
        person::ActiveModel {
            name: ActiveValue::Set(email.into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(store::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[test]
    fn descriptor_names_the_tool_under_aida_namespace() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_create_notation");
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
        let required: Vec<&str> = d["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // `template_code` is the only hard requirement; the
        // boundary supplies the email when authenticated.
        assert!(required.contains(&"template_code"));
        assert!(!required.contains(&"person_email"));
    }

    #[tokio::test]
    async fn pass_through_path_uses_person_email_arg() {
        let db = db().await;
        seed_retainer(&db).await;
        seed_person(&db, "libra@example.com").await;
        let eid = store::test_support::seed_entity(&db).await;
        let runtime = InMemoryRuntime::new();

        let out = call(
            &db,
            &runtime,
            None,
            None,
            &json!({
                "template_code": "onboarding__retainer",
                "person_email": "libra@example.com",
                "entity_id": eid,
            }),
        )
        .await
        .unwrap();

        assert_eq!(out["structuredContent"]["status"], "needs_answer");
        assert!(out["structuredContent"]["notation_id"].is_string());
        assert_eq!(
            out["structuredContent"]["next_question"]["code"],
            "client_name"
        );
    }

    #[tokio::test]
    async fn authenticated_principal_overrides_person_email_arg() {
        // The arg points at someone else; the principal wins so a
        // signed-in user can't accidentally (or maliciously) start
        // a notation as another person.
        let db = db().await;
        seed_retainer(&db).await;
        seed_person(&db, "libra@example.com").await;
        seed_person(&db, "mallory@example.com").await;
        let eid = store::test_support::seed_entity(&db).await;
        let runtime = InMemoryRuntime::new();
        let principal = Principal::new("libra@example.com");

        let out = call(
            &db,
            &runtime,
            None,
            Some(&principal),
            &json!({
                "template_code": "onboarding__retainer",
                "person_email": "mallory@example.com",
                "entity_id": eid,
            }),
        )
        .await
        .unwrap();
        assert_eq!(out["structuredContent"]["status"], "needs_answer");

        // Verify the notation is bound to libra, not mallory.
        let row = store::entity::notation::Entity::find_by_id(
            serde_json::from_value::<Uuid>(out["structuredContent"]["notation_id"].clone())
                .unwrap(),
        )
        .one(&db)
        .await
        .unwrap()
        .unwrap();
        let libra = store::entity::person::Entity::find()
            .filter(person::Column::Email.eq("libra@example.com"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.person_id, libra.id);
    }

    #[tokio::test]
    async fn authenticated_principal_with_unknown_email_is_not_found() {
        let db = db().await;
        seed_retainer(&db).await;
        // No seeded person; principal email doesn't resolve.
        let runtime = InMemoryRuntime::new();
        let principal = Principal::new("ghost@example.com");
        let err = call(
            &db,
            &runtime,
            None,
            Some(&principal),
            &json!({ "template_code": "onboarding__retainer" }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::NotFound(m) => assert!(m.contains("person")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn email_match_is_case_insensitive() {
        let db = db().await;
        seed_retainer(&db).await;
        seed_person(&db, "Libra@Example.com").await;
        let eid = store::test_support::seed_entity(&db).await;
        let runtime = InMemoryRuntime::new();
        let out = call(
            &db,
            &runtime,
            None,
            None,
            &json!({
                "template_code": "onboarding__retainer",
                "person_email": "libra@example.COM",
                "entity_id": eid,
            }),
        )
        .await
        .unwrap();
        assert_eq!(out["structuredContent"]["status"], "needs_answer");
    }

    #[tokio::test]
    async fn unknown_template_is_not_found() {
        let db = db().await;
        seed_person(&db, "libra@example.com").await;
        seed_firm_principal(&db).await;
        let eid = store::test_support::seed_entity(&db).await;
        let runtime = InMemoryRuntime::new();
        let err = call(
            &db,
            &runtime,
            None,
            None,
            &json!({
                "template_code": "does_not_exist",
                "person_email": "libra@example.com",
                "entity_id": eid,
            }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::NotFound(m) => assert!(m.contains("template")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_person_is_not_found() {
        let db = db().await;
        seed_retainer(&db).await;
        let runtime = InMemoryRuntime::new();
        let err = call(
            &db,
            &runtime,
            None,
            None,
            &json!({
                "template_code": "onboarding__retainer",
                "person_email": "ghost@example.com",
            }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::NotFound(m) => assert!(m.contains("person")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pass_through_with_no_email_is_invalid_arguments() {
        // No principal, no person_email arg → caller hasn't told us
        // who's acting.
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let err = call(
            &db,
            &runtime,
            None,
            None,
            &json!({ "template_code": "onboarding__retainer" }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::InvalidArguments(m) => assert!(m.contains("person_email")),
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn blank_template_code_is_invalid_arguments() {
        let db = db().await;
        let runtime = InMemoryRuntime::new();
        let err = call(
            &db,
            &runtime,
            None,
            None,
            &json!({ "template_code": "  ", "person_email": "libra@example.com" }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }
}
