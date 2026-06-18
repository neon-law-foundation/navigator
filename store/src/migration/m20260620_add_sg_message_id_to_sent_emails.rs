//! Add `sg_message_id` to `sent_emails`.
//!
//! The column holds SendGrid's `X-Message-Id` response header,
//! captured by the `LoggingEmail` decorator on a successful 202. It
//! is the join key against the delivery-side Event Webhook stream
//! (`web::email_events` lands those events as Parquet on GCS, queried
//! from BigQuery): one outbound row in `sent_emails` lines up with
//! N lifecycle events (`processed`, `delivered`, `open`, `click`, …)
//! that all carry the same message id.
//!
//! Nullable on purpose — failed sends and the `CapturingEmail` dev
//! backend never receive an id, so backfilling existing rows with a
//! sentinel would lie. The append-only contract from
//! `m20260602_create_sent_emails` still holds: this is a one-shot
//! schema add, not a row rewrite.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SentEmails::Table)
                    .add_column(
                        ColumnDef::new(SentEmails::SgMessageId)
                            .string()
                            .null()
                            .comment(
                                "SendGrid `X-Message-Id` for this send; join key to the \
                                 delivery-side Event Webhook stream. Null for failed sends \
                                 and the capturing dev backend.",
                            ),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SentEmails::Table)
                    .drop_column(SentEmails::SgMessageId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum SentEmails {
    Table,
    SgMessageId,
}
