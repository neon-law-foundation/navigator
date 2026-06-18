//! Drop the `drive_syncs` table.
//!
//! `drive_syncs` was the durable handle for the retired Drive→`documents`
//! sync orchestrator. The per-Project document system of record is now the
//! append-only git repository (`m20260627_add_git_repo_to_projects`,
//! [docs/git-project-repos.md](../../../docs/git-project-repos.md)), and the
//! `DriveSync` workflow that wrote these rows is gone. The table has no live
//! reader or writer left, so we drop it rather than carry a dead surface.
//!
//! Append-only migration history: the original create
//! (`m20260616_create_drive_syncs`) stays untouched; this migration reverses
//! it. `down` recreates the table so the step is reversible. The
//! `documents.source = "drive_sync"` provenance literal is **not** touched —
//! it records where historical document rows came from and stays true.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_drive_syncs_project_id")
                    .table(DriveSyncs::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(DriveSyncs::Table).to_owned())
            .await
    }

    #[allow(clippy::too_many_lines)]
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DriveSyncs::Table)
                    .if_not_exists()
                    .comment(
                        "Drive sync — one inbound pull of the Project's bound Drive folder. \
                         Retired surface; recreated here only so this migration is reversible.",
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DriveSyncs::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(DriveSyncs::DriveId).string().not_null())
                    .col(ColumnDef::new(DriveSyncs::FolderId).string().not_null())
                    .col(
                        ColumnDef::new(DriveSyncs::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Succeeded)
                            .big_integer()
                            .not_null()
                            .default(0_i64),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Failed)
                            .big_integer()
                            .not_null()
                            .default(0_i64),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Skipped)
                            .big_integer()
                            .not_null()
                            .default(0_i64),
                    )
                    .col(ColumnDef::new(DriveSyncs::ErrorMessage).text().null())
                    .col(ColumnDef::new(DriveSyncs::StartedAt).string().not_null())
                    .col(ColumnDef::new(DriveSyncs::FinishedAt).string().null())
                    .col(ColumnDef::new(DriveSyncs::InsertedAt).string().not_null())
                    .col(ColumnDef::new(DriveSyncs::UpdatedAt).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_drive_syncs_project")
                            .from(DriveSyncs::Table, DriveSyncs::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_drive_syncs_project_id")
                    .table(DriveSyncs::Table)
                    .col(DriveSyncs::ProjectId)
                    .col(DriveSyncs::Id)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DriveSyncs {
    Table,
    Id,
    ProjectId,
    DriveId,
    FolderId,
    Status,
    Succeeded,
    Failed,
    Skipped,
    ErrorMessage,
    StartedAt,
    FinishedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
