//! `credentials` table — a person's licensure in a jurisdiction.
//!
//! Each row pairs a `persons.id` with a `jurisdictions.id` and the
//! state-issued `license_number`. The pair `(person_id,
//! jurisdiction_id)` is unique so the same attorney can't be
//! double-listed under one jurisdiction.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "credentials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub person_id: Uuid,
    pub jurisdiction_id: Uuid,
    pub license_number: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::jurisdiction::Entity",
        from = "Column::JurisdictionId",
        to = "super::jurisdiction::Column::Id"
    )]
    Jurisdiction,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::jurisdiction::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Jurisdiction.def()
    }
}

crate::uuid_active_model_behavior!();
