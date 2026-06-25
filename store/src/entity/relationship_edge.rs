//! `relationship_edges` — a typed graph edge with a Person or Entity on
//! each end, traversed by the pre-matter conflict check.
//!
//! Unlike [`super::relationship_log`] (a one-sided audit trail), every
//! edge here connects two nodes — each a `person` or an `entity` — with
//! a typed `kind` between them. `store::conflicts` loads these rows into
//! an in-memory petgraph to decide whether opening a new matter would
//! conflict. See `m20260727_create_relationship_edges` for the schema
//! rationale.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Node kind for an edge endpoint: a row in `persons`.
pub const NODE_PERSON: &str = "person";
/// Node kind for an edge endpoint: a row in `entities`.
pub const NODE_ENTITY: &str = "entity";

/// Relationship kind: one node is legally adverse to the other (opposing
/// party in a dispute, counterparty turned hostile). The strongest
/// conflict signal — a confident `adverse_to` edge between the proposed
/// matter and an existing client blocks the open.
pub const KIND_ADVERSE_TO: &str = "adverse_to";
/// Relationship kind: the two nodes are related parties (family,
/// commonly-controlled entities, insiders) — a softer signal that
/// warrants staff review rather than a hard block.
pub const KIND_RELATED_PARTY: &str = "related_party";

/// Provenance: a human asserted this edge directly.
pub const SOURCE_MANUAL: &str = "manual";
/// Provenance: derived from a `disclosures` row.
pub const SOURCE_DISCLOSURE: &str = "disclosure";
/// Provenance: parsed from a `relationship_logs` entry.
pub const SOURCE_RELATIONSHIP_LOG: &str = "relationship_log";
/// Provenance: extracted from unstructured text by an LLM. These land
/// at lower `confidence_pct` and are always shown as such in findings.
pub const SOURCE_LLM: &str = "llm";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "relationship_edges")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub from_type: String,
    pub from_id: Uuid,
    pub to_type: String,
    pub to_id: Uuid,
    pub kind: String,
    pub confidence_pct: i32,
    pub source_kind: String,
    pub source_id: Option<Uuid>,
    pub detail: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
