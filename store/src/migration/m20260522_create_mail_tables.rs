//! `addresses`, `mailrooms`, `letters` — the mail layer. See
//! glossary terms [Address](../../../docs/glossary.md#address),
//! [Mailroom](../../../docs/glossary.md#mailroom), and
//! [Letter](../../../docs/glossary.md#letter).

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
                    .table(Addresses::Table)
                    .if_not_exists()
                    .comment(
                        "Address — a postal address attached (XOR) to a Person or \
                         an Entity. See docs/glossary.md#address.",
                    )
                    .col(
                        ColumnDef::new(Addresses::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Address."),
                    )
                    .col(
                        ColumnDef::new(Addresses::PersonId).uuid().null().comment(
                            "FK → Person (`persons.id`), null for entity-owned Addresses.",
                        ),
                    )
                    .col(
                        ColumnDef::new(Addresses::EntityId).uuid().null().comment(
                            "FK → Entity (`entities.id`), null for person-owned Addresses.",
                        ),
                    )
                    .col(
                        ColumnDef::new(Addresses::Line1)
                            .string()
                            .not_null()
                            .comment("Street line 1."),
                    )
                    .col(
                        ColumnDef::new(Addresses::Line2)
                            .string()
                            .null()
                            .comment("Optional street line 2 (apt / suite)."),
                    )
                    .col(
                        ColumnDef::new(Addresses::City)
                            .string()
                            .not_null()
                            .comment("City."),
                    )
                    .col(
                        ColumnDef::new(Addresses::Region)
                            .string()
                            .not_null()
                            .comment("State / region code."),
                    )
                    .col(
                        ColumnDef::new(Addresses::PostalCode)
                            .string()
                            .not_null()
                            .comment("Postal / ZIP code."),
                    )
                    .col(
                        ColumnDef::new(Addresses::Country)
                            .string()
                            .not_null()
                            .comment("ISO country code."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_addresses_person")
                            .from(Addresses::Table, Addresses::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_addresses_entity")
                            .from(Addresses::Table, Addresses::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Mailrooms::Table)
                    .if_not_exists()
                    .comment(
                        "Mailroom — a named physical mail-receiving destination \
                         (an Address with a label). See docs/glossary.md#mailroom.",
                    )
                    .col(
                        ColumnDef::new(Mailrooms::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Mailroom."),
                    )
                    .col(
                        ColumnDef::new(Mailrooms::Name)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Display name of the Mailroom."),
                    )
                    .col(
                        ColumnDef::new(Mailrooms::AddressId)
                            .uuid()
                            .not_null()
                            .comment("FK → Address (`addresses.id`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_mailrooms_address")
                            .from(Mailrooms::Table, Mailrooms::AddressId)
                            .to(Addresses::Table, Addresses::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Letters::Table)
                    .if_not_exists()
                    .comment(
                        "Letter — one piece of mail (incoming or outgoing), scoped \
                         to a Mailroom. See docs/glossary.md#letter.",
                    )
                    .col(
                        ColumnDef::new(Letters::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Letter."),
                    )
                    .col(
                        ColumnDef::new(Letters::MailroomId)
                            .uuid()
                            .not_null()
                            .comment("FK → Mailroom (`mailrooms.id`)."),
                    )
                    .col(
                        ColumnDef::new(Letters::Direction)
                            .string()
                            .not_null()
                            .comment("`incoming` or `outgoing`."),
                    )
                    .col(
                        ColumnDef::new(Letters::Sender)
                            .string()
                            .not_null()
                            .comment("Sender display string."),
                    )
                    .col(
                        ColumnDef::new(Letters::Recipient)
                            .string()
                            .not_null()
                            .comment("Recipient display string."),
                    )
                    .col(
                        ColumnDef::new(Letters::Summary)
                            .text()
                            .not_null()
                            .comment("Short human-readable summary of the Letter."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_letters_mailroom")
                            .from(Letters::Table, Letters::MailroomId)
                            .to(Mailrooms::Table, Mailrooms::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Letters::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Mailrooms::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Addresses::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Addresses {
    Table,
    Id,
    PersonId,
    EntityId,
    Line1,
    Line2,
    City,
    Region,
    PostalCode,
    Country,
}

#[derive(DeriveIden)]
enum Mailrooms {
    Table,
    Id,
    Name,
    AddressId,
}

#[derive(DeriveIden)]
enum Letters {
    Table,
    Id,
    MailroomId,
    Direction,
    Sender,
    Recipient,
    Summary,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Entities {
    Table,
    Id,
}
