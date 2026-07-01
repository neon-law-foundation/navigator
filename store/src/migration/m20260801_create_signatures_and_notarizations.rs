//! Create `signatures` and `notarizations`, and retire
//! `notations.signature_request_id`.
//!
//! The envelope id used to live inline on `notations.signature_request_id`
//! — one opaque provider id per notation, with no room for the signer,
//! field, provider, or completion timestamp. This lifts it into a first-
//! class `signatures` row keyed on `(provider, provider_id)` (the webhook's
//! correlation key), adds the notary counterpart `notarizations`, backfills
//! every existing envelope id as a `docusign` signature, and drops the
//! column (no shim — reads resolve through the table).

use sea_orm::{DbBackend, Statement};
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
                    .table(Signatures::Table)
                    .if_not_exists()
                    .comment(
                        "Signature — one e-signature request/execution on a Notation's \
                         document, correlated from the provider by (provider, provider_id).",
                    )
                    .col(
                        ColumnDef::new(Signatures::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this signature."),
                    )
                    .col(
                        ColumnDef::new(Signatures::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`)."),
                    )
                    .col(
                        ColumnDef::new(Signatures::SignerPersonId)
                            .uuid()
                            .null()
                            .comment("FK → the signing Person; null until resolved."),
                    )
                    .col(
                        ColumnDef::new(Signatures::Field)
                            .string()
                            .null()
                            .comment("Signature field/tab (e.g. `client.signature`); null for the whole envelope."),
                    )
                    .col(
                        ColumnDef::new(Signatures::Provider)
                            .string()
                            .not_null()
                            .comment("E-signature provider (`docusign`)."),
                    )
                    .col(
                        ColumnDef::new(Signatures::ProviderId)
                            .string()
                            .not_null()
                            .comment("Opaque provider request id (DocuSign envelopeId)."),
                    )
                    .col(
                        ColumnDef::new(Signatures::SignedAt)
                            .string()
                            .null()
                            .comment("RFC 3339 completion timestamp; null until stamped by the webhook."),
                    )
                    .col(
                        ColumnDef::new(Signatures::InsertedAt)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Signatures::UpdatedAt).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_signatures_notation")
                            .from(Signatures::Table, Signatures::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_signatures_signer")
                            .from(Signatures::Table, Signatures::SignerPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_signatures_provider_provider_id")
                    .table(Signatures::Table)
                    .col(Signatures::Provider)
                    .col(Signatures::ProviderId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Notarizations::Table)
                    .if_not_exists()
                    .comment(
                        "Notarization — one notarization request/execution on a Notation's \
                         document, correlated from the provider by (provider, provider_id).",
                    )
                    .col(
                        ColumnDef::new(Notarizations::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this notarization."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::NotationId)
                            .uuid()
                            .not_null()
                            .comment("FK → Notation (`notations.id`)."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::NotaryPersonId)
                            .uuid()
                            .null()
                            .comment("FK → the notary Person; null until resolved."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::DocumentId)
                            .uuid()
                            .null()
                            .comment("FK → the notarized Document; null until resolved."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::Provider)
                            .string()
                            .not_null()
                            .comment("Notarization provider (`docusign`)."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::ProviderId)
                            .string()
                            .not_null()
                            .comment("Opaque provider request id."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::NotarizedAt)
                            .string()
                            .null()
                            .comment("RFC 3339 completion timestamp; null until stamped."),
                    )
                    .col(
                        ColumnDef::new(Notarizations::InsertedAt)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Notarizations::UpdatedAt).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notarizations_notation")
                            .from(Notarizations::Table, Notarizations::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notarizations_notary")
                            .from(Notarizations::Table, Notarizations::NotaryPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notarizations_document")
                            .from(Notarizations::Table, Notarizations::DocumentId)
                            .to(Documents::Table, Documents::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_notarizations_provider_provider_id")
                    .table(Notarizations::Table)
                    .col(Notarizations::Provider)
                    .col(Notarizations::ProviderId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Backfill every inline envelope id as a `docusign` signature so the
        // webhook keeps resolving completions after the column is dropped.
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "INSERT INTO signatures \
               (id, notation_id, provider, provider_id, inserted_at, updated_at) \
             SELECT gen_random_uuid(), id, 'docusign', signature_request_id, \
                    to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                    to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
             FROM notations WHERE signature_request_id IS NOT NULL"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "DROP INDEX IF EXISTS idx_notations_signature_request_id".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "ALTER TABLE notations DROP COLUMN signature_request_id".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "ALTER TABLE notations ADD COLUMN signature_request_id VARCHAR".to_string(),
        ))
        .await?;
        // Restore the inline id from the backfilled signatures.
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "UPDATE notations n SET signature_request_id = s.provider_id \
             FROM signatures s \
             WHERE s.notation_id = n.id AND s.provider = 'docusign'"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "CREATE INDEX idx_notations_signature_request_id \
             ON notations (signature_request_id)"
                .to_string(),
        ))
        .await?;
        manager
            .drop_table(Table::drop().table(Notarizations::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Signatures::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Signatures {
    Table,
    Id,
    NotationId,
    SignerPersonId,
    Field,
    Provider,
    ProviderId,
    SignedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Notarizations {
    Table,
    Id,
    NotationId,
    NotaryPersonId,
    DocumentId,
    Provider,
    ProviderId,
    NotarizedAt,
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

#[derive(DeriveIden)]
enum Documents {
    Table,
    Id,
}
