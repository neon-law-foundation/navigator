//! Add project-linked testimonials for public marketing proof.
//!
//! A Testimonial always belongs to one Project and one Person. Publication
//! is explicit: website reads require both `consented_at` and `published_at`
//! so a retroactive matter note cannot leak onto the public site by merely
//! existing in the database.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .add_column(
                        ColumnDef::new(Persons::ProfileImageUrl)
                            .string()
                            .null()
                            .comment(
                                "Optional public profile image URL for this Person. \
                                 Used for consented testimonial attribution and contact cards.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Testimonials::Table)
                    .if_not_exists()
                    .comment(
                        "Testimonial — consented public quote linked to one Project \
                         and the Person who sent it.",
                    )
                    .col(
                        ColumnDef::new(Testimonials::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Testimonial."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::ProjectId)
                            .uuid()
                            .not_null()
                            .comment("FK → Project (`projects.id`) this testimonial came from."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) who sent the testimonial."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::ProductCode)
                            .string()
                            .null()
                            .comment(
                                "Optional Product code (`nexus`, `litigation`, …) that \
                                 controls product-page placement.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Testimonials::Quote)
                            .text()
                            .not_null()
                            .comment("The testimonial body, as approved for publication."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::AttributionLabel)
                            .string()
                            .null()
                            .comment(
                                "Optional public attribution override when the Person's \
                                 database name is not the approved display text.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Testimonials::ConsentedAt)
                            .string()
                            .null()
                            .comment(
                                "RFC 3339 timestamp when the sender consented to public use. \
                                 Required for website display.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Testimonials::PublishedAt)
                            .string()
                            .null()
                            .comment(
                                "RFC 3339 timestamp when staff approved website publication. \
                                 Required for website display.",
                            ),
                    )
                    .col(
                        ColumnDef::new(Testimonials::DisplayOrder)
                            .integer()
                            .not_null()
                            .default(0)
                            .comment("Lower numbers render first within a testimonial list."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Testimonials::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was last updated."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_testimonials_project")
                            .from(Testimonials::Table, Testimonials::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_testimonials_person")
                            .from(Testimonials::Table, Testimonials::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_testimonials_product")
                            .from(Testimonials::Table, Testimonials::ProductCode)
                            .to(Products::Table, Products::Code),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_testimonials_public_product")
                    .table(Testimonials::Table)
                    .col(Testimonials::ProductCode)
                    .col(Testimonials::PublishedAt)
                    .col(Testimonials::DisplayOrder)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_testimonials_project")
                    .table(Testimonials::Table)
                    .col(Testimonials::ProjectId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Testimonials::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::ProfileImageUrl)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Testimonials {
    Table,
    Id,
    ProjectId,
    PersonId,
    ProductCode,
    Quote,
    AttributionLabel,
    ConsentedAt,
    PublishedAt,
    DisplayOrder,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
    ProfileImageUrl,
}

#[derive(DeriveIden)]
enum Products {
    Table,
    Code,
}
