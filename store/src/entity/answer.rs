//! `answers` table — one respondent's answer to one question.
//!
//! Answers are **append-only**: re-asks (verification) and corrections are
//! new rows, never updates, and latest-per-`(notation_id, state_name)`
//! wins on read. There is no unique constraint.
//!
//! `person_id` is the **respondent** (whose answer it is). `source` and
//! `authored_by_person_id` record *who supplied it* — staff filling it in
//! on the client's behalf, or the client themselves through the magic
//! link — so a two-sided intake can interleave both authorships on one
//! notation and the data lake can tell them apart.
//!
//! `notation_id` scopes the answer to the Notation that collected it, and
//! `state_name` carries the full `<type>__<role>` questionnaire state
//! (`entity__company`, `entity__subsidiary`) so two records of the same
//! type stay distinct — the bare `question_id` alone would collapse them.
//! `value` is JSONB: primitives as `{"value": …}`, singular record answers
//! mirror the row they create/select, aggregates an array of that shape.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

/// `answers.source` — staff entered the answer on the client's behalf.
pub const SOURCE_STAFF: &str = "staff";
/// `answers.source` — the client self-entered the answer (magic link).
pub const SOURCE_CLIENT: &str = "client";
/// `answers.source` — machine-extracted from a recorded sitting's
/// transcript (AIDA/Gemini), neither staff- nor client-typed. The
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
    /// The Notation that collected this answer. Null for the
    /// person-scoped canonical-seed fixtures (`Answer.yaml`), which have
    /// no Notation behind them; every Notation-bound write site sets it.
    pub notation_id: Option<Uuid>,
    /// Full `<type>__<role>` questionnaire state this answer was given for
    /// (`entity__company`). Null for bare/legacy/seed answers carrying no
    /// role. Render keys on this so two records of one type stay distinct.
    pub state_name: Option<String>,
    /// JSONB. Primitives are `{"value": …}`; singular record answers
    /// mirror the row they create/select; aggregates an array of that
    /// shape. Use [`primitive`]/[`primitive_str`]/[`display_value`].
    #[sea_orm(column_type = "JsonBinary")]
    pub value: Json,
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
    #[sea_orm(
        belongs_to = "super::notation::Entity",
        from = "Column::NotationId",
        to = "super::notation::Column::Id"
    )]
    Notation,
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

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

/// Wrap a scalar answer into the primitive JSON envelope `{"value": …}`.
#[must_use]
pub fn primitive(value: &str) -> Json {
    json!({ "value": value })
}

/// Read the inner string of a primitive envelope (`{"value": "…"}`),
/// `None` for a record/aggregate shape or a non-string `value`.
#[must_use]
pub fn primitive_str(value: &Json) -> Option<&str> {
    value.get("value").and_then(Json::as_str)
}

/// The string a template placeholder should render for this answer. A
/// primitive envelope unwraps to its inner scalar; a record/aggregate
/// shape (resolved by the evaluator) falls back to its compact JSON.
#[must_use]
pub fn display_value(value: &Json) -> String {
    match value.get("value") {
        Some(Json::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => value.to_string(),
    }
}

crate::uuid_active_model_behavior!();
