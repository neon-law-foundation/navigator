//! `entity_types` — see glossary term
//! [Entity Type](../../../docs/glossary.md#entity-type).
//!
//! Reference data — the kinds of legal Entity (LLC, Trust,
//! Corporation, Foundation, …) seeded from
//! `store/seeds/EntityType.yaml`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(EntityTypes::Table)
                    .if_not_exists()
                    .comment(
                        "Entity Type — the kind of legal Entity (LLC, Trust, …). \
                         See docs/glossary.md#entity-type.",
                    )
                    .col(
                        ColumnDef::new(EntityTypes::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Entity Type."),
                    )
                    .col(
                        ColumnDef::new(EntityTypes::Name)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Name of the Entity Type (e.g., `LLC`, `Trust`)."),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(EntityTypes::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum EntityTypes {
    Table,
    Id,
    Name,
}
