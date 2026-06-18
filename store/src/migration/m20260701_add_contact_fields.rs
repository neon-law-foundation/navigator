//! Add contact-directory fields used by the bulk-contact importer.
//!
//! - `persons.title` — the contact's role at their organization
//!   (e.g. "Executive Director"), free text.
//! - `persons.phone` — the contact's direct line.
//! - `entities.phone` — the organization's main switchboard line.
//! - `entities.url` — the organization's canonical website URL
//!   (https, canonicalized by the importer; see
//!   [`docs/bulk-contact-import.md`](../../../docs/bulk-contact-import.md)).
//!
//! All four are nullable: every existing person/entity row predates
//! these columns and must keep inserting unchanged. The importer is
//! the first writer; nothing reads them as required.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .add_column(
                        ColumnDef::new(Persons::Title)
                            .string()
                            .comment("The contact's role at their organization (free text)."),
                    )
                    .add_column(
                        ColumnDef::new(Persons::Phone)
                            .string()
                            .comment("The contact's direct phone line."),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Entities::Table)
                    .add_column(
                        ColumnDef::new(Entities::Phone)
                            .string()
                            .comment("The organization's main phone line."),
                    )
                    .add_column(
                        ColumnDef::new(Entities::Url)
                            .string()
                            .comment("The organization's canonical website URL (https)."),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::Title)
                    .drop_column(Persons::Phone)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Entities::Table)
                    .drop_column(Entities::Phone)
                    .drop_column(Entities::Url)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Title,
    Phone,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Phone,
    Url,
}
