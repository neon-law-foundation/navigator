//! Freeze each Notation's askable set at creation.
//!
//! Runtime spec resolution used to route by the template's bare `code` to
//! the binary's compile-time bundled YAML — so a Notation opened against
//! one version of a questionnaire could be re-walked against whatever spec
//! the currently-deployed binary ships. `questionnaire_snapshot` captures
//! the exact traversal graph (states + transitions + prompts) at
//! `start_notation`; render/step/fill resolve against it, immune to later
//! template or binary changes.
//!
//! Nullable: Notations created before this column fall back to
//! re-resolving from the template at load time.

use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE notations ADD COLUMN questionnaire_snapshot JSONB NULL",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE notations DROP COLUMN questionnaire_snapshot")
            .await?;
        Ok(())
    }
}
