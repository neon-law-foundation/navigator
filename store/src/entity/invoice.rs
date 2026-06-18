//! `invoices` table — one invoice per row, owned by an entity
//! billing profile.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "invoices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub entity_billing_profile_id: Uuid,
    /// Caller-visible invoice number (e.g., `INV-2026-0001`).
    #[sea_orm(unique)]
    pub number: String,
    /// `draft`, `issued`, `paid`, `void`.
    pub status: String,
    /// Total in minor units (cents). Avoids float for money.
    pub total_cents: i64,
    pub currency: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::entity_billing_profile::Entity",
        from = "Column::EntityBillingProfileId",
        to = "super::entity_billing_profile::Column::Id"
    )]
    EntityBillingProfile,
    #[sea_orm(has_many = "super::invoice_line_item::Entity")]
    LineItem,
}

impl Related<super::invoice_line_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::LineItem.def()
    }
}

crate::uuid_active_model_behavior!();
