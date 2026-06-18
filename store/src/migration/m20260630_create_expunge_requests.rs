//! `expunge_requests` — a client's request to delete one of their
//! matter documents, awaiting attorney authorization.
//!
//! The governed-expunge primitive ([the design](../../../docs/git-project-repos.md)
//! §9) is admin-only — a client can never rewrite a matter's history
//! themselves. This table models the *request*: a client asks for a
//! document to be deleted (`status = pending`); a staff/admin reviews it
//! and either **authorizes** it — at which point the admin-gated expunge
//! runs and `expunge_record_id` links the resulting audit row — or
//! **denies** it. The category recorded on the executed expunge is
//! always `client_request`.

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
                    .table(ExpungeRequests::Table)
                    .if_not_exists()
                    .comment(
                        "ExpungeRequest — a client's request to delete a matter document, \
                         which a staff/admin authorizes (running the admin-gated expunge) or \
                         denies. See docs/git-project-repos.md §9.",
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this request (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`) — the matter the document belongs to."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::DocumentId)
                            .uuid()
                            .not_null()
                            .comment("FK → Document (`documents.id`) — the document to delete."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::RequestedByPersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — the client who requested deletion."),
                    )
                    .col(ColumnDef::new(ExpungeRequests::Status).string().not_null().comment(
                        "`pending` (awaiting review), `authorized` (expunge executed), or \
                         `denied`.",
                    ))
                    .col(
                        ColumnDef::new(ExpungeRequests::Note)
                            .text()
                            .comment("Optional non-content note from the client (their reason)."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::ResolvedByPersonId)
                            .uuid()
                            .comment("FK → Person (`persons.id`) — the staff/admin who resolved it. NULL while pending."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::ExpungeRecordId)
                            .uuid()
                            .comment("FK → ExpungeRecord (`expunge_records.id`) — the audit row from the executed expunge. NULL unless authorized."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(ExpungeRequests::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_requests_project")
                            .from(ExpungeRequests::Table, ExpungeRequests::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_requests_document")
                            .from(ExpungeRequests::Table, ExpungeRequests::DocumentId)
                            .to(Documents::Table, Documents::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_requests_requested_by")
                            .from(ExpungeRequests::Table, ExpungeRequests::RequestedByPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_requests_resolved_by")
                            .from(ExpungeRequests::Table, ExpungeRequests::ResolvedByPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_expunge_requests_expunge_record")
                            .from(ExpungeRequests::Table, ExpungeRequests::ExpungeRecordId)
                            .to(ExpungeRecords::Table, ExpungeRecords::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_expunge_requests_project_status")
                    .table(ExpungeRequests::Table)
                    .col(ExpungeRequests::ProjectId)
                    .col(ExpungeRequests::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_expunge_requests_document")
                    .table(ExpungeRequests::Table)
                    .col(ExpungeRequests::DocumentId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_expunge_requests_document")
                    .table(ExpungeRequests::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_expunge_requests_project_status")
                    .table(ExpungeRequests::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ExpungeRequests::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ExpungeRequests {
    Table,
    Id,
    ProjectId,
    DocumentId,
    RequestedByPersonId,
    Status,
    Note,
    ResolvedByPersonId,
    ExpungeRecordId,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ExpungeRecords {
    Table,
    Id,
}
