//! Add the admin-discretion discount override to `notations`.
//!
//! **Neon Law Navigator is the system of record for the discount *decision*; Xero
//! does the client-facing math.** The list price stays one number in the
//! `products` catalog; a discount is a separate recorded event, applied
//! to the matter-close invoice as a Xero line-item discount (see
//! `billing::LineDiscount`). These columns are the audit trail of that
//! decision and what the engagement-letter fee is computed against.
//!
//! A discount only ever goes **down** from list (RPC 7.1 — billing below
//! an advertised flat fee is truthful; above it is misleading). The
//! below-only guardrail is enforced in code at raise time
//! (`billing::MatterCloseInvoiceRequest::validate_discount`), not by a
//! check constraint, because "below list" is only meaningful against the
//! catalog list price the matter resolves to.
//!
//! All columns are nullable: the overwhelming common case is no discount
//! (billed at list), and every existing notation predates this.
//!
//! - `discount_pct` — whole-number percent off (`0..=100`); or
//! - `discount_amount_cents` — a flat amount off, in minor units. At most
//!   one of the two is set per notation.
//! - `discount_reason` — the recorded basis (hardship / pro bono / PPP /
//!   mission), reflected in the engagement letter and the invoice.
//! - `discount_approved_by` — the approving staff/admin's email.
//! - `discount_approved_at` — RFC 3339 timestamp of the approval.

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
                    .add_column(ColumnDef::new(Notations::DiscountPct).integer().comment(
                        "Whole-number percent off list (0..=100); NULL when no \
                         percentage discount. At most one of discount_pct / \
                         discount_amount_cents is set.",
                    ))
                    .add_column(
                        ColumnDef::new(Notations::DiscountAmountCents)
                            .big_integer()
                            .comment(
                                "Flat amount off list, in minor units (cents); NULL \
                                 when no flat discount.",
                            ),
                    )
                    .add_column(ColumnDef::new(Notations::DiscountReason).string().comment(
                        "Recorded basis for the discount (hardship / pro bono / PPP / \
                         mission); NULL when undiscounted.",
                    ))
                    .add_column(
                        ColumnDef::new(Notations::DiscountApprovedBy)
                            .string()
                            .comment("Approving staff/admin email; NULL when undiscounted."),
                    )
                    .add_column(
                        ColumnDef::new(Notations::DiscountApprovedAt)
                            .string()
                            .comment(
                                "RFC 3339 timestamp of the discount approval; NULL when \
                             undiscounted.",
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
                    .table(Notations::Table)
                    .drop_column(Notations::DiscountPct)
                    .drop_column(Notations::DiscountAmountCents)
                    .drop_column(Notations::DiscountReason)
                    .drop_column(Notations::DiscountApprovedBy)
                    .drop_column(Notations::DiscountApprovedAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    DiscountPct,
    DiscountAmountCents,
    DiscountReason,
    DiscountApprovedBy,
    DiscountApprovedAt,
}
