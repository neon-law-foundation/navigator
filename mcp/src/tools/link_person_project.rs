//! `aida_link_person_project` MCP tool.
//!
//! Binds a Person to a Project with a named role. Glossary nouns
//! only — `client`, `attorney`, `paralegal`, `counterparty`. The
//! tool re-uses the `(person_id, project_id, role)` triple as the
//! deduplication key: calling twice with the same triple returns
//! the existing row, not an error, so the model can be idempotent
//! without having to remember.
//!
//! See [`Person–Project Role`].
//!
//! [`Person–Project Role`]: ../../../docs/glossary.md#personproject-role

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::{person, person_project_role, project};
use store::Db;
use uuid::Uuid;

use super::ToolError;

/// Glossary roles accepted by this tool. Locking the enum prevents
/// the model from inventing new role words on the fly — see the
/// `feedback-legal-workflows-feature-first` memory.
const ROLES: &[&str] = &["client", "attorney", "paralegal", "counterparty"];

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_link_person_project",
        "description": "Bind a Person to a Project with a role (client, attorney, paralegal, \
                        counterparty). Idempotent: re-linking the same (person, project, role) \
                        returns the existing row instead of erroring. Returns the link id.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "person_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "Existing persons.id."
                },
                "project_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "Existing projects.id."
                },
                "role": {
                    "type": "string",
                    "enum": ["client", "attorney", "paralegal", "counterparty"],
                    "description": "Glossary role — no free-text role words."
                }
            },
            "required": ["person_id", "project_id", "role"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    person_id: Uuid,
    project_id: Uuid,
    role: String,
}

pub async fn call(db: &Db, arguments: &Value) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let role = args.role.trim().to_string();
    if !ROLES.contains(&role.as_str()) {
        return Err(ToolError::InvalidArguments(format!(
            "role must be one of client/attorney/paralegal/counterparty, got `{role}`"
        )));
    }

    if person::Entity::find_by_id(args.person_id)
        .one(db)
        .await?
        .is_none()
    {
        return Err(ToolError::NotFound(format!("person_id={}", args.person_id)));
    }
    if project::Entity::find_by_id(args.project_id)
        .one(db)
        .await?
        .is_none()
    {
        return Err(ToolError::NotFound(format!(
            "project_id={}",
            args.project_id
        )));
    }

    let existing = person_project_role::Entity::find()
        .filter(person_project_role::Column::PersonId.eq(args.person_id))
        .filter(person_project_role::Column::ProjectId.eq(args.project_id))
        .filter(person_project_role::Column::Participation.eq(role.as_str()))
        .one(db)
        .await?;

    let (id, created) = if let Some(row) = existing {
        (row.id, false)
    } else {
        let inserted = person_project_role::ActiveModel {
            person_id: ActiveValue::Set(args.person_id),
            project_id: ActiveValue::Set(args.project_id),
            participation: ActiveValue::Set(role.clone()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        (inserted.id, true)
    };

    let verb = if created { "Linked" } else { "Already linked" };
    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "{verb} person={} → project={} as {role} (link id={id}).",
                args.person_id, args.project_id
            )
        }],
        "structuredContent": {
            "id": id,
            "person_id": args.person_id,
            "project_id": args.project_id,
            "role": role,
            "created": created,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use crate::tools::ToolError;
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use serde_json::json;
    use store::entity::{person, person_project_role, project};
    use uuid::Uuid;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    async fn seed_person_and_project(db: &store::Db) -> (Uuid, Uuid) {
        let p = person::ActiveModel {
            name: ActiveValue::Set("Jon Sison".into()),
            email: ActiveValue::Set("jon@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let proj = project::ActiveModel {
            name: ActiveValue::Set("Sison".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        (p.id, proj.id)
    }

    #[test]
    fn descriptor_locks_the_role_enum_and_requires_all_three_ids() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_link_person_project");
        let required = d["inputSchema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"person_id"));
        assert!(names.contains(&"project_id"));
        assert!(names.contains(&"role"));
        let roles = d["inputSchema"]["properties"]["role"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(roles.len(), 4);
    }

    #[tokio::test]
    async fn happy_path_creates_link() {
        let db = db().await;
        let (pid, projid) = seed_person_and_project(&db).await;
        let r = call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "client" }),
        )
        .await
        .unwrap();
        assert_eq!(r["structuredContent"]["role"], "client");
        assert_eq!(r["structuredContent"]["created"], true);
        let all = person_project_role::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn re_link_same_triple_is_idempotent() {
        let db = db().await;
        let (pid, projid) = seed_person_and_project(&db).await;
        let first = call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "client" }),
        )
        .await
        .unwrap();
        let second = call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "client" }),
        )
        .await
        .unwrap();
        assert_eq!(
            first["structuredContent"]["id"],
            second["structuredContent"]["id"]
        );
        assert_eq!(second["structuredContent"]["created"], false);
        let all = person_project_role::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn same_pair_with_different_role_creates_a_second_link() {
        let db = db().await;
        let (pid, projid) = seed_person_and_project(&db).await;
        call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "client" }),
        )
        .await
        .unwrap();
        call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "attorney" }),
        )
        .await
        .unwrap();
        let all = person_project_role::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn unknown_person_id_returns_not_found() {
        let db = db().await;
        let (_, projid) = seed_person_and_project(&db).await;
        let err = call(
            &db,
            &json!({ "person_id": Uuid::now_v7(), "project_id": projid, "role": "client" }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn unknown_project_id_returns_not_found() {
        let db = db().await;
        let (pid, _) = seed_person_and_project(&db).await;
        let err = call(
            &db,
            &json!({ "person_id": pid, "project_id": Uuid::now_v7(), "role": "client" }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn bogus_role_is_invalid_arguments() {
        let db = db().await;
        let (pid, projid) = seed_person_and_project(&db).await;
        let err = call(
            &db,
            &json!({ "person_id": pid, "project_id": projid, "role": "wizard" }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }
}
