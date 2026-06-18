//! `aida_send_welcome_email` MCP tool.
//!
//! Re-fires the firm's welcome email at an existing person — the same
//! "Welcome to Neon Law" message the OAuth callback sends on a
//! brand-new signup and the `/portal/admin/people` "Send welcome"
//! button sends on demand. All three share one template + render path
//! ([`workflows::email::welcome`]); this tool is the MCP/A2A door onto
//! it.
//!
//! **Trust boundary (per the council's Scorpio note):** the tool takes
//! a `person_id`, never a free-text email address. You can only welcome
//! someone already seeded in `persons`, so AIDA can't be turned into a
//! sender for arbitrary inboxes. Unknown id → `NotFound`.
//!
//! The name + email handed to [`trigger_welcome`] come from the DB row,
//! not from the caller — the model can't spoof who the greeting names
//! or where it lands. The send is idempotent on the broker side:
//! [`trigger_welcome`] keys the Restate invocation off `person_id`, so
//! a repeated call no-ops rather than double-sending.

use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::person;
use uuid::Uuid;
use workflows::email::welcome::{trigger_welcome, WELCOME_SUBJECT};

use super::ToolError;
use crate::server::McpState;

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_send_welcome_email",
        "description": "Send the firm's \"Welcome to Neon Law\" email to an existing \
                        person. This is the correct tool for any \"send/email a welcome\" \
                        request, even when the user names the recipient only by email \
                        address. Identify the recipient by their Navigator person_id: \
                        when you were given an email or name instead, call aida_show_person \
                        FIRST to resolve the person_id, then call this — do NOT create a \
                        new person. The email and name are read from that record, so you \
                        can only welcome someone already in the system, never an arbitrary \
                        address. Idempotent: re-sending to the same person is safe.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "person_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "UUID of the person to welcome. Must already exist \
                                    in Navigator (see aida_show_person)."
                }
            },
            "required": ["person_id"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    person_id: Uuid,
}

pub async fn call(state: &McpState, arguments: &Value) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let person = person::Entity::find_by_id(args.person_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ToolError::NotFound(format!("person_id={}", args.person_id)))?;

    // Name + email come from the row, never the caller — the recipient
    // is whoever the DB says it is.
    trigger_welcome(
        state.questionnaire_runtime.as_ref(),
        person.id,
        &person.name,
        &person.email,
    )
    .await
    .map_err(|e| ToolError::Internal(format!("welcome trigger failed: {e}")))?;

    let summary = format!(
        "Sent the welcome email to {} <{}> (id={}).",
        person.name, person.email, person.id
    );
    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "person_id": person.id,
            "name": person.name,
            "email": person.email,
            "subject": WELCOME_SUBJECT,
            "status": "sent",
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use crate::server::McpState;
    use crate::tools::ToolError;
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use serde_json::json;
    use std::sync::Arc;
    use store::entity::person;
    use uuid::Uuid;
    use workflows::InMemoryRuntime;

    async fn state() -> McpState {
        let db = store::test_support::pg().await;
        let runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(InMemoryRuntime::new());
        McpState::new(db, runtime)
    }

    async fn seed_person(db: &store::Db, name: &str, email: &str) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set(name.into()),
            email: ActiveValue::Set(email.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[test]
    fn descriptor_names_the_tool_and_requires_person_id() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_send_welcome_email");
        let required = d["inputSchema"]["required"].as_array().unwrap();
        assert_eq!(required, &vec![json!("person_id")]);
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
    }

    #[tokio::test]
    async fn happy_path_drives_runtime_and_returns_sent_status() {
        let state = state().await;
        let pid = seed_person(&state.db, "Aries", "aries@example.com").await;
        let r = call(&state, &json!({ "person_id": pid })).await.unwrap();
        assert_eq!(r["structuredContent"]["person_id"], pid.to_string());
        assert_eq!(r["structuredContent"]["name"], "Aries");
        assert_eq!(r["structuredContent"]["email"], "aries@example.com");
        assert_eq!(r["structuredContent"]["subject"], "Welcome to Neon Law");
        assert_eq!(r["structuredContent"]["status"], "sent");
        assert!(r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("aries@example.com"));
    }

    #[tokio::test]
    async fn name_and_email_come_from_the_row_not_the_caller() {
        // The schema forbids extra fields, but even if a caller smuggles
        // an `email`, the greeting must target the DB row's address.
        let state = state().await;
        let pid = seed_person(&state.db, "Real Person", "real@example.com").await;
        let r = call(
            &state,
            &json!({ "person_id": pid, "email": "attacker@evil.test" }),
        )
        .await
        .unwrap();
        assert_eq!(r["structuredContent"]["email"], "real@example.com");
    }

    #[tokio::test]
    async fn unknown_person_returns_not_found() {
        let state = state().await;
        let missing = Uuid::now_v7();
        let err = call(&state, &json!({ "person_id": missing }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn missing_person_id_is_invalid_arguments() {
        let state = state().await;
        let err = call(&state, &json!({})).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    // No in-memory idempotency test: re-send safety is a Restate-broker
    // property (the invocation is keyed off `person_id`, so a repeat
    // no-ops on the broker). `InMemoryRuntime` can't model that — once a
    // person's ephemeral workflow reaches the terminal `END`, a second
    // `start` has nothing to reset to. The property is exercised at the
    // wire level in the `workflows` runtime_restate tests.
}
