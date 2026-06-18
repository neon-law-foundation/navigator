//! Add `retainer_template_code` to `products`.
//!
//! The retainer template `code` (e.g. `onboarding__retainer_nest`) whose
//! engagement agreement a matter under this product opens with — the
//! demand-side mirror of [`matter_close_template_code`], which names the
//! template whose *close* raises the product's fee. Pinning the retainer
//! on the product row makes "each product opens its own retainer" a data
//! mapping, not a hard-coded `match` in the matter-open handler.
//!
//! Soft reference, not a FK (the same convention as
//! `matter_close_template_code`). Nullable: a product with no dedicated
//! retainer falls back to the generic `onboarding__retainer`.

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
                        ColumnDef::new(Products::RetainerTemplateCode)
                            .string()
                            .null()
                            .comment(
                                "The retainer template `code` whose engagement agreement a \
                                 matter under this product opens with (e.g. \
                                 `onboarding__retainer_nest`). Soft reference, not a FK. \
                                 NULL falls back to the generic `onboarding__retainer`.",
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
                    .drop_column(Products::RetainerTemplateCode)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    RetainerTemplateCode,
}
