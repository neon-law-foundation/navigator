//! `question_translations` table — an attorney-reviewed localized
//! variant of a Question's prompt (and optional help text) for one
//! locale.
//!
//! The base [`super::question`] row carries the English (`en`) prompt;
//! a `question_translations` row supplies the same question's prompt in
//! another locale (`es`, …), keyed `(question_id, locale)`. The
//! questionnaire renders the variant matching the person's
//! `persons.preferred_language`, falling back to the English base when
//! no translation exists. Reviewed copy, not runtime machine
//! translation — see `m20260623_add_intake_language`.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "question_translations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::question`].
    pub question_id: Uuid,
    /// BCP-47 locale of this variant (e.g. `es`).
    pub locale: String,
    /// Localized question prompt (attorney-reviewed).
    pub prompt: String,
    /// Localized help text, if any.
    pub help_text: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::question::Entity",
        from = "Column::QuestionId",
        to = "super::question::Column::Id"
    )]
    Question,
}

impl Related<super::question::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Question.def()
    }
}

crate::uuid_active_model_behavior!();
