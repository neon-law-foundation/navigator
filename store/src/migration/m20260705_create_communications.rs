//! `communications` — the spine of the project-scoped, attorney-client
//! privileged conversation log.
//!
//! One privileged conversation per matter, no matter which door a message
//! came through: a client comment on a draft, an inbound email, the firm's
//! reply, an internal note — and, in the near future, a text message.
//!
//! This is **spine + satellites**, not single-table inheritance. The spine
//! carries the fields every channel shares (project, channel discriminator,
//! direction, author/counterparty, subject, body, source ref for dedup, a
//! raw-payload blob, and when it occurred). Channel-specific fidelity lives
//! in satellites FK'd back to the spine — the existing `document_comments`
//! table is the comment satellite and gains a `communication_id` FK here;
//! `email_conversation_message` becomes the email satellite when that
//! channel is back-filled. Adding `sms_inbound`/`sms_outbound` later is a
//! new `channel` literal (+ optional satellite) with no change to this
//! table.
//!
//! Privilege is a structural invariant enforced in the access layer, not a
//! column: every row is project-scoped client communication, so there is no
//! non-privileged row to flag. `direction = internal` rows are firm-only.

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
                    .table(Communications::Table)
                    .if_not_exists()
                    .comment(
                        "Communication — one message in a matter's single privileged \
                         conversation log, regardless of channel (document comment, inbound \
                         or outbound email, portal message, future SMS). The spine of a \
                         spine+satellites model; channel-specific detail lives in satellites \
                         (document_comments, email_conversation_message). Attorney-client \
                         privileged — read access is project-scoped, never firm-wide.",
                    )
                    .col(
                        ColumnDef::new(Communications::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Communication (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(Communications::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`) — the matter this message belongs to. The spine."),
                    )
                    .col(ColumnDef::new(Communications::Channel).string().not_null().comment(
                        "Channel discriminator: `document_comment`, `email_inbound`, \
                         `email_outbound`, `portal_message`, and (near-future) `sms_inbound` / \
                         `sms_outbound`.",
                    ))
                    .col(ColumnDef::new(Communications::Direction).string().not_null().comment(
                        "`inbound` (from the client), `outbound` (to the client), or `internal` \
                         (firm note — never shown to the client).",
                    ))
                    .col(
                        ColumnDef::new(Communications::AuthorPersonId)
                            .uuid()
                            .null()
                            .comment("FK → Person (`persons.id`) — who authored it; null for system / unknown sender."),
                    )
                    .col(ColumnDef::new(Communications::Counterparty).string().null().comment(
                        "Email address or name of the other party when there is no Person row \
                         (e.g. an inbound email from an address we don't yet know).",
                    ))
                    .col(
                        ColumnDef::new(Communications::Subject)
                            .string()
                            .null()
                            .comment("Optional subject line (email subject; null for comments)."),
                    )
                    .col(
                        ColumnDef::new(Communications::Body)
                            .text()
                            .not_null()
                            .comment("Normalized message text (the comment body, the email's text part, …)."),
                    )
                    .col(ColumnDef::new(Communications::SourceRef).string().null().comment(
                        "External id for idempotent ingest / dedup: the email `Message-ID`, the \
                         comment id, the SMS provider id. Unique per channel when present.",
                    ))
                    .col(
                        ColumnDef::new(Communications::BlobId)
                            .uuid()
                            .null()
                            .comment("FK → Blob (`blobs.id`) — raw payload (verbatim `.eml`, …); null for comments."),
                    )
                    .col(
                        ColumnDef::new(Communications::OccurredAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of when the message actually happened (sent/received/posted)."),
                    )
                    .col(
                        ColumnDef::new(Communications::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Communications::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_communications_project")
                            .from(Communications::Table, Communications::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_communications_author")
                            .from(Communications::Table, Communications::AuthorPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_communications_blob")
                            .from(Communications::Table, Communications::BlobId)
                            .to(Blobs::Table, Blobs::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // The conversation thread reads by project, oldest→newest.
        manager
            .create_index(
                Index::create()
                    .name("idx_communications_project_occurred")
                    .table(Communications::Table)
                    .col(Communications::ProjectId)
                    .col(Communications::OccurredAt)
                    .to_owned(),
            )
            .await?;

        // Idempotent ingest: a re-delivered email / re-ingested source can't
        // duplicate. NULL source_ref is allowed (Postgres treats NULLs as
        // distinct), so channels without an external id are unrestricted.
        manager
            .create_index(
                Index::create()
                    .name("uq_communications_channel_source_ref")
                    .table(Communications::Table)
                    .col(Communications::Channel)
                    .col(Communications::SourceRef)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // The comment satellite links back to its spine row. Nullable so the
        // shipped Phase A surface keeps working until the review POST routes
        // through the spine.
        manager
            .alter_table(
                Table::alter()
                    .table(DocumentComments::Table)
                    .add_column(
                        ColumnDef::new(DocumentComments::CommunicationId)
                            .uuid()
                            .null()
                            .comment("FK → Communication (`communications.id`) — the spine row for this comment."),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_document_comments_communication")
                    .from(DocumentComments::Table, DocumentComments::CommunicationId)
                    .to(Communications::Table, Communications::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_document_comments_communication")
                    .table(DocumentComments::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(DocumentComments::Table)
                    .drop_column(DocumentComments::CommunicationId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("uq_communications_channel_source_ref")
                    .table(Communications::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_communications_project_occurred")
                    .table(Communications::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Communications::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Communications {
    Table,
    Id,
    ProjectId,
    Channel,
    Direction,
    AuthorPersonId,
    Counterparty,
    Subject,
    Body,
    SourceRef,
    BlobId,
    OccurredAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DocumentComments {
    Table,
    CommunicationId,
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

#[derive(DeriveIden)]
enum Blobs {
    Table,
    Id,
}
