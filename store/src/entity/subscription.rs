//! `subscriptions` table — an active recurring engagement billed once per
//! period through Xero. See
//! [`m20260715_create_subscriptions`](super::super::migration).
//!
//! The `products` catalog holds the recurring product's price + cadence;
//! this row holds the per-engagement state the recurring-billing workflow
//! needs: who is billed (`contact_name` / `contact_email`, with soft
//! `person_id` / `entity_id` / `project_id` links), the `status`, and the
//! durable idempotency ledger [`Model::last_invoiced_period`] (`YYYY-MM`,
//! UTC). The workflow bills every `active` subscription whose
//! `last_invoiced_period` is behind the current period and advances it
//! only after the Xero invoice returns Ok.
//!
//! A discount mirrors [`billing::LineDiscount`]'s two shapes: at most one
//! of `discount_percent` / `discount_amount_cents` is set; both `None`
//! bills at list.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "subscriptions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Billed person, when the payer is an individual. Soft link.
    pub person_id: Option<Uuid>,
    /// Billed entity, when the payer is an organisation. Soft link.
    pub entity_id: Option<Uuid>,
    /// Originating project/matter, when one exists. Soft link.
    pub project_id: Option<Uuid>,
    /// The recurring product's `code` (`nexus`, `nautilus`). Soft
    /// reference to `products.code`.
    pub product_code: String,
    /// Billed party's display name (the Xero contact `Name`).
    pub contact_name: String,
    /// Billed party's email — the Xero contact match key.
    pub contact_email: String,
    /// `pending` | `active` | `paused` | `cancelled`. Only `active` is
    /// billed. A subscription tied to an unsigned retainer starts
    /// `pending` and is activated when that retainer is signed.
    pub status: String,
    /// RFC 3339 timestamp the subscription began.
    pub started_at: String,
    /// The most recent billing period (`YYYY-MM`, UTC) already invoiced.
    /// `None` = never billed.
    pub last_invoiced_period: Option<String>,
    /// Optional whole-percent discount off list (`0..=100`).
    pub discount_percent: Option<i32>,
    /// Optional flat discount off list, in cents.
    pub discount_amount_cents: Option<i64>,
    pub inserted_at: String,
    pub updated_at: String,
}

/// A pending subscription — created but not yet billable, awaiting the
/// signed retainer that activates it. Skipped by the workflow until then,
/// so a recurring engagement is never invoiced before its engagement
/// agreement is executed.
pub const STATUS_PENDING: &str = "pending";
/// An active subscription — the only status the recurring-billing workflow
/// invoices.
pub const STATUS_ACTIVE: &str = "active";
/// A paused subscription — skipped by the workflow, can resume.
pub const STATUS_PAUSED: &str = "paused";
/// A cancelled subscription — skipped by the workflow, terminal.
pub const STATUS_CANCELLED: &str = "cancelled";

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
