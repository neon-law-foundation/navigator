//! `notation_clauses` — one custom paragraph added to a single notation's
//! assembled document, without forking the shared template.
//!
//! Staff add ad-hoc prose to *this matter's* engagement document; the
//! assembled PDF splices these clauses at the template body's
//! `{{custom_clauses}}` marker, in `position` order. `authored_by_person_id`
//! records the staff author (analytics dimension for the data lake). See
//! [`crate::notation_clauses`] for the helpers.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "notation_clauses")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::notation`] — the matter this clause is on.
    pub notation_id: Uuid,
    /// Render order within the notation, ascending.
    pub position: i32,
    /// The clause prose (markdown), as the attorney reviews it.
    pub body_markdown: String,
    /// FK → [`super::person`] — the staff member who added the clause.
    pub authored_by_person_id: Option<Uuid>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::notation::Entity",
        from = "Column::NotationId",
        to = "super::notation::Column::Id"
    )]
    Notation,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
