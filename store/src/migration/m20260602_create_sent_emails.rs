//! `sent_emails` — append-only audit log of every outbound message
//! that went through the `EmailService` (SendGrid in prod,
//! `CapturingEmail` in dev when wrapped). Reading this table answers
//! "did we send Aries a welcome, and when?". Gmail-sourced mail from
//! Workspace mailboxes is NOT in scope: that path doesn't touch the
//! `EmailService` trait and is intentionally private.
//!
//! The table is read by the `/portal/admin/email-log` index and written by
//! the `LoggingEmail` decorator. There are no UPDATEs and no DELETEs
//! in application code; a future Restate cron will rotate older
//! rows out to parquet on GCS (Iceberg-friendly schema) and DELETE
//! rows older than 30 days from Postgres.
//!
//! Append-only enforcement at the Postgres role layer (REVOKE UPDATE,
//! DELETE from the `navigator-web` role) is configured separately
//! in the database-bootstrap path, not in this SeaORM migration —
//! the Postgres role-grant model lives outside SeaORM's vocabulary.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SentEmails::Table)
                    .if_not_exists()
                    .comment(
                        "SentEmail — one outbound message that went through the \
                         EmailService trait. Append-only audit log; rows are \
                         rotated to parquet on GCS by the Iceberg export cron \
                         and never updated in place.",
                    )
                    .col(
                        ColumnDef::new(SentEmails::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this SentEmail row."),
                    )
                    .col(
                        ColumnDef::new(SentEmails::Recipient)
                            .string()
                            .not_null()
                            .comment("Envelope `To:` address as supplied to the trait."),
                    )
                    .col(
                        ColumnDef::new(SentEmails::Subject)
                            .string()
                            .not_null()
                            .comment("Message subject — useful index column for filtering."),
                    )
                    .col(
                        ColumnDef::new(SentEmails::Sender)
                            .string()
                            .not_null()
                            .comment(
                                "Envelope `From:` address — `support@neonlaw.com` for the \
                                 default outbound path; carried for the audit trail in \
                                 case a future template uses a different from-address.",
                            ),
                    )
                    .col(
                        ColumnDef::new(SentEmails::TemplateSlug)
                            .string()
                            .null()
                            .comment(
                                "Template identifier (e.g. `welcome`) when the body \
                                 came from a named template; null for ad-hoc messages.",
                            ),
                    )
                    .col(ColumnDef::new(SentEmails::Body).text().not_null().comment(
                        "Rendered body bytes. Kept inline today to make the parquet \
                                 export trivially flat; split into object storage if the \
                                 average row outgrows the page size.",
                    ))
                    .col(
                        ColumnDef::new(SentEmails::Outcome)
                            .string()
                            .not_null()
                            .comment(
                                "`sent` on success, `failed:<reason>` on failure. The \
                                 decorator writes one row per attempt, so retries land \
                                 as separate rows.",
                            ),
                    )
                    .col(
                        ColumnDef::new(SentEmails::SentAt)
                            .string()
                            .not_null()
                            .comment(
                                "RFC 3339 / ISO 8601 timestamp when the row was written. \
                                 Matches the timestamp convention used by notation_events.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;

        // Index for the admin index view's hot read: newest-first
        // listing, paginated. Without this the page does a sequential
        // scan once the table grows past a few thousand rows.
        manager
            .create_index(
                Index::create()
                    .name("idx_sent_emails_sent_at")
                    .table(SentEmails::Table)
                    .col(SentEmails::SentAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SentEmails::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum SentEmails {
    Table,
    Id,
    Recipient,
    Subject,
    Sender,
    TemplateSlug,
    Body,
    Outcome,
    SentAt,
}
