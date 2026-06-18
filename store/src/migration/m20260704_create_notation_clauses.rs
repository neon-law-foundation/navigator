//! `notation_clauses` — per-notation custom prose a staff member adds to
//! *this matter's* engagement document before it is sent, without forking
//! the shared template.
//!
//! Additive, not a body clone: the assembled document renders the bound
//! template body and splices these clauses at its `{{custom_clauses}}`
//! marker, in `position` order. A dedicated table (rather than a
//! Project-scoped template override) keeps every custom clause one
//! analyzable row — who added it, when, on which notation — so the nightly
//! Postgres→Parquet snapshot can answer "how often do we depart from the
//! standard retainer, and on what?" in the data lake. A blob override
//! would bury that.
//!
//! The presence of any clause is also one half of the review gate: a
//! notation carrying custom prose must pass back through `staff_review`
//! before signature (see `web::retainer_walk`).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(NotationClauses::Table)
                    .if_not_exists()
                    .comment(
                        "NotationClause — one custom paragraph added to a single notation's \
                         assembled document, spliced at its {{custom_clauses}} marker.",
                    )
                    .col(
                        ColumnDef::new(NotationClauses::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this clause (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::NotationId)
                            .uuid()
                            .not_null()
                            .comment(
                                "FK → Notation (`notations.id`) — the matter this clause is on.",
                            ),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::Position)
                            .integer()
                            .not_null()
                            .comment("Render order within the notation, ascending."),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::BodyMarkdown)
                            .text()
                            .not_null()
                            .comment(
                                "The clause prose (markdown), as the attorney will review it.",
                            ),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::AuthoredByPersonId)
                            .uuid()
                            .null()
                            .comment("FK → persons.id — the staff member who added the clause."),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 creation timestamp."),
                    )
                    .col(
                        ColumnDef::new(NotationClauses::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 last-update timestamp."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notation_clauses_notation")
                            .from(NotationClauses::Table, NotationClauses::NotationId)
                            .to(Notations::Table, Notations::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notation_clauses_authored_by_person")
                            .from(NotationClauses::Table, NotationClauses::AuthoredByPersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_notation_clauses_notation_id_position")
                    .table(NotationClauses::Table)
                    .col(NotationClauses::NotationId)
                    .col(NotationClauses::Position)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NotationClauses::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum NotationClauses {
    Table,
    Id,
    NotationId,
    Position,
    BodyMarkdown,
    AuthoredByPersonId,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
