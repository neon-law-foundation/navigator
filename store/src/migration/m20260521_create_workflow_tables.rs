//! `templates`, `questions`, `answers`, `notations` —
//! the Notation system core. See glossary terms
//! [Template](../../../docs/notation.md#template),
//! [Question](../../../docs/notation.md#question),
//! [Answer](../../../docs/notation.md#answer), and
//! [Notation](../../../docs/notation.md#notation).

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
                    .table(Templates::Table)
                    .if_not_exists()
                    .comment(
                        "Template — the static markdown + frontmatter blueprint a \
                         Notation runs against. See docs/notation.md#template.",
                    )
                    .col(
                        ColumnDef::new(Templates::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Template."),
                    )
                    .col(
                        ColumnDef::new(Templates::Code)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Stable code that identifies this Template (e.g., `llc-california`)."),
                    )
                    .col(
                        ColumnDef::new(Templates::Title)
                            .string()
                            .not_null()
                            .comment("Human-readable title of the Template."),
                    )
                    .col(
                        ColumnDef::new(Templates::RespondentType)
                            .string()
                            .not_null()
                            .comment("`entity`, `person`, or `person_and_entity` — who the Template is bound to."),
                    )
                    .col(
                        ColumnDef::new(Templates::Body)
                            .text()
                            .not_null()
                            .comment("Markdown body with `{{question_code}}` placeholders (frontmatter stripped)."),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Questions::Table)
                    .if_not_exists()
                    .comment(
                        "Question — one prompt presented to a respondent during \
                         Template traversal. See docs/notation.md#question.",
                    )
                    .col(
                        ColumnDef::new(Questions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Question."),
                    )
                    .col(
                        ColumnDef::new(Questions::Code)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("Stable code for this Question (e.g., `client_name`)."),
                    )
                    .col(
                        ColumnDef::new(Questions::Prompt)
                            .text()
                            .not_null()
                            .comment("Human-readable prompt shown to the respondent."),
                    )
                    .col(
                        ColumnDef::new(Questions::AnswerType)
                            .string()
                            .not_null()
                            .comment("Answer shape — `string`, `int`, `bool`, `choice`, …"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Answers::Table)
                    .if_not_exists()
                    .comment(
                        "Answer — one respondent's answer to one Question, \
                         deduplicated by (question, person, value). \
                         See docs/notation.md#answer.",
                    )
                    .col(
                        ColumnDef::new(Answers::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Answer."),
                    )
                    .col(
                        ColumnDef::new(Answers::QuestionId)
                            .uuid()
                            .not_null()
                            .comment("FK → Question (`questions.id`)."),
                    )
                    .col(
                        ColumnDef::new(Answers::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) who gave the Answer."),
                    )
                    .col(
                        ColumnDef::new(Answers::Value)
                            .text()
                            .not_null()
                            .comment("Captured answer value (typed per `questions.answer_type`)."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_answers_question")
                            .from(Answers::Table, Answers::QuestionId)
                            .to(Questions::Table, Questions::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_answers_person")
                            .from(Answers::Table, Answers::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Notations::Table)
                    .if_not_exists()
                    .comment(
                        "Notation — one running instance of a Template bound to a \
                         Person (respondent) and optionally an Entity. The unit of \
                         legal work. See docs/notation.md#notation.",
                    )
                    .col(
                        ColumnDef::new(Notations::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this Notation."),
                    )
                    .col(
                        ColumnDef::new(Notations::TemplateId)
                            .uuid()
                            .not_null()
                            .comment("FK → Template (`templates.id`)."),
                    )
                    .col(
                        ColumnDef::new(Notations::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — the respondent."),
                    )
                    .col(ColumnDef::new(Notations::EntityId).uuid().null().comment(
                        "FK → Entity (`entities.id`), nullable for person-only Notations.",
                    ))
                    .col(
                        ColumnDef::new(Notations::State)
                            .string()
                            .not_null()
                            .comment(
                                "Current workflow State — `draft`, `staff_review`, \
                                 `signed`, … Mirrors the latest `notation_events.to_state`.",
                            ),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notations_template")
                            .from(Notations::Table, Notations::TemplateId)
                            .to(Templates::Table, Templates::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notations_person")
                            .from(Notations::Table, Notations::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notations_entity")
                            .from(Notations::Table, Notations::EntityId)
                            .to(Entities::Table, Entities::Id),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Notations::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Answers::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Questions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Templates::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Templates {
    Table,
    Id,
    Code,
    Title,
    RespondentType,
    Body,
}

#[derive(DeriveIden)]
enum Questions {
    Table,
    Id,
    Code,
    Prompt,
    AnswerType,
}

#[derive(DeriveIden)]
enum Answers {
    Table,
    Id,
    QuestionId,
    PersonId,
    Value,
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    Id,
    TemplateId,
    PersonId,
    EntityId,
    State,
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
