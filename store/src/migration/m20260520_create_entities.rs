//! `entities` — see glossary term
//! [Entity](../../../docs/glossary.md#entity).
//!
//! A legal organization (LLC, trust, corporation, foundation, …)
//! with a type and a jurisdiction.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Entities::Table)
                    .if_not_exists()
                    .comment(
                        "Entity — a legal organization (LLC, trust, corporation, …). \
                         See docs/glossary.md#entity.",
                    )
                    .col(
                        ColumnDef::new(Entities::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Entity."),
                    )
                    .col(
                        ColumnDef::new(Entities::Name)
                            .string()
                            .not_null()
                            .comment("Display name of the Entity."),
                    )
                    .col(
                        ColumnDef::new(Entities::EntityTypeId)
                            .uuid()
                            .not_null()
                            .comment("FK → Entity Type (`entity_types.id`)."),
                    )
                    .col(
                        ColumnDef::new(Entities::JurisdictionId)
                            .uuid()
                            .not_null()
                            .comment("FK → Jurisdiction (`jurisdictions.id`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_entities_entity_type")
                            .from(Entities::Table, Entities::EntityTypeId)
                            .to(EntityTypes::Table, EntityTypes::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_entities_jurisdiction")
                            .from(Entities::Table, Entities::JurisdictionId)
                            .to(Jurisdictions::Table, Jurisdictions::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Entities::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
    Name,
    EntityTypeId,
    JurisdictionId,
}

#[derive(DeriveIden)]
enum EntityTypes {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Jurisdictions {
    Table,
    Id,
}
