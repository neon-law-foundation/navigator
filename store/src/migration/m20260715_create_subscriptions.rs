//! `subscriptions` — an active recurring engagement: a billed party tied
//! to a `recurring` product, billed one Xero invoice per period.
//!
//! The `products` catalog holds the *price* and cadence of a recurring
//! product (Nexus, Nautilus), but not the per-engagement state the
//! recurring-billing workflow needs: who is billed, since when, and
//! through which period it has already been invoiced. A `projects` row
//! models a *matter* (open/closed, a close fee), not an open-ended
//! subscription, so a recurring engagement gets its own row here.
//!
//! `last_invoiced_period` (`YYYY-MM`, UTC) is the durable idempotency
//! ledger: the workflow bills every `active` subscription whose
//! `last_invoiced_period < current_period`, then advances it only after
//! the Xero invoice returns Ok — so a re-run in the same month bills no
//! one twice. See `billing-workflows::recurring`.
//!
//! A discount is the same below-list event the matter-close path records:
//! at most one of `discount_percent` / `discount_amount_cents` is set,
//! mirroring `billing::LineDiscount`'s two shapes. Both null bills at list.

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
                    .table(Subscriptions::Table)
                    .if_not_exists()
                    .comment(
                        "An active recurring engagement: a billed party tied to a \
                         `recurring` product, invoiced once per billing period by \
                         the recurring-billing workflow.",
                    )
                    .col(
                        ColumnDef::new(Subscriptions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this subscription."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::PersonId)
                            .uuid()
                            .comment("Billed person, when the payer is an individual. Soft link."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::EntityId).uuid().comment(
                            "Billed entity, when the payer is an organisation. Soft link.",
                        ),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::ProjectId)
                            .uuid()
                            .comment("Originating project/matter, when one exists. Soft link."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::ProductCode)
                            .string()
                            .not_null()
                            .comment(
                                "The recurring product's `code` (`nexus`, `nautilus`). \
                                 Soft reference to `products.code`; the workflow reads \
                                 the price + account code from that row.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::ContactName)
                            .string()
                            .not_null()
                            .comment("Billed party's display name (the Xero contact `Name`)."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::ContactEmail)
                            .string()
                            .not_null()
                            .comment("Billed party's email — the Xero contact match key."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::Status)
                            .string()
                            .not_null()
                            .default("active")
                            .comment("`active` | `paused` | `cancelled`. Only `active` is billed."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::StartedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp the subscription began."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::LastInvoicedPeriod)
                            .string()
                            .comment(
                                "The most recent billing period (`YYYY-MM`, UTC) already \
                                 invoiced. `None` = never billed. The durable idempotency \
                                 ledger: advanced only after a successful Xero invoice.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::DiscountPercent)
                            .integer()
                            .comment(
                                "Optional whole-percent discount off list (`0..=100`), \
                                 mirroring `billing::LineDiscount::Percent`. At most one \
                                 of the two discount columns is set.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::DiscountAmountCents)
                            .big_integer()
                            .comment(
                                "Optional flat discount off list, in cents, mirroring \
                                 `billing::LineDiscount::AmountCents`. At most one of the \
                                 two discount columns is set.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Subscriptions::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Subscriptions::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Subscriptions {
    Table,
    Id,
    PersonId,
    EntityId,
    ProjectId,
    ProductCode,
    ContactName,
    ContactEmail,
    Status,
    StartedAt,
    LastInvoicedPeriod,
    DiscountPercent,
    DiscountAmountCents,
    InsertedAt,
    UpdatedAt,
}
