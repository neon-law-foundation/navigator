//! `statute_revisions` table — append-only, immutable history of one
//! section's text.
//!
//! One row per *distinct* normalized text ever observed for a section.
//! Rows are only ever **inserted** — never updated, never deleted — so
//! "immutable" is a structural guarantee rather than a claim. "Current"
//! is the row with the greatest `observed_at` for a given `statute_id`;
//! there is no stored end-of-interval to keep in sync. See
//! `prompts/nrs-statute-scraper-design.md`.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "statute_revisions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::statute`] (`statutes.id`) — the section identity.
    pub statute_id: Uuid,
    /// The section's verbatim text as observed (normalized for hashing
    /// upstream, but stored as the cleaned display body).
    #[sea_orm(column_type = "Text")]
    pub body: String,
    /// SHA-256 of the normalized body — the change-detection key.
    pub body_sha256: String,
    /// The section heading as observed in this revision (`Definitions.`).
    pub section_title: String,
    /// The legislature's amendment tail, verbatim
    /// (`(Added to NRS by 1991, 1293; A 1997, 715)`). `None` when the
    /// source carries no annotation for the section.
    #[sea_orm(nullable)]
    pub history_note: Option<String>,
    /// Run date (RFC 3339) this text was first seen.
    pub observed_at: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// Each revision belongs to one section.
    #[sea_orm(
        belongs_to = "super::statute::Entity",
        from = "Column::StatuteId",
        to = "super::statute::Column::Id"
    )]
    Statute,
}

impl Related<super::statute::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Statute.def()
    }
}

crate::uuid_active_model_behavior!();
