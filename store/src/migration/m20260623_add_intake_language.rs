//! Make language access in intake explicit.
//!
//! Two additions so a client can complete the questionnaire in their
//! own language, not only English:
//!
//! - `persons.preferred_language` — the BCP-47 locale (`en`, `es`, …)
//!   the questionnaire renders in for this person. Defaults to `en`, so
//!   every existing person keeps the English experience unchanged.
//! - `question_translations` — attorney-reviewed localized variants of a
//!   Question's `prompt` (and optional `help_text`), keyed by
//!   `(question_id, locale)`. The base `questions.prompt` stays the `en`
//!   default; a missing translation falls back to it. Translation is a
//!   table of reviewed copy, **not** runtime machine translation — the
//!   `staff_review` gate and the legal copy stay attorney-reviewed in
//!   each language.

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
                        ColumnDef::new(Persons::PreferredLanguage)
                            .string()
                            .not_null()
                            .default("en")
                            .comment(
                                "BCP-47 locale the questionnaire renders in for this person \
                                 (e.g. `en`, `es`). Defaults to `en`.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(QuestionTranslations::Table)
                    .if_not_exists()
                    .comment(
                        "Question Translation — an attorney-reviewed localized variant of a \
                         Question's prompt/help_text for one locale. Falls back to the base \
                         English `questions.prompt` when absent.",
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this translation."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::QuestionId)
                            .uuid()
                            .not_null()
                            .comment("FK → Question (`questions.id`)."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::Locale)
                            .string()
                            .not_null()
                            .comment("BCP-47 locale of this variant (e.g. `es`)."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::Prompt)
                            .text()
                            .not_null()
                            .comment("Localized question prompt (attorney-reviewed)."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::HelpText)
                            .text()
                            .null()
                            .comment("Localized help text, if any."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(QuestionTranslations::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_question_translations_question")
                            .from(
                                QuestionTranslations::Table,
                                QuestionTranslations::QuestionId,
                            )
                            .to(Questions::Table, Questions::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // One translation per (question, locale).
        manager
            .create_index(
                Index::create()
                    .name("idx_question_translations_question_locale")
                    .table(QuestionTranslations::Table)
                    .col(QuestionTranslations::QuestionId)
                    .col(QuestionTranslations::Locale)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(QuestionTranslations::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::PreferredLanguage)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    PreferredLanguage,
}

#[derive(DeriveIden)]
enum QuestionTranslations {
    Table,
    Id,
    QuestionId,
    Locale,
    Prompt,
    HelpText,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Questions {
    Table,
    Id,
}
