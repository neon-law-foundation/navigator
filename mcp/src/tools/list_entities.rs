//! `aida_list_entities` MCP tool.
//!
//! Returns every row in the `entities` table with its resolved
//! `entity_type` and jurisdiction names — enough for the model to
//! pick an `entity_id` when calling `aida_create_project`. The
//! entity set is bounded (firms, trusts, foundations a single law
//! practice manages) so we don't paginate. Sorted by `name`.

use sea_orm::{EntityTrait, QueryOrder};
use serde_json::{json, Value};
use store::entity::{entity as ent, entity_type, jurisdiction};
use store::Db;

use super::ToolError;

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_list_entities",
        "description": "List every legal Entity Neon Law Navigator knows about (LLCs, trusts, \
                        corporations, foundations, etc.), returning id, name, entity_type, \
                        and jurisdiction. Use this when a user wants to bind a Project to \
                        an existing Entity but only knows the name (e.g. \"the Shook family \
                        trust\"). Takes no arguments.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

pub async fn call(db: &Db, _arguments: &Value) -> Result<Value, ToolError> {
    let rows = ent::Entity::find()
        .order_by_asc(ent::Column::Name)
        .all(db)
        .await?;
    let types = entity_type::Entity::find().all(db).await?;
    let jurs = jurisdiction::Entity::find().all(db).await?;

    let by_type = |id: uuid::Uuid| {
        types
            .iter()
            .find(|t| t.id == id)
            .map_or("(unknown)", |t| t.name.as_str())
    };
    let by_jur = |id: uuid::Uuid| {
        jurs.iter()
            .find(|j| j.id == id)
            .map_or("(unknown)", |j| j.name.as_str())
    };

    let entities: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "name": row.name,
                "entity_type": by_type(row.entity_type_id),
                "jurisdiction": by_jur(row.jurisdiction_id),
            })
        })
        .collect();

    let summary = if rows.is_empty() {
        "No entities in the database.".to_string()
    } else {
        let listed = rows
            .iter()
            .map(|r| {
                format!(
                    "{} ({}, {})",
                    r.name,
                    by_type(r.entity_type_id),
                    by_jur(r.jurisdiction_id)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} entities: {listed}.", rows.len())
    };

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "count": entities.len(),
            "entities": entities,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use serde_json::json;
    use store::entity::{entity as ent, entity_type, jurisdiction};
    use uuid::Uuid;

    async fn db() -> store::Db {
        let db = store::test_support::pg().await;
        db
    }

    async fn seed_jurisdiction(db: &store::Db, name: &str, code: &str) -> Uuid {
        jurisdiction::ActiveModel {
            name: ActiveValue::Set(name.into()),
            code: ActiveValue::Set(code.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed_entity_type(db: &store::Db, name: &str) -> Uuid {
        entity_type::ActiveModel {
            name: ActiveValue::Set(name.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn seed(db: &store::Db, name: &str, et_id: Uuid, jur_id: Uuid) -> Uuid {
        ent::ActiveModel {
            name: ActiveValue::Set(name.into()),
            entity_type_id: ActiveValue::Set(et_id),
            jurisdiction_id: ActiveValue::Set(jur_id),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[test]
    fn descriptor_names_the_tool_and_takes_no_arguments() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_list_entities");
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
        let props = d["inputSchema"]["properties"].as_object().unwrap();
        assert!(props.is_empty());
    }

    #[tokio::test]
    async fn empty_database_returns_zero_count_not_an_error() {
        let db = db().await;
        let r = call(&db, &json!({})).await.unwrap();
        assert_eq!(r["structuredContent"]["count"], 0);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("No entities"));
    }

    #[tokio::test]
    async fn returns_seeded_entities_sorted_by_name() {
        let db = db().await;
        let jur = seed_jurisdiction(&db, "Nevada", "NV").await;
        let llc = seed_entity_type(&db, "Multi Member LLC").await;
        let trust = seed_entity_type(&db, "Family Trust").await;
        seed(&db, "Zeta Holdings", llc, jur).await;
        seed(&db, "Alpha Trust", trust, jur).await;
        let r = call(&db, &json!({})).await.unwrap();
        let names: Vec<&str> = r["structuredContent"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["Alpha Trust", "Zeta Holdings"]);
    }

    #[tokio::test]
    async fn each_row_carries_id_name_type_and_jurisdiction() {
        let db = db().await;
        let jur = seed_jurisdiction(&db, "Nevada", "NV").await;
        let trust = seed_entity_type(&db, "Family Trust").await;
        seed(&db, "shook.family", trust, jur).await;
        let r = call(&db, &json!({})).await.unwrap();
        let row = &r["structuredContent"]["entities"][0];
        assert_eq!(row["name"], "shook.family");
        assert_eq!(row["entity_type"], "Family Trust");
        assert_eq!(row["jurisdiction"], "Nevada");
        let id = row["id"].as_str().unwrap();
        uuid::Uuid::parse_str(id).expect("valid UUID");
    }

    #[tokio::test]
    async fn summary_lists_each_entity_with_type_and_jurisdiction() {
        let db = db().await;
        let jur = seed_jurisdiction(&db, "Nevada", "NV").await;
        let trust = seed_entity_type(&db, "Family Trust").await;
        seed(&db, "shook.family", trust, jur).await;
        let r = call(&db, &json!({})).await.unwrap();
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("1 entities:"));
        assert!(text.contains("shook.family (Family Trust, Nevada)"));
    }

    #[tokio::test]
    async fn ignores_arguments_silently() {
        let db = db().await;
        let jur = seed_jurisdiction(&db, "Nevada", "NV").await;
        let trust = seed_entity_type(&db, "Family Trust").await;
        seed(&db, "shook.family", trust, jur).await;
        let r = call(&db, &json!({ "garbage": 42 })).await.unwrap();
        assert_eq!(r["structuredContent"]["count"], 1);
    }
}
