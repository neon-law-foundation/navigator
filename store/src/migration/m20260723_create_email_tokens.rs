//! `email_tokens` — single-use, expiring tokens emailed to a person to
//! prove control of their address.
//!
//! Two purposes share one table because the mechanics are identical
//! (hash + expiry + single-use + per-person throttle):
//!
//! - `password_reset` — the link in a "reset your password" email. On
//!   confirm, `web` sets a new password in GCP Identity Platform (the
//!   password vault) for the matching account.
//! - `email_confirm` — the link in a "confirm your email" email. On
//!   confirm, `web` flips `emailVerified` in Identity Platform. The
//!   sign-in tail hard-gates an unverified password user until they
//!   click it (Google sign-in carries `email_verified: true` already,
//!   so the rule is "sign in with Google **or** confirm your email").
//!
//! Tokens are stored **hashed** (`token_hash` = SHA-256 hex); the
//! plaintext lives only in the emailed URL and is never persisted.
//! `used_at` enforces single use; `expires_at` bounds the window;
//! `inserted_at` backs the throttle ("don't mint a second link within
//! N seconds"). Identity is owned by Identity Platform — this table
//! only carries our `persons` FK plus the email snapshot for audit.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EmailTokens::Table)
                    .if_not_exists()
                    .comment(
                        "EmailToken — a single-use, expiring token emailed to a person to \
                         prove control of their address. `purpose` is `password_reset` or \
                         `email_confirm`. Stored hashed; the plaintext lives only in the \
                         emailed link. `used_at` enforces single use. Revocation = setting \
                         `used_at` or letting `expires_at` pass.",
                    )
                    .col(
                        ColumnDef::new(EmailTokens::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this token (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — whose account this token acts on."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::Email)
                            .string()
                            .not_null()
                            .comment("Snapshot of the recipient address at mint time, for audit."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::Purpose)
                            .string()
                            .not_null()
                            .comment("`password_reset` or `email_confirm` — what claiming the token does."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::TokenHash)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("SHA-256 hex of the token plaintext; the plaintext is only ever in the emailed link."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::ExpiresAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 expiry; a token at or past this instant is rejected as if absent."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::UsedAt)
                            .string()
                            .comment("RFC 3339 timestamp the token was claimed. NULL = unused; set = spent (single-use)."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted; backs the per-person mint throttle."),
                    )
                    .col(
                        ColumnDef::new(EmailTokens::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_email_tokens_person")
                            .from(EmailTokens::Table, EmailTokens::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_email_tokens_person_purpose")
                    .table(EmailTokens::Table)
                    .col(EmailTokens::PersonId)
                    .col(EmailTokens::Purpose)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_email_tokens_person_purpose")
                    .table(EmailTokens::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(EmailTokens::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum EmailTokens {
    Table,
    Id,
    PersonId,
    Email,
    Purpose,
    TokenHash,
    ExpiresAt,
    UsedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
