//! `persons` — see glossary term [Person](../../../docs/glossary.md#person).
//!
//! A human contact. Authorization roles live on this row (in the
//! `roles` JSON column added by a later migration), not on the OIDC
//! token: the IdP carries only `sub` + `email`, and the callback
//! handler links that pair to a `persons` row via `oidc_subject`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Persons::Table)
                    .if_not_exists()
                    .comment("Person — a human contact. See docs/glossary.md#person.")
                    .col(
                        ColumnDef::new(Persons::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Person."),
                    )
                    .col(
                        ColumnDef::new(Persons::Name)
                            .string()
                            .not_null()
                            .comment("Display name of the Person."),
                    )
                    .col(
                        ColumnDef::new(Persons::Email)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Unique contact email for the Person."),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Persons::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
    Name,
    Email,
}
