//! Drop `projects.drive_folder_id` — retire the per-Project Google Drive
//! sync surface.
//!
//! The per-Project archive is now the append-only git repo served from
//! `web` (`/projects/<id>.git`); Drive is no longer the address of
//! record, so the column has no remaining reader. (Google Drive was
//! since removed entirely — the `cloud::drive` client and the `cli drive`
//! login/ls commands are gone — but that came after this migration; this
//! step only drops the column.)
//!
//! Pre-live clean slate: dropping the column loses no production data.
//! The `down` re-adds it as a nullable text column (matching its
//! original shape from `m20260613_add_drive_folder_id_to_projects`) so
//! the migration is reversible.

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
                    .drop_column(Projects::DriveFolderId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .add_column(ColumnDef::new(Projects::DriveFolderId).string().null())
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
