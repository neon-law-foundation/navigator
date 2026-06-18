//! `share_issuances` — see glossary term
//! [Share Issuance](../../../docs/glossary.md#share-issuance).
//!
//! One row per issuance event (Entity X issued N shares of `<class>`
//! to `<holder>` on `<date>`). The cap-table admin view aggregates
//! by `holder_name` to compute the ownership breakdown for a given
//! Entity.
//!
//! Holder identity is denormalized into a `holder_name` string so
//! the table can record issuances to people, entities, or external
//! parties (a trust, an estate) without a polymorphic foreign-key
//! escape hatch.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ShareIssuances::Table)
                    .if_not_exists()
                    .comment(
                        "Share Issuance — one issuance event for an Entity (holder + \
                         class + share count + date). See docs/glossary.md#share-issuance.",
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Share Issuance."),
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::EntityId)
                            .uuid()
                            .not_null()
                            .comment("FK → Entity (`entities.id`) issuing the shares."),
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::HolderName)
                            .string()
                            .not_null()
                            .comment("Holder display name (Person, Entity, trust, estate, …)."),
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::ShareClass)
                            .string()
                            .not_null()
                            .default("common")
                            .comment("Share class (e.g., `common`, `preferred_a`)."),
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::Shares)
                            .big_integer()
                            .not_null()
                            .comment("Number of shares issued (count; not a UUID)."),
                    )
                    .col(
                        ColumnDef::new(ShareIssuances::IssuedAt)
                            .string()
                            .not_null()
                            .comment("ISO 8601 date string (YYYY-MM-DD) of the issuance."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_share_issuances_entity")
                            .from(ShareIssuances::Table, ShareIssuances::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_share_issuances_entity")
                    .table(ShareIssuances::Table)
                    .col(ShareIssuances::EntityId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ShareIssuances::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ShareIssuances {
    Table,
    Id,
    EntityId,
    HolderName,
    ShareClass,
    Shares,
    IssuedAt,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}
