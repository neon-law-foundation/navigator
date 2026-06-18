//! `products` — the firm's product catalog and the single source of
//! truth for a product's list price.
//!
//! Before this table, a product's price was duplicated across at least
//! three representations and drifted: the marketing frontmatter
//! (`web/content/marketing/*.md`), the `flat_fee_cents()` match in
//! `web::retainer_walk`, and the Xero invoice built in `billing`.
//! Changing Nexus $5,000 → $2,222 touched ~10 locations. This table
//! collapses that to one authoritative list price per product, keyed by
//! a stable product `code`.
//!
//! **List price is data; a discount is a separate event, not a second
//! price.** This table holds exactly one `list_price_cents` per product.
//! An admin discount is recorded on the engagement (the notation) and
//! applied as a Xero line-item discount — never as a second row here.
//!
//! `code` is the marketing/Xero product key (`northstar`, `nest`,
//! `nexus`, `nautilus`, `litigation`), **not** a template prefix: the
//! marketed name and the template that opens the matter diverge
//! (Northstar's matter is opened by the `onboarding__estate` template).
//! The billing trigger is therefore a separate, explicit column —
//! `matter_close_template_code` — naming the originating template
//! `code` whose matter-close raises this product's flat fee. It is a
//! *soft* reference, not a foreign key: `templates.code` is not globally
//! unique (a Project may override a shared `code`; see
//! `store::templates`), so a hard FK would be wrong. The seed loader
//! resolves it by string match.

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
                    .table(Products::Table)
                    .if_not_exists()
                    .comment(
                        "Product catalog — one row per firm product, the single \
                         source of truth for its list price. A discount is a \
                         recorded override on the engagement, never a second row.",
                    )
                    .col(
                        ColumnDef::new(Products::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this product row."),
                    )
                    .col(
                        ColumnDef::new(Products::Code)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment(
                                "Stable product key (`northstar`, `nest`, `nexus`, \
                                 `nautilus`, `litigation`) — the marketing/Xero \
                                 identity. NOT a template prefix.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Products::DisplayName)
                            .string()
                            .not_null()
                            .comment("Human-facing product name (e.g. `Neon Law Nexus`)."),
                    )
                    .col(
                        ColumnDef::new(Products::ListPriceCents)
                            .big_integer()
                            .not_null()
                            .comment(
                                "List price in minor units (cents). No float. For an \
                                 hourly product this is the hourly rate in cents.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Products::Currency)
                            .string()
                            .not_null()
                            .comment("ISO 4217 currency code (e.g., `USD`)."),
                    )
                    .col(
                        ColumnDef::new(Products::Cadence)
                            .string()
                            .not_null()
                            .comment("Billing cadence: `once` | `monthly` | `yearly` | `hourly`."),
                    )
                    .col(
                        ColumnDef::new(Products::BillingKind)
                            .string()
                            .not_null()
                            .comment(
                                "How the price is billed: `matter_close_flat` (a flat \
                                 fee raised when the matter closes), `recurring`, or \
                                 `hourly`. Only `matter_close_flat` products raise a \
                                 matter-close fee.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Products::Active)
                            .boolean()
                            .not_null()
                            .default(true)
                            .comment("Whether the product is currently offered."),
                    )
                    .col(ColumnDef::new(Products::XeroItemCode).string().comment(
                        "Optional Xero `ItemCode` mirror, so invoice lines can \
                             reference a Xero Item. `None` until mirrored.",
                    ))
                    .col(
                        ColumnDef::new(Products::MatterCloseTemplateCode)
                            .string()
                            .comment(
                                "The originating template `code` whose matter-close \
                                 raises this product's flat fee (e.g. \
                                 `onboarding__estate` for Northstar). Soft reference, \
                                 not a FK — `templates.code` is not globally unique. \
                                 `None` for products with no matter-close flat fee.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Products::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Products::UpdatedAt)
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
            .drop_table(Table::drop().table(Products::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Id,
    Code,
    DisplayName,
    ListPriceCents,
    Currency,
    Cadence,
    BillingKind,
    Active,
    XeroItemCode,
    MatterCloseTemplateCode,
    InsertedAt,
    UpdatedAt,
}
