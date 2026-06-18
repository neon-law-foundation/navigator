//! `playbooks` — a client Entity's stored contract-negotiation positions.
//!
//! Scoped to the client **Entity** (the company), so one playbook serves
//! every matter for that client. `positions` is a JSONB array edited as a
//! whole through the admin playbook surface; the typed view of a position
//! is [`crate::playbooks::Position`]. See
//! [`m20260721_create_contract_review_tables`](super::super::migration) and
//! [`crate::playbooks`] for the helpers.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

// NB: no `Eq` derive — `Json` (serde_json::Value) is `PartialEq` but not
// `Eq` (it can hold an `f64`).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "playbooks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::entity`] — the client company this playbook belongs to.
    pub entity_id: Uuid,
    /// Human label for the playbook (e.g. `SaaS vendor MSA`). Unique per
    /// Entity.
    pub name: String,
    /// JSONB array of positions: `{topic, preferred, fallback, walkaway,
    /// severity}`. Typed view: [`crate::playbooks::Position`].
    #[sea_orm(column_type = "JsonBinary")]
    pub positions: Json,
    /// Whether this playbook is currently applied to new reviews.
    pub active: bool,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::entity::Entity",
        from = "Column::EntityId",
        to = "super::entity::Column::Id"
    )]
    Entity,
    #[sea_orm(has_many = "super::contract_review::Entity")]
    ContractReview,
}

impl Related<super::entity::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Entity.def()
    }
}

impl Related<super::contract_review::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ContractReview.def()
    }
}

crate::uuid_active_model_behavior!();
