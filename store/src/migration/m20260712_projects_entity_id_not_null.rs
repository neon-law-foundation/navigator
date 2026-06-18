//! Make `projects.entity_id` `NOT NULL` — a matter always tracks a
//! pre-existing entity (a legal organization, or a `Human` entity for a
//! solo natural person). The matter-open service validates the entity
//! exists before opening; this is the schema backstop.
//!
//! Clean slate, not a backfill: the operator's directive (pre-live) is to
//! **delete all existing projects** rather than guess an entity for legacy
//! rows. We `TRUNCATE projects CASCADE`, which drops every `projects` row
//! and every row that references it (notations, person_project_roles,
//! documents, communications, …), then tighten the column.
//!
//! This deletes **database rows only**. The append-only archives are
//! untouched: each Project's bare git repo lives on the
//! `NAVIGATOR_GIT_REPO_ROOT` volume, and the nightly Postgres→Parquet
//! snapshots live as immutable objects in GCS — neither is a database
//! row, so a SQL `TRUNCATE` cannot reach them. The matter history a
//! deleted project accrued stays readable in those archives by design.
//!
//! On a fresh dev/test database the `TRUNCATE` is a no-op (the tables are
//! empty until the seed/fixtures run, which is after migrations), so the
//! same migration is safe in every environment.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Clean slate (DB rows only; GCS + git archives persist). CASCADE
        // so the FK-referencing rows go with the projects.
        db.execute_unprepared("TRUNCATE TABLE projects CASCADE")
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .modify_column(ColumnDef::new(Projects::EntityId).uuid().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .modify_column(ColumnDef::new(Projects::EntityId).uuid().null())
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    EntityId,
}
