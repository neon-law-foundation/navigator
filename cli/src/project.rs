//! `project` subcommand: write-side primitives for the `projects`
//! table.
//!
//! Today the only operation is `create`, which inserts one row with
//! a required Entity link. The caller is expected to have already run
//! migrate + seed against the target Postgres so the Entity it names
//! actually exists.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use store::entity::{entity as entities, project};
use uuid::Uuid;

/// Outcome of a `project create` run, returned so tests can assert
/// on the inserted row without re-querying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedProject {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub entity_id: Uuid,
}

/// Insert a new `projects` row. `entity_name`, when supplied, must
/// match an existing row in `entities` by exact `name` — the lookup
/// is strict so callers can't silently drop the link by misspelling.
pub async fn create(
    db: &DatabaseConnection,
    name: &str,
    entity_name: Option<&str>,
    status: &str,
) -> anyhow::Result<CreatedProject> {
    // A matter always opens against a pre-existing entity
    // (`projects.entity_id` is NOT NULL). Require `--entity-name` and
    // resolve it strictly.
    let needle = entity_name.ok_or_else(|| {
        anyhow::anyhow!("an entity is required — pass --entity-name (create the entity first)")
    })?;
    let entity_id = entities::Entity::find()
        .filter(entities::Column::Name.eq(needle))
        .one(db)
        .await?
        .map(|e| e.id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no entity named `{needle}` — run `cli list entities` to see what's seeded"
            )
        })?;
    let inserted = project::ActiveModel {
        name: ActiveValue::Set(name.to_string()),
        status: ActiveValue::Set(status.to_string()),
        entity_id: ActiveValue::Set(entity_id),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(CreatedProject {
        id: inserted.id,
        name: inserted.name,
        status: inserted.status,
        entity_id: inserted.entity_id,
    })
}
