//! `attestations` — durable local record of one on-chain attorney
//! attestation (the Neon Law Node product).
//!
//! When the matter reaches the `onchain__record_attestation` step the
//! worker hashes the signed attestation document, writes this row, and —
//! when a real chain backend is configured — records the same hash on
//! Solana (the `pda` + `tx_signature`). The **row is the system of
//! record**: it is written unconditionally, inside the step's `ctx.run`,
//! so it survives even if the chain write is deferred, fails, or the
//! backend is the `null` (no-chain) attestor. `status` distinguishes
//! `pending` (no on-chain tx yet) from `recorded` (a real tx landed).
//!
//! One row per notation: `notation_id` is unique, so a Restate replay of
//! the journaled step upserts rather than duplicating. The on-chain
//! payload is identifiers + a hash only — never client content — which
//! is the same trust boundary telemetry observes.

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
                    .table(Attestations::Table)
                    .if_not_exists()
                    .comment(
                        "Attestation — durable local record of one on-chain attorney \
                         attestation (Neon Law Node), written by the worker inside the \
                         onchain__record_attestation step. The row is the system of record; \
                         the Solana tx is a mirror. See docs/notation-authoring.md.",
                    )
                    .col(
                        ColumnDef::new(Attestations::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Attestation (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(Attestations::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`) — the matter attested."),
                    )
                    .col(
                        ColumnDef::new(Attestations::Chain)
                            .string()
                            .not_null()
                            .comment(
                                "On-chain backend that recorded (or would record) this \
                                 attestation: `solana`, or `null` when no chain is configured.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Attestations::Sha256)
                            .string()
                            .not_null()
                            .comment("Lowercase hex SHA-256 of the attested document bytes."),
                    )
                    .col(
                        ColumnDef::new(Attestations::Status)
                            .string()
                            .not_null()
                            .comment(
                                "`pending` (row written, no on-chain tx yet), `recorded` (a real \
                                 chain tx landed), or `failed` (the chain write errored).",
                            ),
                    )
                    .col(ColumnDef::new(Attestations::Pda).string().null().comment(
                        "Solana Program Derived Address holding the attestation account. \
                                 Null until a real chain backend records it.",
                    ))
                    .col(
                        ColumnDef::new(Attestations::TxSignature)
                            .string()
                            .null()
                            .comment(
                                "Solana transaction signature. Null until a real chain tx lands.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Attestations::FirmWallet)
                            .string()
                            .null()
                            .comment("Firm wallet public key bound in the attestation."),
                    )
                    .col(
                        ColumnDef::new(Attestations::ClientWallet)
                            .string()
                            .null()
                            .comment("Client wallet public key bound in the attestation."),
                    )
                    .col(
                        ColumnDef::new(Attestations::RecordedAt)
                            .string()
                            .null()
                            .comment(
                                "RFC 3339 timestamp the on-chain tx confirmed. Null while pending.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Attestations::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Attestations::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_attestations_notation")
                            .from(Attestations::Table, Attestations::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // One attestation per notation — the idempotency key. A replay of
        // the journaled onchain step upserts on this constraint rather
        // than writing a second row.
        manager
            .create_index(
                Index::create()
                    .name("idx_attestations_notation_id")
                    .table(Attestations::Table)
                    .col(Attestations::NotationId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_attestations_notation_id")
                    .table(Attestations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Attestations::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Attestations {
    Table,
    Id,
    NotationId,
    Chain,
    Sha256,
    Status,
    Pda,
    TxSignature,
    FirmWallet,
    ClientWallet,
    RecordedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}
