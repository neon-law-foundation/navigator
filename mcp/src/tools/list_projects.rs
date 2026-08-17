//! `aida_list_projects` MCP tool.
//!
//! Returns every row in the `projects` table with its resolved
//! Entity name (when bound) — enough for the model to pick a
//! `project_id` when calling tools like `aida_link_person_project`.
//! Sorted by `name`. No pagination: the matter set for a single
//! practice stays small enough to ship in one response.

use serde_json::{json, Value};

use super::ToolError;

#[must_use]
pub fn descriptor() -> Value {
    json!({
        "name": "aida_list_projects",
        "description": "List every Project (matter) Neon Law Navigator knows about, returning id, name, \
                        status, and the bound Entity's id and name when one is \
                        attached. Use this when a user asks to see open matters or wants to pick \
                        a Project by name (e.g. \"ShookEstate\") before linking a Person or \
                        attaching a Notation. Takes no arguments.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

pub async fn call(
    surreal: &store::surreal::SurrealDb,
    _arguments: &Value,
) -> Result<Value, ToolError> {
    let rows = store::projects::all(surreal)
        .await
        .map_err(|error| ToolError::Database(error.to_string()))?;
    let entities = store::entities::all(surreal).await?;

    let entity_name = |id: uuid::Uuid| {
        entities
            .iter()
            .find(|e| e.id == id)
            .map_or("(unknown)", |e| e.name.as_str())
    };

    let projects: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "name": row.name,
                "status": row.status,
                "entity_id": row.entity_id,
                "entity_name": entity_name(row.entity_id),
            })
        })
        .collect();

    let summary = if rows.is_empty() {
        "No projects in the database.".to_string()
    } else {
        let listed = rows
            .iter()
            .map(|r| format!("{} ({}, {})", r.name, r.status, entity_name(r.entity_id)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} projects: {listed}.", rows.len())
    };

    Ok(json!({
        "content": [{ "type": "text", "text": summary }],
        "structuredContent": {
            "count": projects.len(),
            "projects": projects,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::{call, descriptor};
    use serde_json::json;
    use uuid::Uuid;

    use store::test_support::mem_surreal;
    async fn db() -> store::surreal::SurrealDb {
        let surreal = mem_surreal().await;
        surreal
    }

    async fn seed_entity(surreal: &store::surreal::SurrealDb, name: &str) -> Uuid {
        // The engine does not validate a `record<>` link and nothing in
        // this tool resolves either reference, so the fixture points them
        // at rows it never writes.
        let jur_id = Uuid::now_v7();
        let et_id = Uuid::now_v7();
        store::entities::create(
            surreal,
            &store::entities::NewEntity {
                name: name.into(),
                entity_type_id: et_id,
                jurisdiction_id: jur_id,
                phone: None,
                url: None,
                firm_anchor_key: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn seed_project(
        surreal: &store::surreal::SurrealDb,
        name: &str,
        status: &str,
        entity_id: Option<Uuid>,
    ) -> Uuid {
        // projects.entity_id is NOT NULL: open against the given entity, or
        // a fresh throwaway one when the test doesn't care which.
        let entity_id = match entity_id {
            Some(e) => e,
            None => store::test_support::seed_entity(surreal).await,
        };
        store::projects::create(
            surreal,
            &store::projects::NewProject {
                code: format!("test-{}", Uuid::now_v7()),
                name: name.into(),
                status: status.into(),
                entity_id,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    #[test]
    fn descriptor_names_the_tool_and_takes_no_arguments() {
        let d = descriptor();
        assert_eq!(d["name"], "aida_list_projects");
        assert_eq!(d["inputSchema"]["additionalProperties"], false);
        let props = d["inputSchema"]["properties"].as_object().unwrap();
        assert!(props.is_empty());
    }

    #[tokio::test]
    async fn empty_database_returns_zero_count_not_an_error() {
        let surreal = db().await;
        let r = call(&surreal, &json!({})).await.unwrap();
        assert_eq!(r["structuredContent"]["count"], 0);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("No projects"));
    }

    #[tokio::test]
    async fn returns_seeded_projects_sorted_by_name() {
        let surreal = db().await;
        seed_project(&surreal, "Zeta Settlement", "open", None).await;
        seed_project(&surreal, "Alpha Matter", "open", None).await;
        let r = call(&surreal, &json!({})).await.unwrap();
        let names: Vec<&str> = r["structuredContent"]["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["Alpha Matter", "Zeta Settlement"]);
    }

    #[tokio::test]
    async fn row_carries_status_and_bound_entity_name() {
        let surreal = db().await;
        let eid = seed_entity(&surreal, "shook.family").await;
        seed_project(&surreal, "ShookEstate", "open", Some(eid)).await;
        let r = call(&surreal, &json!({})).await.unwrap();
        let row = &r["structuredContent"]["projects"][0];
        assert_eq!(row["name"], "ShookEstate");
        assert_eq!(row["status"], "open");
        assert_eq!(row["entity_id"], eid.to_string());
        assert_eq!(row["entity_name"], "shook.family");
    }

    #[tokio::test]
    async fn summary_lists_status_and_bound_entity() {
        let surreal = db().await;
        let eid = seed_entity(&surreal, "shook.family").await;
        seed_project(&surreal, "ShookEstate", "open", Some(eid)).await;
        seed_project(&surreal, "Sison", "closed", Some(eid)).await;
        let r = call(&surreal, &json!({})).await.unwrap();
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("2 projects:"));
        assert!(text.contains("ShookEstate (open, shook.family)"));
        assert!(text.contains("Sison (closed, shook.family)"));
    }

    #[tokio::test]
    async fn ignores_arguments_silently() {
        let surreal = db().await;
        seed_project(&surreal, "Sison", "open", None).await;
        let r = call(&surreal, &json!({ "garbage": 42 })).await.unwrap();
        assert_eq!(r["structuredContent"]["count"], 1);
    }
}
