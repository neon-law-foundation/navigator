//! `xero_invoices` table — the local mirror of a matter's Xero invoice.
//!
//! One row per matter (`UNIQUE(project_id)`), written when the
//! matter-close fee is raised and updated by the nightly reconcile
//! workflow. The portal reads this mirror to show per-project paid
//! invoices; it never calls Xero live. See
//! [`m20260707_create_xero_invoices`](super::super::migration). The
//! canonical AR table is [`super::invoice`] (entity-billing-profile
//! scoped) — this is the project-scoped Xero side, kept separate on
//! purpose.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "xero_invoices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → Project; at most one close invoice per matter.
    #[sea_orm(unique)]
    pub project_id: Uuid,
    /// Xero `InvoiceID` (GUID) returned on create. Internal — never
    /// surfaced on a client-facing response.
    pub xero_invoice_id: String,
    /// The invoice-level `Reference` carried into Xero
    /// (`Matter <project_id>`); the durable join key on the Xero side.
    pub reference: String,
    /// Xero invoice status mirror (`AUTHORISED`, `PAID`, `VOIDED`, …).
    /// Updated by the reconcile workflow.
    pub status: String,
    /// Invoice total in minor units (cents). Avoids float.
    pub amount_cents: i64,
    /// Amount paid in minor units (cents); `0` until reconciled.
    pub amount_paid_cents: i64,
    /// ISO 4217 currency code (e.g., `USD`).
    pub currency: String,
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
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

crate::uuid_active_model_behavior!();
