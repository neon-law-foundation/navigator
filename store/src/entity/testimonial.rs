//! `testimonials` table — consented public proof tied to real matters.
//!
//! Each Testimonial belongs to exactly one Project and one Person. Public
//! website reads require both `consented_at` and `published_at` so staff
//! can collect matter feedback without accidentally publishing it.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "testimonials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub person_id: Uuid,
    /// Optional product placement key (`nexus`, `litigation`, …).
    pub product_code: Option<String>,
    /// The approved testimonial body.
    pub quote: String,
    /// Optional public display override when `persons.name` is not the
    /// approved attribution text.
    pub attribution_label: Option<String>,
    /// RFC 3339 consent timestamp. `None` means never render publicly.
    pub consented_at: Option<String>,
    /// RFC 3339 staff publication timestamp. `None` means never render
    /// publicly.
    pub published_at: Option<String>,
    pub display_order: i32,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::product::Entity",
        from = "Column::ProductCode",
        to = "super::product::Column::Code"
    )]
    Product,
}

crate::uuid_active_model_behavior!();
