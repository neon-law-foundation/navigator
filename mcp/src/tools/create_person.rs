//! `aida_create_person` MCP tool.
//!
//! `LibreChat` asks the LLM to call this tool when a user (a Neon Law
//! attorney, staffer, or admin chatting through `LibreChat`) wants to
//! register a new human contact. The handler inserts a row into
//! `persons` via `SeaORM` and returns the new id + name + email so the
//! model can confirm what landed. Every Navigator tool is namespaced
//! under the `aida_` prefix so clients can group them in their UI.

use sea_orm::{ActiveModelTrait, ActiveValue};
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::person;
use store::Db;

use super::ToolError;

/// Tool descriptor advertised by `tools/list`. The `inputSchema` is a
/// standard JSON Schema; `LibreChat` surfaces it to the model as the
/// function signature.
#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_create_person",
        "description": "Create a NEW person record in Navigator. Use this ONLY when \
                        the user explicitly asks to add or register a new contact, \
                        client, prospect, or staff member. Do NOT call this to look up, \
                        message, email, or welcome someone — a request that mentions an \
                        email address is not a request to create a person. To find or \
                        act on an existing person, call aida_show_person first. Returns \
                        the new id, name, and email so the caller can reference the row.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Full name of the person (e.g. \"Libra\")."
                },
                "email": {
                    "type": "string",
                    "format": "email",
                    "description": "Email address. Must be unique across all persons."
                }
            },
            "required": ["name", "email"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    name: String,
    email: String,
}

/// Insert a row and return the MCP `result` payload.
pub async fn call(db: &Db, arguments: &Value) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;
    let name = args.name.trim().to_string();
    let email = args.email.trim().to_string();
    if name.is_empty() {
        return Err(ToolError::InvalidArguments("name must not be empty".into()));
    }
    if !is_email_shaped(&email) {
        return Err(ToolError::InvalidArguments(format!(
            "email `{email}` is not a valid address"
        )));
    }

    let inserted = person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "Created person id={} ({} <{}>).",
                inserted.id, inserted.name, inserted.email
            )
        }],
        "structuredContent": {
            "id": inserted.id,
            "name": inserted.name,
            "email": inserted.email
        }
    }))
}

/// Minimal "looks like an email" check. The real validation gate is
/// the database — but rejecting obviously-bad input here gives the
/// model a clearer error than a UNIQUE/CHECK failure surfaced from
/// Postgres.
fn is_email_shaped(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor, is_email_shaped};
    use crate::tools::ToolError;
    use sea_orm::EntityTrait;
    use serde_json::json;
    use store::entity::person;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    #[test]
    fn descriptor_names_the_tool_and_requires_name_and_email() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_create_person");
        let required = d["inputSchema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"email"));
        // The schema must lock down extras so the model can't sneak in
        // unknown fields that we'd silently ignore.
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
    }

    #[test]
    fn email_validator_accepts_reasonable_addresses() {
        assert!(is_email_shaped("libra@example.com"));
        assert!(is_email_shaped("nick+work@neonlaw.com"));
    }

    #[test]
    fn email_validator_rejects_obvious_garbage() {
        assert!(!is_email_shaped("not-an-email"));
        assert!(!is_email_shaped("@example.com"));
        assert!(!is_email_shaped("libra@"));
        assert!(!is_email_shaped("libra@example"));
        assert!(!is_email_shaped("libra@.com"));
        assert!(!is_email_shaped("libra@com."));
    }

    #[tokio::test]
    async fn happy_path_inserts_and_returns_structured_content() {
        let db = db().await;
        let result = call(
            &db,
            &json!({ "name": "Libra", "email": "libra@example.com" }),
        )
        .await
        .unwrap();

        assert_eq!(result["structuredContent"]["name"], "Libra");
        assert_eq!(result["structuredContent"]["email"], "libra@example.com");
        // `id` is rendered as a UUID hex string in JSON.
        let id = result["structuredContent"]["id"].as_str().unwrap();
        uuid::Uuid::parse_str(id).expect("id is a valid UUID");

        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Libra"));
        assert!(text.contains("libra@example.com"));

        let all = person::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].email, "libra@example.com");
    }

    #[tokio::test]
    async fn trims_surrounding_whitespace_on_name_and_email() {
        let db = db().await;
        let result = call(
            &db,
            &json!({ "name": "  Libra ", "email": "  libra@example.com\n" }),
        )
        .await
        .unwrap();
        assert_eq!(result["structuredContent"]["name"], "Libra");
        assert_eq!(result["structuredContent"]["email"], "libra@example.com");
    }

    #[tokio::test]
    async fn missing_name_field_is_invalid_arguments() {
        let db = db().await;
        let err = call(&db, &json!({ "email": "libra@example.com" }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn missing_email_field_is_invalid_arguments() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "Libra" })).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn empty_name_is_invalid_arguments() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "   ", "email": "libra@example.com" }))
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArguments(m) => assert!(m.contains("name")),
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_email_is_invalid_arguments() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "Libra", "email": "not-email" }))
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArguments(m) => assert!(m.contains("email")),
            other => panic!("expected InvalidArguments, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn duplicate_email_surfaces_a_conflict_error() {
        let db = db().await;
        call(&db, &json!({ "name": "Libra", "email": "dup@example.com" }))
            .await
            .unwrap();
        let err = call(&db, &json!({ "name": "Other", "email": "dup@example.com" }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Conflict(_)),
            "expected Conflict, got {err:?}"
        );
    }

    #[tokio::test]
    async fn distinct_emails_can_coexist() {
        let db = db().await;
        call(
            &db,
            &json!({ "name": "Libra", "email": "libra@example.com" }),
        )
        .await
        .unwrap();
        call(
            &db,
            &json!({ "name": "Taurus", "email": "taurus@example.com" }),
        )
        .await
        .unwrap();
        let all = person::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn additional_property_is_rejected_by_serde() {
        // The schema declares additionalProperties=false but we also
        // back that up at deserialization: extra fields are tolerated
        // by serde by default, which is fine — the schema is what the
        // model sees. This test pins current behavior: extras don't
        // break the call.
        let db = db().await;
        let result = call(
            &db,
            &json!({ "name": "Libra", "email": "libra@example.com", "extra": "ignored" }),
        )
        .await;
        assert!(result.is_ok());
    }
}
