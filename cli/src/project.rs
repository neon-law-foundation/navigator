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
use store::entity::{entity as entities, person, project};
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
    client_email: &str,
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
    // Both DRI columns are NOT NULL. The client side is the pre-existing
    // client this matter is opened for, resolved by `--client-email` and
    // required to be a `role = client` person (the client of record is a
    // client, never a firm attorney). The staff side defaults to the firm
    // principal (resolved by role).
    let client = person::Entity::find()
        .filter(person::Column::Email.eq(client_email))
        .one(db)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no person with email `{client_email}` — create the client first \
                 (`cli person create` / bulk import)"
            )
        })?;
    if client.role != person::Role::Client {
        anyhow::bail!(
            "the client DRI `{client_email}` must be a client person, not {}",
            client.role.as_str()
        );
    }
    let staff_dri = store::persons::default_firm_dri(db).await?.ok_or_else(|| {
        anyhow::anyhow!("no firm principal for the staff DRI — seed a staff/admin person first")
    })?;
    // Conflict check — runs before the matter is created, like the web and
    // MCP paths. The CLI is non-interactive, so **any** finding (block or
    // review) refuses the open; resolve it through the portal, where
    // authorized staff can review and acknowledge.
    let conflict = store::conflicts::check_new_matter(db, client.id, entity_id).await?;
    if !conflict.is_clear() {
        anyhow::bail!(
            "conflict check refused this matter — resolve it in the portal before opening:\n{}",
            conflict.summary_lines().join("\n")
        );
    }
    let inserted = project::ActiveModel {
        name: ActiveValue::Set(name.to_string()),
        status: ActiveValue::Set(status.to_string()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff_dri)),
        client_dri_person_id: ActiveValue::Set(Some(client.id)),
        ..Default::default()
    }
    .insert(db)
    .await?;

    // Eagerly stand up the matter's append-only git repo when the repo
    // volume is configured (`NAVIGATOR_GIT_REPO_ROOT`). Best-effort: a CLI
    // run against a Postgres with no attached repo volume (e.g. a remote
    // prod DB) skips, and `web` materializes the repo lazily on first
    // clone. `ensure` is idempotent, so a re-run is a no-op.
    if let Ok(repo_store) = repos::RepoStore::from_env() {
        match repo_store.ensure(inserted.id) {
            Ok(_) => {
                store::projects::mark_git_initialized(db, inserted.id, chrono::Utc::now()).await?;
            }
            Err(e) => tracing::warn!(
                project_id = %inserted.id,
                error = %e,
                "eager git repo init failed; lazy path will create it on first clone",
            ),
        }
    } else {
        tracing::info!(
            project_id = %inserted.id,
            "NAVIGATOR_GIT_REPO_ROOT unset; deferring git repo to lazy init on first clone",
        );
    }

    Ok(CreatedProject {
        id: inserted.id,
        name: inserted.name,
        status: inserted.status,
        entity_id: inserted.entity_id,
    })
}
