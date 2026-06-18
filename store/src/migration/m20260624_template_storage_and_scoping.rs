//! Move Template bodies to blob storage and add Project scoping.
//!
//! Two changes from `docs/notation.md`'s planned design:
//!
//! - **Body → blob.** Drop the inline `templates.body` TEXT column; add
//!   `templates.blob_id` referencing a [Blob]. The markdown bytes move
//!   through `cloud::StorageService` like every other artifact. A
//!   migration cannot reach object storage, so the bytes are
//!   (re)ingested by the seed / `navigator import` paths, which set
//!   `blob_id`; a fresh cluster carries the full catalog after seeding.
//!
//! - **Project scoping.** Add `templates.project_id: Option<Uuid>`.
//!   `NULL` = workspace-shared (the public catalog); a non-null value
//!   scopes the Template to one Project. The single `UNIQUE(code)` is
//!   replaced by two partial unique indexes so shared codes stay
//!   globally unique while each Project can reuse short codes:
//!     - `UNIQUE (code) WHERE project_id IS NULL`
//!     - `UNIQUE (project_id, code) WHERE project_id IS NOT NULL`
//!
//! [Blob]: ../../../docs/glossary.md#blob

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // Project scope + blob reference.
        db.execute_unprepared(
            "ALTER TABLE templates ADD COLUMN project_id uuid NULL REFERENCES projects(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE templates ADD COLUMN blob_id uuid NULL REFERENCES blobs(id)",
        )
        .await?;
        // Replace the single UNIQUE(code) with two partial unique
        // indexes (the create migration used an inline `.unique_key()`,
        // named `templates_code_key` by Postgres).
        db.execute_unprepared("ALTER TABLE templates DROP CONSTRAINT templates_code_key")
            .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX uq_templates_shared_code ON templates (code) \
             WHERE project_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX uq_templates_project_code ON templates (project_id, code) \
             WHERE project_id IS NOT NULL",
        )
        .await?;
        // The body now lives in a blob; drop the inline column.
        db.execute_unprepared("ALTER TABLE templates DROP COLUMN body")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE templates ADD COLUMN body text NOT NULL DEFAULT ''")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_project_code")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_shared_code")
            .await?;
        db.execute_unprepared(
            "ALTER TABLE templates ADD CONSTRAINT templates_code_key UNIQUE (code)",
        )
        .await?;
        db.execute_unprepared("ALTER TABLE templates DROP COLUMN blob_id")
            .await?;
        db.execute_unprepared("ALTER TABLE templates DROP COLUMN project_id")
            .await?;
        Ok(())
    }
}
