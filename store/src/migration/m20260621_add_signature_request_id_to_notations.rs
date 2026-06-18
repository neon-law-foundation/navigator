//! Add `signature_request_id` to `notations`.
//!
//! Closes the signature loop's correlation gap. When the workflow
//! sends a rendered retainer out for e-signature
//! (`web::signature::SignatureProvider::send_for_signature`), the
//! provider hands back an opaque request id (DocuSign's `envelopeId`).
//! Previously that id was rendered onto the result page and then
//! discarded, so the provider's later "completed" webhook had no way
//! to find the notation it belonged to.
//!
//! Nullable on purpose — only notations that have reached
//! `sent_for_signature__pending` carry one, and the dev/test
//! `StubSignatureProvider` still issues synthetic ids. Indexed because
//! the inbound webhook's only lookup key is this column: it receives
//! the provider's request id and must resolve it back to one notation.
//!
//! 1:1 assumption: one envelope per notation. If a notation ever needs
//! re-sends (voided envelope → fresh envelope) or multiple signers, a
//! single column lies and the escape hatch is a `signature_requests`
//! side table keyed by request id. We do not build that speculatively.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .add_column(
                        ColumnDef::new(Notations::SignatureRequestId)
                            .string()
                            .null()
                            .comment(
                                "Opaque e-signature provider request id (DocuSign envelopeId) \
                                 for this notation; the inbound completion webhook's lookup key. \
                                 Null until the retainer reaches sent_for_signature__pending.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notations_signature_request_id")
                    .table(Notations::Table)
                    .col(Notations::SignatureRequestId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_notations_signature_request_id")
                    .table(Notations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .drop_column(Notations::SignatureRequestId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    SignatureRequestId,
}
