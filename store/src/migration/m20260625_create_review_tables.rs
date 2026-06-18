//! `review_documents` + `document_comments` — the comment-only client
//! review surface (Northstar Phase A).
//!
//! A matter (notation) can produce several documents a client must read
//! before signing — an estate plan is a will, a trust, and health and
//! financial directives. Each lands as one `review_documents` row
//! holding the attorney-reviewed draft as HTML (the review viewer
//! renders HTML, not the signing PDF). A row is visible to the client
//! only once its
//! `status` reaches `pending_review`; the generation workflow parks it at
//! `draft` until an attorney approves it — no client-facing
//! auto-generated legal document without a human in the loop.
//!
//! `document_comments` holds the client's read-only feedback: a text
//! range (`anchor_start`/`anchor_end`, character offsets into the
//! document text) plus the
//! `quoted_text` it covered, the comment `body`, and a `resolved` flag
//! staff flip when they've addressed it. Comments anchor to a specific
//! `review_documents` row, never to the bare notation, so the will's
//! thread and the trust's thread stay separate.

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
                    .table(ReviewDocuments::Table)
                    .if_not_exists()
                    .comment(
                        "ReviewDocument — one attorney-reviewed draft a client reads (and \
                         comments on) before signing. HTML body, one row per document in a \
                         matter (will, trust, directive). See Northstar Phase A.",
                    )
                    .col(
                        ColumnDef::new(ReviewDocuments::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this ReviewDocument (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(ReviewDocuments::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`) — the matter this draft belongs to."),
                    )
                    .col(ColumnDef::new(ReviewDocuments::Kind).string().not_null().comment(
                        "Document kind within the matter: `will`, `trust`, \
                         `directive_health`, `directive_financial`, …",
                    ))
                    .col(
                        ColumnDef::new(ReviewDocuments::Title)
                            .string()
                            .not_null()
                            .comment("Human-readable title shown to the client (e.g. `Last Will and Testament`)."),
                    )
                    .col(
                        ColumnDef::new(ReviewDocuments::BodyHtml)
                            .text()
                            .not_null()
                            .comment("Attorney-reviewed draft body as sanitized HTML; TipTap renders it read-only."),
                    )
                    .col(ColumnDef::new(ReviewDocuments::Status).string().not_null().comment(
                        "`draft` (not yet attorney-approved, hidden from client), \
                         `pending_review` (client may read + comment), or `approved` \
                         (client signed off; ready for signature).",
                    ))
                    .col(
                        ColumnDef::new(ReviewDocuments::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(ReviewDocuments::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_review_documents_notation")
                            .from(ReviewDocuments::Table, ReviewDocuments::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_review_documents_notation_id")
                    .table(ReviewDocuments::Table)
                    .col(ReviewDocuments::NotationId)
                    .col(ReviewDocuments::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DocumentComments::Table)
                    .if_not_exists()
                    .comment(
                        "DocumentComment — one client (or staff) comment anchored to a text \
                         range within a ReviewDocument. The review surface is read-only; \
                         comments are the only thing a client writes.",
                    )
                    .col(
                        ColumnDef::new(DocumentComments::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this DocumentComment (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::ReviewDocumentId)
                            .uuid()
                            .not_null()
                            .comment("FK → ReviewDocument (`review_documents.id`) — the draft commented on."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — who wrote the comment."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::AnchorStart)
                            .integer()
                            .not_null()
                            .comment("Start character offset (into the document text) of the commented range."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::AnchorEnd)
                            .integer()
                            .not_null()
                            .comment("End character offset (into the document text) of the commented range."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::QuotedText)
                            .text()
                            .not_null()
                            .comment("The text the range covered when the comment was made (display + resilience)."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::Body)
                            .text()
                            .not_null()
                            .comment("The comment text the reader wrote."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::Resolved)
                            .boolean()
                            .not_null()
                            .comment("`true` once staff have addressed the comment."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(DocumentComments::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_document_comments_review_document")
                            .from(DocumentComments::Table, DocumentComments::ReviewDocumentId)
                            .to(ReviewDocuments::Table, ReviewDocuments::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_document_comments_person")
                            .from(DocumentComments::Table, DocumentComments::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_document_comments_review_document_id")
                    .table(DocumentComments::Table)
                    .col(DocumentComments::ReviewDocumentId)
                    .col(DocumentComments::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_document_comments_review_document_id")
                    .table(DocumentComments::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(DocumentComments::Table).to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_review_documents_notation_id")
                    .table(ReviewDocuments::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ReviewDocuments::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ReviewDocuments {
    Table,
    Id,
    NotationId,
    Kind,
    Title,
    BodyHtml,
    Status,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DocumentComments {
    Table,
    Id,
    ReviewDocumentId,
    PersonId,
    AnchorStart,
    AnchorEnd,
    QuotedText,
    Body,
    Resolved,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
