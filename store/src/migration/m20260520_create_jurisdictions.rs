//! `jurisdictions` — see glossary term
//! [Jurisdiction](../../../docs/glossary.md#jurisdiction).
//!
//! A US state, federal jurisdiction, or foreign jurisdiction that an
//! Entity can be organized under, or that a Credential is issued by.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Jurisdictions::Table)
                    .if_not_exists()
                    .comment(
                        "Jurisdiction — US state, federal, or foreign jurisdiction. \
                         See docs/glossary.md#jurisdiction.",
                    )
                    .col(
                        ColumnDef::new(Jurisdictions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Jurisdiction."),
                    )
                    .col(
                        ColumnDef::new(Jurisdictions::Name)
                            .string()
                            .not_null()
                            .comment("Long name of the Jurisdiction (e.g., `Nevada`)."),
                    )
                    .col(
                        ColumnDef::new(Jurisdictions::Code)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Short code for the Jurisdiction (e.g., `NV`, `CA`, `US`)."),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Jurisdictions::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Jurisdictions {
    Table,
    Id,
    Name,
    Code,
}
