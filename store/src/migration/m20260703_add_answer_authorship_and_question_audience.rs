//! Record *who entered* each answer and *which side of the intake* each
//! question is for — the data-model half of the mutable, two-sided
//! intake.
//!
//! - `answers.source` — `staff` | `client`: who supplied the answer. The
//!   existing `answers.person_id` stays the **respondent** (whose answer
//!   it is); this column is the **authorship dimension**. NOT NULL with a
//!   `staff` default because every answer that predates this column was
//!   typed by staff through the admin walker, and because a never-null,
//!   low-cardinality string is exactly what the nightly Postgres→Parquet
//!   snapshot (`archives`) wants as a `GROUP BY` dimension in the data
//!   lake — a nullable column or a boolean would lose meaning there.
//! - `answers.authored_by_person_id` — who actually *typed* the answer
//!   (the staff member on behalf of the client, or the client
//!   themselves). Nullable: legacy rows and system-supplied answers have
//!   no typist. FK → persons.
//! - `questions.audience` — `staff` | `client` | `both`: which side sees
//!   the question. Drives which questions the client magic-link shows.
//!   NOT NULL, default `both` — every existing question is shown to
//!   whoever is walking the notation until an author narrows it. Kept as
//!   data (a column), never a code branch per product.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Answers::Table)
                    .add_column(
                        ColumnDef::new(Answers::Source)
                            .string()
                            .not_null()
                            .default("staff")
                            .comment(
                                "Who supplied this answer: `staff` (entered on the client's \
                                 behalf) or `client` (self-entered). Analytics dimension; \
                                 person_id remains the respondent.",
                            ),
                    )
                    .add_column(
                        ColumnDef::new(Answers::AuthoredByPersonId)
                            .uuid()
                            .null()
                            .comment(
                                "FK → persons.id — who actually typed this answer (a staff \
                                 member or the client). Null for legacy/system answers.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_answers_authored_by_person")
                    .from(Answers::Table, Answers::AuthoredByPersonId)
                    .to(Persons::Table, Persons::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_answers_authored_by_person_id")
                    .table(Answers::Table)
                    .col(Answers::AuthoredByPersonId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Questions::Table)
                    .add_column(
                        ColumnDef::new(Questions::Audience)
                            .string()
                            .not_null()
                            .default("both")
                            .comment(
                                "Which side of the intake sees this question: `staff`, \
                                 `client`, or `both`. Filters the client magic-link's \
                                 question set.",
                            ),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Questions::Table)
                    .drop_column(Questions::Audience)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_answers_authored_by_person_id")
                    .table(Answers::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_answers_authored_by_person")
                    .table(Answers::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Answers::Table)
                    .drop_column(Answers::Source)
                    .drop_column(Answers::AuthoredByPersonId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Answers {
    Table,
    Source,
    AuthoredByPersonId,
}

#[derive(DeriveIden)]
enum Questions {
    Table,
    Audience,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
