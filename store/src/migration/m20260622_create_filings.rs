//! `filings` — durable record of one outbound compliance submission.
//!
//! When a compliance workflow reaches a submission step
//! (`mailroom_send`, `certified_mail`, `e_filing`, `filing__*`) the
//! worker records what was submitted, to which office, and when —
//! inside the step's `ctx.run`, so the record is the durable, replay-
//! idempotent proof of the filing. One row per submission.
//!
//! Crucially, a row is only ever written *after* the matter passes
//! `staff_review` — the workflow spec guarantees no submission state is
//! reachable without crossing a review (the
//! `workflows::staff_review_precedes_submission` guardrail), so a
//! `filings` row means an attorney approved the specific filing.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Filings::Table)
                    .if_not_exists()
                    .comment(
                        "Filing — durable record of one outbound compliance submission \
                         (mail to a party or a filing with a government office), written \
                         by the worker after staff_review. See docs/notation-authoring.md.",
                    )
                    .col(
                        ColumnDef::new(Filings::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Filing (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(Filings::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`) — the matter filed."),
                    )
                    .col(ColumnDef::new(Filings::Kind).string().not_null().comment(
                        "Submission step kind: `mailroom_send`, `certified_mail`, \
                                 `e_filing`, or `filing` — the state-name prefix that fired it.",
                    ))
                    .col(
                        ColumnDef::new(Filings::Office).string().not_null().comment(
                            "Recipient office / party (e.g. `Nevada Secretary of State`).",
                        ),
                    )
                    .col(ColumnDef::new(Filings::Reference).string().null().comment(
                        "Provider/office tracking reference (certified-mail number, \
                                 e-filing receipt id). Null until the provider returns one.",
                    ))
                    .col(
                        ColumnDef::new(Filings::Summary)
                            .text()
                            .not_null()
                            .comment("Human-readable summary of what was submitted."),
                    )
                    .col(
                        ColumnDef::new(Filings::SubmittedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp the submission side effect fired."),
                    )
                    .col(
                        ColumnDef::new(Filings::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Filings::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_filings_notation")
                            .from(Filings::Table, Filings::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_filings_notation_id")
                    .table(Filings::Table)
                    .col(Filings::NotationId)
                    .col(Filings::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_filings_notation_id")
                    .table(Filings::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Filings::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Filings {
    Table,
    Id,
    NotationId,
    Kind,
    Office,
    Reference,
    Summary,
    SubmittedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}
