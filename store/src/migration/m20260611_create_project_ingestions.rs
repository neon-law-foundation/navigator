//! `project_ingestions` — see glossary term
//! [Ingestion](../../../docs/glossary.md#ingestion).
//!
//! One inbound artifact landing on a Project — an email, a scanned
//! letter, an upload, a fax. The mapping between an Ingestion row
//! and the source channel's revision id is the matter's audit trail.
//!
//! The `commit_sha` column created here is renamed to
//! `source_revision_id` by `m20260617_rename_project_ingestion_commit_sha`;
//! this file is left as it shipped so the migration sequence replays
//! cleanly against any database.

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
                    .table(ProjectIngestions::Table)
                    .if_not_exists()
                    .comment(
                        "Project Ingestion — one inbound artifact landing on a Project. \
                         One Ingestion ↔ one upstream revision id (Drive headRevisionId, \
                         email Message-ID, etc.). See docs/glossary.md#ingestion.",
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Ingestion (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`)."),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::Source)
                            .string()
                            .not_null()
                            .comment("Inbound channel — `email`, `letter`, `upload`, `fax`, …"),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::Summary)
                            .string()
                            .null()
                            .comment(
                                "Human-readable one-line summary for the staff view \
                                 (e.g., `Letter from Acme Bank dated 2026-05-23`).",
                            ),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::Payload)
                            .text()
                            .null()
                            .comment(
                                "Opaque JSON payload — sender, headers, MIME parts, \
                                 anything the inbound channel knows that the staff view \
                                 might surface later.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::CommitSha)
                            .string()
                            .null()
                            .comment(
                                "Upstream artifact's revision id (Drive headRevisionId, \
                                 email Message-ID, fax sequence number, …). Null until \
                                 the ingestion_intake workflow finishes; immutable once \
                                 set. Renamed to source_revision_id in m20260617.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::ReceivedAt)
                            .string()
                            .not_null()
                            .comment(
                                "RFC 3339 / ISO 8601 timestamp when the Ingestion was \
                                 received by the inbound channel.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::InsertedAt)
                            .string()
                            .not_null()
                            .comment(
                                "RFC 3339 timestamp when this row was inserted. Set by \
                                 the application, not the database; workspace convention \
                                 is `inserted_at` + `updated_at` on every table.",
                            ),
                    )
                    .col(
                        ColumnDef::new(ProjectIngestions::UpdatedAt)
                            .string()
                            .not_null()
                            .comment(
                                "RFC 3339 timestamp of the last update; equals \
                                 inserted_at on insert.",
                            ),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_ingestions_project")
                            .from(ProjectIngestions::Table, ProjectIngestions::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_project_ingestions_project_id")
                    .table(ProjectIngestions::Table)
                    .col(ProjectIngestions::ProjectId)
                    .col(ProjectIngestions::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_project_ingestions_project_id")
                    .table(ProjectIngestions::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ProjectIngestions::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ProjectIngestions {
    Table,
    Id,
    ProjectId,
    Source,
    Summary,
    Payload,
    CommitSha,
    ReceivedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
