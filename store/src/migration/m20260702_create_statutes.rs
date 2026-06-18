//! `statutes` + `statute_revisions` — the public legal-code reference.
//!
//! `statutes` is a thin identity row per `(code, section)`; it holds no
//! text, only the official-source link, a `status` flag, and bookkeeping
//! dates. `statute_revisions` is append-only and immutable: one row per
//! distinct normalized text ever observed for a section, written only by
//! `INSERT`. "Current" is the latest revision, derived at read time —
//! there is deliberately no `observed_until` interval to keep in sync.
//! See `prompts/nrs-statute-scraper-design.md`.

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
                    .table(Statutes::Table)
                    .if_not_exists()
                    .comment(
                        "Statute — stable identity row for one section of a published \
                         legal code (e.g. NRS 86.011). Holds no statute text; the text \
                         lives in append-only statute_revisions. Public reference.",
                    )
                    .col(
                        ColumnDef::new(Statutes::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this section (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(Statutes::Jurisdiction)
                            .string()
                            .not_null()
                            .comment("Jurisdiction the code belongs to (e.g. `NV`)."),
                    )
                    .col(
                        ColumnDef::new(Statutes::Code)
                            .string()
                            .not_null()
                            .comment("Code abbreviation (e.g. `NRS`)."),
                    )
                    .col(
                        ColumnDef::new(Statutes::Chapter)
                            .string()
                            .not_null()
                            .comment("Chapter the section lives in (`86`, `118A`)."),
                    )
                    .col(
                        ColumnDef::new(Statutes::ChapterTitle)
                            .string()
                            .not_null()
                            .comment("Human-readable chapter title."),
                    )
                    .col(
                        ColumnDef::new(Statutes::Section)
                            .string()
                            .not_null()
                            .comment("Section number as the source prints it (`86.011`)."),
                    )
                    .col(
                        ColumnDef::new(Statutes::SourceUrl)
                            .string()
                            .not_null()
                            .comment("Permalink to the section on the official source."),
                    )
                    .col(
                        ColumnDef::new(Statutes::Status)
                            .string()
                            .not_null()
                            .default("active")
                            .comment("`active` while published, `repealed` once it vanishes."),
                    )
                    .col(
                        ColumnDef::new(Statutes::FirstSeenAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 run date the section was first observed."),
                    )
                    .col(
                        ColumnDef::new(Statutes::LastCheckedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 run date of the most recent run that saw it."),
                    )
                    .col(
                        ColumnDef::new(Statutes::LastChangedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 run date the body last changed."),
                    )
                    .col(
                        ColumnDef::new(Statutes::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(Statutes::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .check(Expr::col(Statutes::Status).is_in(["active", "repealed"]))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_statutes_code_section")
                    .table(Statutes::Table)
                    .col(Statutes::Code)
                    .col(Statutes::Section)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_statutes_code_chapter")
                    .table(Statutes::Table)
                    .col(Statutes::Code)
                    .col(Statutes::Chapter)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(StatuteRevisions::Table)
                    .if_not_exists()
                    .comment(
                        "Statute revision — append-only, immutable. One row per distinct \
                         normalized text ever observed for a section. Rows are only ever \
                         INSERTed; current = the greatest observed_at per statute_id.",
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this revision (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::StatuteId)
                            .uuid()
                            .not_null()
                            .comment("FK → Statute (`statutes.id`) — the section identity."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::Body)
                            .text()
                            .not_null()
                            .comment("The section's verbatim display text as observed."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::BodySha256)
                            .string()
                            .not_null()
                            .comment("SHA-256 of the normalized body — change-detection key."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::SectionTitle)
                            .string()
                            .not_null()
                            .comment("Section heading as observed in this revision."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::HistoryNote)
                            .string()
                            .null()
                            .comment("Legislature's amendment tail, verbatim; null if none."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::ObservedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 run date this text was first seen."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(StatuteRevisions::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_statute_revisions_statute")
                            .from(StatuteRevisions::Table, StatuteRevisions::StatuteId)
                            .to(Statutes::Table, Statutes::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_statute_revisions_statute_observed")
                    .table(StatuteRevisions::Table)
                    .col(StatuteRevisions::StatuteId)
                    .col(StatuteRevisions::ObservedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_statute_revisions_statute_observed")
                    .table(StatuteRevisions::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(StatuteRevisions::Table).to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_statutes_code_chapter")
                    .table(Statutes::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_statutes_code_section")
                    .table(Statutes::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Statutes::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Statutes {
    Table,
    Id,
    Jurisdiction,
    Code,
    Chapter,
    ChapterTitle,
    Section,
    SourceUrl,
    Status,
    FirstSeenAt,
    LastCheckedAt,
    LastChangedAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum StatuteRevisions {
    Table,
    Id,
    StatuteId,
    Body,
    BodySha256,
    SectionTitle,
    HistoryNote,
    ObservedAt,
    InsertedAt,
    UpdatedAt,
}
