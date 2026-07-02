//! `aida_answer_notation` MCP tool.
//!
//! Submit one answer to a notation's questionnaire. Server
//! advances the state machine and tells the LLM either the next
//! question to ask or that the questionnaire is complete (so the
//! caller can hand off to the post-intake workflow).
//!
//! Always pair this with a prior `aida_create_notation` call —
//! the `notation_id` returned there is what gets echoed back
//! here. The `question_code` MUST match the code from the most
//! recent `next_question` response; mismatches are rejected so a
//! confused LLM fails fast.

use serde::Deserialize;
use serde_json::{json, Value};
use store::Db;
use uuid::Uuid;
use workflows::{notation_session, NextStep, NotationSessionError, StateMachineRuntime};

use super::ToolError;

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_answer_notation",
        "description":
            "Submit one answer to an in-flight notation questionnaire. \
             Pass the `notation_id` from `aida_create_notation` (or a \
             prior `aida_answer_notation` response), the `question_code` \
             from the most recent `next_question`, and the user's `value`. \
             Returns `status: \"needs_answer\"` with the next \
             `next_question` to ask, or `status: \"complete\"` once the \
             questionnaire reaches END (after which the caller should \
             trigger the post-intake workflow). Errors with `invalid \
             arguments` if `question_code` doesn't match what the \
             questionnaire is currently asking.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "notation_id": {
                    "type": "string",
                    "description":
                        "UUID returned by the most recent \
                         `aida_create_notation` (or echoed back from \
                         the prior `aida_answer_notation`)."
                },
                "question_code": {
                    "type": "string",
                    "description":
                        "Stable code of the question being answered. \
                         MUST match the `code` from the most recent \
                         `next_question`."
                },
                "value": {
                    "type": "string",
                    "description":
                        "The user's answer as a string. Even \
                         `answer_type: int`/`bool` are submitted as the \
                         textual rendering; the server stores them \
                         verbatim."
                }
            },
            "required": ["notation_id", "question_code", "value"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    notation_id: Uuid,
    question_code: String,
    value: String,
}

pub async fn call(
    db: &Db,
    runtime: &dyn StateMachineRuntime,
    storage: Option<&std::sync::Arc<dyn cloud::StorageService>>,
    arguments: &Value,
) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let question_code = args.question_code.trim();
    if question_code.is_empty() {
        return Err(ToolError::InvalidArguments(
            "`question_code` must not be blank".into(),
        ));
    }

    // AIDA answers as the firm's agent, not a Person row, so the answer
    // is staff-sourced with no individual typist.
    let next = notation_session::answer_step(
        db,
        runtime,
        storage,
        args.notation_id,
        question_code,
        args.value.as_str(),
        notation_session::AnswerAuthor::staff(None),
    )
    .await
    .map_err(map_notation_err)?;

    let (payload, summary) = match next {
        NextStep::NeedsAnswer { question } => {
            let prompt = question.prompt.clone();
            (
                json!({
                    "notation_id": args.notation_id,
                    "status": "needs_answer",
                    "next_question": {
                        "code": question.code,
                        "prompt": question.prompt,
                        "answer_type": question.answer_type,
                    }
                }),
                format!("Answer accepted. Ask the user: {prompt}"),
            )
        }
        NextStep::QuestionnaireComplete => (
            json!({
                "notation_id": args.notation_id,
                "status": "complete",
            }),
            format!(
                "Answer accepted. Questionnaire for notation {} complete; \
                 trigger the post-intake workflow next.",
                args.notation_id
            ),
        ),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": payload,
    }))
}

fn map_notation_err(err: NotationSessionError) -> ToolError {
    match err {
        NotationSessionError::TemplateNotFound(c) => ToolError::NotFound(format!("template `{c}`")),
        NotationSessionError::TemplateHasNoQuestionnaire(c) => {
            ToolError::InvalidArguments(format!("template `{c}` has no questionnaire"))
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
        NotationSessionError::SnapshotEncode(e) | NotationSessionError::SnapshotDecode(e) => {
            ToolError::Internal(format!("questionnaire snapshot: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use crate::tools::{create_notation, ToolError};
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use serde_json::{json, Value};
    use store::entity::{person, question, template};
    use uuid::Uuid;
    use workflows::InMemoryRuntime;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    async fn seed(db: &store::Db) {
        template::ActiveModel {
            code: ActiveValue::Set("onboarding__retainer".into()),
            title: ActiveValue::Set("Retainer".into()),
            respondent_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        question::ActiveModel {
            code: ActiveValue::Set("person".into()),
            prompt: ActiveValue::Set("Who is the person?".into()),
            answer_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        question::ActiveModel {
            code: ActiveValue::Set("project".into()),
            prompt: ActiveValue::Set("What is the project?".into()),
            answer_type: ActiveValue::Set("project".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        question::ActiveModel {
            code: ActiveValue::Set("custom_text".into()),
            prompt: ActiveValue::Set("Prompt for custom text".into()),
            answer_type: ActiveValue::Set("string".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            role: ActiveValue::Set(store::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        // The engagement opens a project, whose staff-side DRI is resolved
        // by `default_firm_dri` — seed a firm principal so it resolves.
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

    /// Helper: start a retainer via the create tool and return
    /// `(notation_id, first_question_code)`.
    async fn start_retainer(db: &store::Db, runtime: &InMemoryRuntime) -> (Uuid, String) {
        let eid = store::test_support::seed_entity(db).await;
        let repo_root = tempfile::tempdir().unwrap();
        let out = create_notation::call_with_repo_store(
            db,
            runtime,
            None,
            None,
            repos::RepoStore::new(repo_root.path()),
            &json!({
                "template_code": "onboarding__retainer",
                "person_email": "libra@example.com",
                "entity_id": eid,
            }),
        )
        .await
        .unwrap();
        let id: Uuid =
            serde_json::from_value(out["structuredContent"]["notation_id"].clone()).unwrap();
        let code = out["structuredContent"]["next_question"]["code"]
            .as_str()
            .unwrap()
            .to_string();
        (id, code)
    }

    #[test]
    fn descriptor_names_the_tool_under_aida_namespace() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_answer_notation");
        let required: Vec<&str> = d["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"notation_id"));
        assert!(required.contains(&"question_code"));
        assert!(required.contains(&"value"));
    }

    #[tokio::test]
    async fn answering_one_question_returns_the_next_one() {
        let db = db().await;
        seed(&db).await;
        let runtime = InMemoryRuntime::new();
        let (id, code) = start_retainer(&db, &runtime).await;
        assert_eq!(code, "person__client");

        let out = call(
            &db,
            &runtime,
            None,
            &json!({
                "notation_id": id,
                "question_code": code,
                "value": "Libra",
            }),
        )
        .await
        .unwrap();
        assert_eq!(out["structuredContent"]["status"], "needs_answer");
        assert_eq!(
            out["structuredContent"]["next_question"]["code"],
            "project__engagement"
        );
    }

    #[tokio::test]
    async fn full_walk_lands_on_complete_status() {
        let db = db().await;
        seed(&db).await;
        let runtime = InMemoryRuntime::new();
        let (id, mut code) = start_retainer(&db, &runtime).await;
        let values = [
            ("person__client", "Libra"),
            ("project__engagement", "Apollo"),
        ];
        let mut last: Value = Value::Null;
        for (expected_code, value) in values {
            assert_eq!(code, expected_code);
            let out = call(
                &db,
                &runtime,
                None,
                &json!({
                    "notation_id": id,
                    "question_code": code,
                    "value": value,
                }),
            )
            .await
            .unwrap();
            last = out.clone();
            if let Some(next_code) = out["structuredContent"]["next_question"]["code"].as_str() {
                code = next_code.to_string();
            }
        }
        assert_eq!(last["structuredContent"]["status"], "complete");
        assert!(last["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("trigger the post-intake workflow"));
    }

    #[tokio::test]
    async fn wrong_question_code_is_invalid_arguments() {
        let db = db().await;
        seed(&db).await;
        let runtime = InMemoryRuntime::new();
        let (id, _code) = start_retainer(&db, &runtime).await;
        let err = call(
            &db,
            &runtime,
            None,
            &json!({
                "notation_id": id,
                "question_code": "custom_text__settlement_terms",
                "value": "Apollo",
            }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::InvalidArguments(m) => {
                assert!(
                    m.contains("person__client") && m.contains("custom_text__settlement_terms")
                );
            }
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_notation_id_is_not_found() {
        let db = db().await;
        seed(&db).await;
        let runtime = InMemoryRuntime::new();
        let err = call(
            &db,
            &runtime,
            None,
            &json!({
                "notation_id": Uuid::nil(),
                "question_code": "person__client",
                "value": "Libra",
            }),
        )
        .await
        .unwrap_err();
        match err {
            ToolError::NotFound(m) => assert!(m.contains("notation")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn blank_question_code_is_invalid_arguments() {
        let db = db().await;
        seed(&db).await;
        let runtime = InMemoryRuntime::new();
        let (id, _) = start_retainer(&db, &runtime).await;
        let err = call(
            &db,
            &runtime,
            None,
            &json!({
                "notation_id": id,
                "question_code": "  ",
                "value": "Libra",
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }
}
