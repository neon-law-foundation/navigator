//! Add `drive_folder_id` to `projects` — the per-Project archive
//! address in the NeonLaw Google shared drive. See glossary term
//! [Project](../../../docs/glossary.md#project).
//!
//! Load-bearing invariant: every Project corresponds to exactly
//! one folder in the configured shared drive (the drive id is a
//! deployment secret read from env / Secret Manager, not embedded
//! here). The column is nullable for now so the canonical seed's
//! `navigator-examples` Project (which has no Drive folder) still
//! inserts; it will tighten to NOT NULL once the seed row is
//! backfilled or removed.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .add_column(ColumnDef::new(Projects::DriveFolderId).string().comment(
                        "Google Drive folder id in the NeonLaw shared drive — \
                             the per-Project archive address.",
                    ))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(Projects::DriveFolderId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    DriveFolderId,
}
