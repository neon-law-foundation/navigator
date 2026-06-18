//! `drive_syncs` — one Drive→`documents` pull operation per Project.
//! Historical table; the orchestrator that drove it has been retired
//! (the per-Project archive is now the append-only git repo) and the
//! table itself was dropped in `m20260718_drop_drive_syncs`. Kept here
//! as append-only migration history.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DriveSyncs::Table)
                    .if_not_exists()
                    .comment(
                        "Drive sync — one inbound pull of the Project's bound Drive folder. \
                         Status row a polling client reads; crash-safe because the \
                         planner skips files already represented in documents + \
                         project_ingestions.",
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 sync id (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`)."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::DriveId)
                            .string()
                            .not_null()
                            .comment("Drive id (shared-drive root) the sync targets."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::FolderId)
                            .string()
                            .not_null()
                            .comment(
                                "Folder id whose immediate children get synced. \
                                 Sub-folders are skipped by today's planner.",
                            ),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Status)
                            .string()
                            .not_null()
                            .default("pending")
                            .comment("`pending`, `running`, `succeeded`, `failed`."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Succeeded)
                            .big_integer()
                            .not_null()
                            .default(0_i64)
                            .comment("Per-file ingest succeeded."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Failed)
                            .big_integer()
                            .not_null()
                            .default(0_i64)
                            .comment("Per-file ingest errored; orchestrator continued."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::Skipped)
                            .big_integer()
                            .not_null()
                            .default(0_i64)
                            .comment(
                                "Files the planner skipped — folders, known SHAs, \
                                 known revisions, unsupported Google-native types.",
                            ),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::ErrorMessage)
                            .text()
                            .null()
                            .comment(
                                "Orchestrator-level failure reason (folder list 4xx, \
                                 auth dead, etc). None for per-file failures.",
                            ),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::StartedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was created."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::FinishedAt)
                            .string()
                            .null()
                            .comment("RFC 3339 timestamp the orchestrator reached terminal state."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(DriveSyncs::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
