//! `expunge_records` — the audit trail of governed expunges.
//!
//! A governed expunge (privilege clawback, sealing order, lawful
//! deletion) is the one operation that rewrites a matter repo's history
//! (see [the design](../../../docs/git-project-repos.md) §9). The legal
//! council's load-bearing requirement: the **expunge itself** is
//! recorded — *who* authorized it, *when*, and the *category* — but
//! **not the content** — so the redaction is auditable without
//! re-exposing what was removed.
//!
//! `head_before` / `head_after` capture the rewritten head oids, proving
//! a rewrite occurred. `path` is the repo path that was removed
//! (metadata, not content); access to this table is staff/admin-only.

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
                    .table(ExpungeRecords::Table)
                    .if_not_exists()
                    .comment(
                        "ExpungeRecord — one governed expunge of a matter repo. Records who \
                         authorized it, when, and the category (privilege / sealing / \
                         client_request), but never the content removed. See \
                         docs/git-project-repos.md §9.",
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this expunge record (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`) — the matter whose repo was rewritten."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::Path).string().not_null().comment(
                            "Repo path removed from all history. Metadata, not content; \
                             this table is staff/admin-only.",
                        ),
                    )
                    .col(ColumnDef::new(ExpungeRecords::Category).string().not_null().comment(
                        "`privilege` (clawback), `sealing` (court order), or \
                         `client_request` (lawful deletion).",
                    ))
                    .col(
                        ColumnDef::new(ExpungeRecords::AuthorizedByPersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — the admin who authorized the expunge."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::HeadBefore)
                            .string()
                            .comment("`refs/heads/main` oid before the rewrite (NULL if the repo was empty)."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::HeadAfter)
                            .string()
                            .comment("`refs/heads/main` oid after the rewrite."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::Note)
                            .text()
                            .comment("Optional non-content note (e.g. a sealing-order docket reference)."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRecords::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_records_project")
                            .from(ExpungeRecords::Table, ExpungeRecords::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_records_authorized_by")
                            .from(ExpungeRecords::Table, ExpungeRecords::AuthorizedByPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_expunge_records_project_id")
                    .table(ExpungeRecords::Table)
                    .col(ExpungeRecords::ProjectId)
                    .col(ExpungeRecords::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_expunge_records_project_id")
                    .table(ExpungeRecords::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ExpungeRecords::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ExpungeRecords {
    Table,
    Id,
    ProjectId,
    Path,
    Category,
    AuthorizedByPersonId,
    HeadBefore,
    HeadAfter,
    Note,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
