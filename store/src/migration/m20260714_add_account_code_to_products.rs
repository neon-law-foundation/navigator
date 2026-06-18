//! Add `account_code` to `products`.
//!
//! The Xero chart-of-accounts code a product's revenue posts to (e.g.
//! `200` = Sales). Before this column the account code was passed
//! ad-hoc by the matter-close caller; pinning it on the product row
//! keeps the Xero mapping beside `xero_item_code`, so both the
//! matter-close fee and the recurring-subscription invoice draw the
//! revenue account from the single source of truth.
//!
//! Added NOT NULL with a `200` default so existing rows backfill to the
//! standard revenue account without a data migration.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Products::Table)
                    .add_column(
                        ColumnDef::new(Products::AccountCode)
                            .string()
                            .not_null()
                            .default("200")
                            .comment(
                                "Xero chart-of-accounts code this product's revenue \
                                 posts to (e.g. `200` = Sales). The recurring-billing \
                                 workflow reads it as the invoice line's `AccountCode`.",
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
                    .table(Products::Table)
                    .drop_column(Products::AccountCode)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    AccountCode,
}
