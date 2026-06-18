//! `xero_invoices` — the local mirror of a matter's Xero invoice, plus
//! `persons.xero_contact_id` — the Xero `ContactID` cache.
//!
//! Why a dedicated mirror table rather than extending `invoices`: the
//! canonical [`invoices`](super::m20260525_create_billing_tables) table
//! is **entity-billing-profile-scoped** (internal accounts-receivable),
//! while a matter-close Xero invoice is **project-scoped** and keyed by
//! the Navigator `project_id` carried into Xero's invoice `Reference`
//! field. Overloading `invoices` with a half-null `project_id` /
//! `xero_invoice_id` would muddy that seam, so the Xero side gets its
//! own table. The portal reads this mirror; it never calls Xero live.
//!
//! `UNIQUE(project_id)` enforces the invariant that a matter has at most
//! one close invoice — the same key the provider dedupes on via its
//! `Idempotency-Key: <project_id>` header — so a replay or a double-close
//! updates the one row rather than writing a second.
//!
//! `persons.xero_contact_id` caches the Xero `ContactID` the first time a
//! person is mirrored to Xero Contacts (one-way, Navigator → Xero). It
//! backs both the contacts sync and the admin people-detail Xero
//! deep-link; nullable, since every existing person predates it.

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
                    .table(XeroInvoices::Table)
                    .if_not_exists()
                    .comment(
                        "Xero Invoice mirror — the locally-persisted record of a \
                         matter's Xero invoice, keyed by `project_id`. The portal \
                         reads this; it never calls Xero live. One row per matter \
                         (UNIQUE project_id).",
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this mirror row."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::ProjectId)
                            .uuid()
                            .not_null()
                            .unique_key()
                            .comment(
                                "FK → Project (`projects.id`); at most one close \
                                 invoice per matter.",
                            ),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::XeroInvoiceId)
                            .string()
                            .not_null()
                            .comment("Xero `InvoiceID` (GUID) returned on create."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::Reference)
                            .string()
                            .not_null()
                            .comment(
                                "The invoice-level `Reference` carried into Xero \
                                 (`Matter <project_id>`); the durable join key.",
                            ),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::Status)
                            .string()
                            .not_null()
                            .comment(
                                "Xero invoice status mirror (`AUTHORISED`, `PAID`, \
                                 `VOIDED`, …). Updated by the reconcile workflow.",
                            ),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::AmountCents)
                            .big_integer()
                            .not_null()
                            .comment("Invoice total in minor units (cents). No float."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::AmountPaidCents)
                            .big_integer()
                            .not_null()
                            .default(0)
                            .comment("Amount paid in minor units (cents); 0 until reconciled."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::Currency)
                            .string()
                            .not_null()
                            .comment("ISO 4217 currency code (e.g., `USD`)."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(XeroInvoices::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_xero_invoices_project")
                            .from(XeroInvoices::Table, XeroInvoices::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .add_column(ColumnDef::new(Persons::XeroContactId).string().comment(
                        "Xero `ContactID` (GUID) once mirrored to Xero \
                             Contacts; `None` until first synced.",
                    ))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::XeroContactId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(XeroInvoices::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum XeroInvoices {
    Table,
    Id,
    ProjectId,
    XeroInvoiceId,
    Reference,
    Status,
    AmountCents,
    AmountPaidCents,
    Currency,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    XeroContactId,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
