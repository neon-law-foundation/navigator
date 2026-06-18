//! `share_issuances` table — one row per issuance event (entity X
//! issued N shares of <class> to <holder>). The cap-table admin
//! view aggregates by `holder_name` to compute the ownership
//! breakdown for a given entity.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "share_issuances")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub entity_id: Uuid,
    pub holder_name: String,
    pub share_class: String,
    pub shares: i64,
    /// ISO 8601 date string (YYYY-MM-DD). Stored as text so the
    /// entity stays portable across SQLite (no native date type)
    /// and Postgres without pulling chrono behind a feature flag.
    pub issued_at: String,
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
}

impl Related<super::entity::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Entity.def()
    }
}

crate::uuid_active_model_behavior!();
