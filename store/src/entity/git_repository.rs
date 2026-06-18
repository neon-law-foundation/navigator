//! `git_repositories` — repositories that carry imported notation
//! content. The CLI's `navigator import` writes the repo+SHA here.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "git_repositories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Hash of `git remote get-url origin`.
    #[sea_orm(unique)]
    pub remote_hash: String,
    /// Last imported commit SHA.
    pub last_commit_sha: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
