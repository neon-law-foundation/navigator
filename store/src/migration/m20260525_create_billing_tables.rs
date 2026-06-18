//! `entity_billing_profiles`, `invoices`, `invoice_line_items` —
//! the billing chain. See glossary terms
//! [Entity Billing Profile](../../../docs/glossary.md#entity-billing-profile),
//! [Invoice](../../../docs/glossary.md#invoice), and
//! [Invoice Line Item](../../../docs/glossary.md#invoice-line-item).

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
                    .table(EntityBillingProfiles::Table)
                    .if_not_exists()
                    .comment(
                        "Entity Billing Profile — one billing profile per Entity \
                         (billing email + address). Invoices belong to a profile, \
                         not directly to an Entity. \
                         See docs/glossary.md#entity-billing-profile.",
                    )
                    .col(
                        ColumnDef::new(EntityBillingProfiles::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Entity Billing Profile."),
                    )
                    .col(
                        ColumnDef::new(EntityBillingProfiles::EntityId)
                            .uuid()
                            .not_null()
                            .unique_key()
                            .comment(
                                "FK → Entity (`entities.id`); exactly one profile per Entity.",
                            ),
                    )
                    .col(
                        ColumnDef::new(EntityBillingProfiles::BillingEmail)
                            .string()
                            .not_null()
                            .comment("Billing-contact email for the Entity."),
                    )
                    .col(
                        ColumnDef::new(EntityBillingProfiles::BillingAddressId)
                            .uuid()
                            .null()
                            .comment("FK → Address (`addresses.id`), nullable when not yet set."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_ebp_entity")
                            .from(
                                EntityBillingProfiles::Table,
                                EntityBillingProfiles::EntityId,
                            )
                            .to(Entities::Table, Entities::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_ebp_address")
                            .from(
                                EntityBillingProfiles::Table,
                                EntityBillingProfiles::BillingAddressId,
                            )
                            .to(Addresses::Table, Addresses::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Invoices::Table)
                    .if_not_exists()
                    .comment(
                        "Invoice — one billable invoice owned by an Entity Billing \
                         Profile; totals are stored in minor units (cents). \
                         See docs/glossary.md#invoice.",
                    )
                    .col(
                        ColumnDef::new(Invoices::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Invoice."),
                    )
                    .col(
                        ColumnDef::new(Invoices::EntityBillingProfileId)
                            .uuid()
                            .not_null()
                            .comment("FK → Entity Billing Profile (`entity_billing_profiles.id`)."),
                    )
                    .col(
                        ColumnDef::new(Invoices::Number)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Caller-visible invoice number (e.g., `INV-2026-0001`)."),
                    )
                    .col(
                        ColumnDef::new(Invoices::Status)
                            .string()
                            .not_null()
                            .comment("`draft`, `issued`, `paid`, or `void`."),
                    )
                    .col(
                        ColumnDef::new(Invoices::TotalCents)
                            .big_integer()
                            .not_null()
                            .comment("Total amount in minor units (cents). Avoids float."),
                    )
                    .col(
                        ColumnDef::new(Invoices::Currency)
                            .string()
                            .not_null()
                            .comment("ISO 4217 currency code (e.g., `USD`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_invoices_ebp")
                            .from(Invoices::Table, Invoices::EntityBillingProfileId)
                            .to(EntityBillingProfiles::Table, EntityBillingProfiles::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(InvoiceLineItems::Table)
                    .if_not_exists()
                    .comment(
                        "Invoice Line Item — one billable line on an Invoice \
                         (description, quantity, unit price). \
                         See docs/glossary.md#invoice-line-item.",
                    )
                    .col(
                        ColumnDef::new(InvoiceLineItems::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Invoice Line Item."),
                    )
                    .col(
                        ColumnDef::new(InvoiceLineItems::InvoiceId)
                            .uuid()
                            .not_null()
                            .comment("FK → Invoice (`invoices.id`)."),
                    )
                    .col(
                        ColumnDef::new(InvoiceLineItems::Description)
                            .string()
                            .not_null()
                            .comment("Description of the billable line."),
                    )
                    .col(
                        ColumnDef::new(InvoiceLineItems::Quantity)
                            .integer()
                            .not_null()
                            .comment("Quantity (integer; not a UUID)."),
                    )
                    .col(
                        ColumnDef::new(InvoiceLineItems::UnitPriceCents)
                            .big_integer()
                            .not_null()
                            .comment("Per-unit price in minor units (cents)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_line_items_invoice")
                            .from(InvoiceLineItems::Table, InvoiceLineItems::InvoiceId)
                            .to(Invoices::Table, Invoices::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(InvoiceLineItems::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Invoices::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(EntityBillingProfiles::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum EntityBillingProfiles {
    Table,
    Id,
    EntityId,
    BillingEmail,
    BillingAddressId,
}

#[derive(DeriveIden)]
enum Invoices {
    Table,
    Id,
    EntityBillingProfileId,
    Number,
    Status,
    TotalCents,
    Currency,
}

#[derive(DeriveIden)]
enum InvoiceLineItems {
    Table,
    Id,
    InvoiceId,
    Description,
    Quantity,
    UnitPriceCents,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Addresses {
    Table,
    Id,
}
