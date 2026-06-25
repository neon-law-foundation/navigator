//! `coupons` — a reusable, named discount the firm can apply to a
//! subscription at sign-up (e.g. `FRIEND99` for 99% off).
//!
//! Xero has no coupon primitive — only a per-invoice `DiscountRate` /
//! `DiscountAmount`. A coupon is therefore a Neon Law Navigator concept: the
//! *intent* of a standing discount, owned here. When a coupon is applied
//! to a subscription it is resolved to one of `billing::LineDiscount`'s
//! two shapes and **snapshotted** onto the subscription's own discount
//! columns, so later expiring or editing the coupon never silently
//! re-prices an existing client's monthly invoice.
//!
//! A coupon carries at most one of `discount_percent` /
//! `discount_amount_cents` (mirroring the subscription columns), an
//! optional `product_code` scope (null = any product), an optional
//! `expires_at` (null = never), and an optional `max_redemptions` cap
//! tracked by `redeemed_count`.

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
                    .table(Coupons::Table)
                    .if_not_exists()
                    .comment(
                        "A reusable, named discount applied to a subscription at \
                         sign-up. Resolves to a `billing::LineDiscount` and is \
                         snapshotted onto the subscription's discount columns.",
                    )
                    .col(
                        ColumnDef::new(Coupons::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this coupon."),
                    )
                    .col(
                        ColumnDef::new(Coupons::Code)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment(
                                "The redeemable code (`FRIEND99`). Unique, \
                                 case-sensitive; the key staff hand to a client.",
                            ),
                    )
                    .col(ColumnDef::new(Coupons::DiscountPercent).integer().comment(
                        "Whole-percent discount off list (`0..=100`), \
                                 mirroring `billing::LineDiscount::Percent`. At most \
                                 one of the two discount columns is set.",
                    ))
                    .col(
                        ColumnDef::new(Coupons::DiscountAmountCents)
                            .big_integer()
                            .comment(
                                "Flat discount off list, in cents, mirroring \
                                 `billing::LineDiscount::AmountCents`. At most one of \
                                 the two discount columns is set.",
                            ),
                    )
                    .col(ColumnDef::new(Coupons::ProductCode).string().comment(
                        "Optional product `code` this coupon is scoped to \
                             (`nexus`). `None` = applies to any product.",
                    ))
                    .col(ColumnDef::new(Coupons::ExpiresAt).string().comment(
                        "Optional RFC 3339 (UTC) expiry. `None` = never expires; \
                             a coupon applied at or after this instant is rejected.",
                    ))
                    .col(ColumnDef::new(Coupons::MaxRedemptions).integer().comment(
                        "Optional cap on how many subscriptions may apply this \
                             coupon. `None` = unlimited.",
                    ))
                    .col(
                        ColumnDef::new(Coupons::RedeemedCount)
                            .integer()
                            .not_null()
                            .default(0)
                            .comment(
                                "How many times this coupon has been applied. \
                                 Incremented atomically on each redemption; the cap \
                                 is `max_redemptions`.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Coupons::Active)
                            .boolean()
                            .not_null()
                            .default(true)
                            .comment(
                                "Whether the coupon may currently be applied. A \
                                 deactivated coupon is rejected but its prior \
                                 snapshots on existing subscriptions are unaffected.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Coupons::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Coupons::UpdatedAt)
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
            .drop_table(Table::drop().table(Coupons::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Coupons {
    Table,
    Id,
    Code,
    DiscountPercent,
    DiscountAmountCents,
    ProductCode,
    ExpiresAt,
    MaxRedemptions,
    RedeemedCount,
    Active,
    InsertedAt,
    UpdatedAt,
}
