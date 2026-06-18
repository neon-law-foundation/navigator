#![allow(clippy::module_inception)]
//! `entities` table — a legal organization (LLC, trust, etc.) with
//! a type and a jurisdiction.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "entities")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub entity_type_id: Uuid,
    pub jurisdiction_id: Uuid,
    /// The organization's main phone line. `None` until set by the
    /// bulk-contact importer or an admin edit.
    #[sea_orm(nullable)]
    pub phone: Option<String>,
    /// The organization's canonical website URL (https). Canonicalized
    /// on the way in by the importer. `None` until set.
    #[sea_orm(nullable)]
    pub url: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::entity_type::Entity",
        from = "Column::EntityTypeId",
        to = "super::entity_type::Column::Id"
    )]
    EntityType,
    #[sea_orm(
        belongs_to = "super::jurisdiction::Entity",
        from = "Column::JurisdictionId",
        to = "super::jurisdiction::Column::Id"
    )]
    Jurisdiction,
}

impl Related<super::entity_type::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EntityType.def()
    }
}

impl Related<super::jurisdiction::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Jurisdiction.def()
    }
}

crate::uuid_active_model_behavior!();
