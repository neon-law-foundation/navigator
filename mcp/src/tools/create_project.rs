//! `aida_create_project` MCP tool.
//!
//! Opens a new Project (a [Matter] in client English) without
//! attaching a Notation yet. Use this when onboarding a matter
//! whose Template doesn't exist in Neon Law Navigator (a one-off settlement,
//! a custom expungement petition, an entity-management container) —
//! the Project is the durable home for Persons and Documents;
//! Notations attach later as Templates ship.
//!
//! [Matter]: ../../../docs/glossary.md#matter

use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use serde::Deserialize;
use serde_json::{json, Value};
use store::entity::{entity as ent, person, project};
use store::Db;
use uuid::Uuid;

use super::ToolError;

const STATUSES: &[&str] = &["open", "closed", "archived"];

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_create_project",
        "description": "Open a new Project (matter) in Neon Law Navigator. A matter always opens against a \
                        pre-existing Entity — pass its uuid as `entity_id` (create the Entity \
                        first if needed). Returns the new id, name, status, and entity_id so the \
                        caller can reference the row.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable matter name (e.g. \"Sison — mutual release\", \"ShookEstate\")."
                },
                "status": {
                    "type": "string",
                    "enum": ["open", "closed", "archived"],
                    "description": "Lifecycle state. Defaults to `open`."
                },
                "entity_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "Uuid of the pre-existing Entity this matter opens against (the \
                                    LLC, trust, estate, or a `Human` entity for a solo person)."
                },
                "client_dri_person_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "Uuid of the pre-existing client Person this matter is opened \
                                    for — its client-side Directly Responsible Individual. Must be \
                                    an existing person with role `client` (create the client \
                                    first). The matter's client of record is a client, never a \
                                    firm attorney."
                }
            },
            "required": ["name", "entity_id", "client_dri_person_id"],
            "additionalProperties": false
        }
    })
}

#[derive(Debug, Deserialize)]
struct Args {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    entity_id: Option<Uuid>,
    #[serde(default)]
    client_dri_person_id: Option<Uuid>,
}

pub async fn call(db: &Db, arguments: &Value) -> Result<Value, ToolError> {
    let args: Args = super::decode_args(arguments)?;

    let name = args.name.trim().to_string();
    if name.is_empty() {
        return Err(ToolError::InvalidArguments("name must not be empty".into()));
    }

    let status = args
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("open")
        .to_string();
    if !STATUSES.contains(&status.as_str()) {
        return Err(ToolError::InvalidArguments(format!(
            "status must be one of open/closed/archived, got `{status}`"
        )));
    }

    // A matter always opens against a pre-existing entity (projects.entity_id
    // is NOT NULL). Require it and validate it exists before opening.
    let entity_id = args.entity_id.ok_or_else(|| {
        ToolError::InvalidArguments(
            "entity_id is required — open the matter against an existing entity".into(),
        )
    })?;
    if ent::Entity::find_by_id(entity_id).one(db).await?.is_none() {
        return Err(ToolError::NotFound(format!("entity_id={entity_id}")));
    }

    // Both DRI columns are NOT NULL. The client side is the pre-existing
    // client this matter is opened for — required, and a real `Role::Client`
    // person (the client of record is a client, never a firm attorney). The
    // staff side defaults to the firm principal (resolved by role).
    let client_dri_id = args.client_dri_person_id.ok_or_else(|| {
        ToolError::InvalidArguments(
            "client_dri_person_id is required — open the matter for an existing client".into(),
        )
    })?;
    let client = person::Entity::find_by_id(client_dri_id)
        .one(db)
        .await?
        .ok_or_else(|| ToolError::NotFound(format!("client_dri_person_id={client_dri_id}")))?;
    if client.role != store::entity::person::Role::Client {
        return Err(ToolError::InvalidArguments(format!(
            "the client DRI must be a client person, not {}",
            client.role.as_str()
        )));
    }
    let staff_dri = store::persons::default_firm_dri(db).await?.ok_or_else(|| {
        ToolError::InvalidArguments(
            "no firm principal to assign as staff DRI — seed a staff/admin person first".into(),
        )
    })?;

    let inserted = project::ActiveModel {
        name: ActiveValue::Set(name),
        status: ActiveValue::Set(status),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff_dri)),
        client_dri_person_id: ActiveValue::Set(Some(client.id)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let summary = format!(
        "Created project id={} ({}, status={}, entity_id={}).",
        inserted.id, inserted.name, inserted.status, inserted.entity_id
    );

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "id": inserted.id,
            "name": inserted.name,
            "status": inserted.status,
            "entity_id": inserted.entity_id,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use crate::tools::ToolError;
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use serde_json::json;
    use store::entity::{entity as ent, entity_type, jurisdiction, person, project};
    use uuid::Uuid;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    /// Seed a `Role::Client` person and return its id — the matter's
    /// client of record, required as `client_dri_person_id`.
    async fn seed_client(db: &store::Db) -> Uuid {
        person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            role: ActiveValue::Set(person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    /// Seed a `Role::Admin` person so `default_firm_dri` can resolve a
    /// staff-side DRI for the project.
    async fn seed_firm_principal(db: &store::Db) {
        person::ActiveModel {
            name: ActiveValue::Set("Firm Principal".into()),
            email: ActiveValue::Set("principal@example.com".into()),
            role: ActiveValue::Set(person::Role::Admin),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn seed_entity(db: &store::Db) -> Uuid {
        let jur = jurisdiction::ActiveModel {
            name: ActiveValue::Set("Nevada".into()),
            code: ActiveValue::Set("NV".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let et = entity_type::ActiveModel {
            name: ActiveValue::Set("Family Trust".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let e = ent::ActiveModel {
            name: ActiveValue::Set("shook.family".into()),
            entity_type_id: ActiveValue::Set(et.id),
            jurisdiction_id: ActiveValue::Set(jur.id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        e.id
    }

    #[test]
    fn descriptor_names_the_tool_and_requires_name_and_entity() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_create_project");
        let required = d["inputSchema"]["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(names, vec!["name", "entity_id", "client_dri_person_id"]);
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
    }

    #[tokio::test]
    async fn happy_path_inserts_with_defaults() {
        let db = db().await;
        let eid = seed_entity(&db).await;
        let cid = seed_client(&db).await;
        seed_firm_principal(&db).await;
        let r = call(
            &db,
            &json!({ "name": "Sison", "entity_id": eid, "client_dri_person_id": cid }),
        )
        .await
        .unwrap();
        assert_eq!(r["structuredContent"]["name"], "Sison");
        assert_eq!(r["structuredContent"]["status"], "open");
        assert_eq!(r["structuredContent"]["entity_id"], eid.to_string());
        let all = project::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn binds_entity_when_provided_and_exists() {
        let db = db().await;
        let eid = seed_entity(&db).await;
        let cid = seed_client(&db).await;
        seed_firm_principal(&db).await;
        let r = call(
            &db,
            &json!({ "name": "ShookEstate", "entity_id": eid, "client_dri_person_id": cid }),
        )
        .await
        .unwrap();
        assert_eq!(r["structuredContent"]["entity_id"], eid.to_string());
    }

    #[tokio::test]
    async fn unknown_entity_id_returns_not_found() {
        let db = db().await;
        let missing = Uuid::now_v7();
        let err = call(&db, &json!({ "name": "X", "entity_id": missing }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn empty_name_is_invalid() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "   " })).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn bad_status_is_invalid() {
        let db = db().await;
        let err = call(&db, &json!({ "name": "Sison", "status": "pending" }))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments(_)));
    }
}
