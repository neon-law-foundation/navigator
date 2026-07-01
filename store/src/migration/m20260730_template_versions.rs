//! Templates as versions — rows become immutable by policy.
//!
//! `notation.template_id` already pins the exact Template a Notation was
//! assembled from. Making a Template row immutable-by-policy — an edit
//! **INSERTs a new row** and flips a pointer rather than rewriting the
//! existing spec — delivers "changed template ⇒ new Notation" for free:
//! an in-flight Notation keeps resolving to the exact bytes it was opened
//! against, while new Notations pick up the current version. No new table,
//! no FK — just new rows in `templates`.
//!
//! `is_current` is that pointer. `code` stops being globally unique;
//! instead exactly one **current** row may exist per `code`
//! (workspace-shared) or per `(project_id, code)` (project-scoped).
//! Retired versions share the code freely.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE templates ADD COLUMN is_current BOOLEAN NOT NULL DEFAULT true",
        )
        .await?;
        // Uniqueness now applies only to the *current* version, so two
        // versions of one code coexist as long as one is current.
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_shared_code")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_project_code")
            .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX uq_templates_current_shared_code ON templates (code) \
             WHERE is_current AND project_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX uq_templates_current_project_code \
             ON templates (project_id, code) \
             WHERE is_current AND project_id IS NOT NULL",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_current_project_code")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_templates_current_shared_code")
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
        db.execute_unprepared("ALTER TABLE templates DROP COLUMN is_current")
            .await?;
        Ok(())
    }
}
