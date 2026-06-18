//! `relationship_logs` — append-only audit trail of relationship
//! changes (`person joined entity`, `project closed`, …). Source
//! of truth for "what changed when" outside of normal table rows.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "relationship_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub actor_person_id: Option<Uuid>,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub action: String,
    pub detail: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
