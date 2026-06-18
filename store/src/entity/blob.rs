//! `blobs` table — opaque byte references (file uploads, signed
//! documents). The bytes themselves live in object storage; this
//! table holds the metadata + storage key.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "blobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Storage key returned by [`cloud::StorageService`].
    #[sea_orm(unique)]
    pub storage_key: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256_hex: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::document::Entity")]
    Document,
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

crate::uuid_active_model_behavior!();
