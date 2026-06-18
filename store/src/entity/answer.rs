//! `answers` table — one respondent's answer to one question.
//! Answers are deduplicated by (question, person, value).
//!
//! `person_id` is the **respondent** (whose answer it is). `source` and
//! `authored_by_person_id` record *who supplied it* — staff filling it in
//! on the client's behalf, or the client themselves through the magic
//! link — so a two-sided intake can interleave both authorships on one
//! notation and the data lake can tell them apart.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// `answers.source` — staff entered the answer on the client's behalf.
pub const SOURCE_STAFF: &str = "staff";
/// `answers.source` — the client self-entered the answer (magic link).
pub const SOURCE_CLIENT: &str = "client";
/// `answers.source` — machine-extracted from a recorded sitting's
/// transcript (Ada/Gemini), neither staff- nor client-typed. The
/// distinct value is the human-in-the-loop boundary: a machine-proposed
/// answer is visibly different from a confirmed one, so an attorney can
/// see and correct it before any draft is released to the client.
pub const SOURCE_EXTRACTED: &str = "extracted";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "answers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub question_id: Uuid,
    pub person_id: Uuid,
    pub value: String,
    /// `staff` | `client` — who supplied this answer. A low-cardinality
    /// analytics dimension; never null (defaults `staff`).
    pub source: String,
    /// Who actually typed the answer (FK → persons). Null for
    /// legacy/system answers.
    pub authored_by_person_id: Option<Uuid>,
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
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
}

impl Related<super::question::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Question.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

crate::uuid_active_model_behavior!();
