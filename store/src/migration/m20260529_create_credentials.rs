//! `credentials` — see glossary term
//! [Credential](../../../docs/glossary.md#credential).
//!
//! A Person's licensure in a Jurisdiction. Pairs a Person with a
//! Jurisdiction and a state-issued license number; the pair
//! `(person_id, jurisdiction_id)` is unique so the same attorney
//! can't be double-listed under one Jurisdiction.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Credentials::Table)
                    .if_not_exists()
                    .comment(
                        "Credential — a Person's licensure in a Jurisdiction \
                         (license number). See docs/glossary.md#credential.",
                    )
                    .col(
                        ColumnDef::new(Credentials::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Credential."),
                    )
                    .col(
                        ColumnDef::new(Credentials::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`)."),
                    )
                    .col(
                        ColumnDef::new(Credentials::JurisdictionId)
                            .uuid()
                            .not_null()
                            .comment("FK → Jurisdiction (`jurisdictions.id`)."),
                    )
                    .col(
                        ColumnDef::new(Credentials::LicenseNumber)
                            .string()
                            .not_null()
                            .comment(
                                "State-issued license number for the Person in this Jurisdiction.",
                            ),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_credentials_person")
                            .from(Credentials::Table, Credentials::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_credentials_jurisdiction")
                            .from(Credentials::Table, Credentials::JurisdictionId)
                            .to(Jurisdictions::Table, Jurisdictions::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_credentials_person_jurisdiction")
                    .table(Credentials::Table)
                    .col(Credentials::PersonId)
                    .col(Credentials::JurisdictionId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Credentials::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Credentials {
    Table,
    Id,
    PersonId,
    JurisdictionId,
    LicenseNumber,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Jurisdictions {
    Table,
    Id,
}
