//! `products` table — the firm's product catalog and the single source
//! of truth for a product's list price. See
//! [`m20260709_create_products`](super::super::migration).
//!
//! One row per product, keyed by a stable `code`. The list price lives
//! here exactly once; an admin discount is a recorded override on the
//! engagement (see [`super::notation`]), never a second product row.
//!
//! `code` is the marketing/Xero product key (`northstar`, `nest`,
//! `nexus`, `nautilus`, `litigation`) — NOT a template prefix. The
//! billing trigger is the separate [`Model::matter_close_template_code`]
//! column, which names the originating template `code` whose matter-close
//! raises this product's flat fee (a soft reference, not a FK).

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "products")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Stable product key — the marketing/Xero identity. Unique.
    #[sea_orm(unique)]
    pub code: String,
    pub display_name: String,
    /// List price in minor units (cents). No float. For an hourly
    /// product this carries the hourly rate in cents.
    pub list_price_cents: i64,
    /// ISO 4217 currency code (e.g., `USD`).
    pub currency: String,
    /// Billing cadence: see the `CADENCE_*` constants.
    pub cadence: String,
    /// How the price is billed: see the `BILLING_KIND_*` constants. Only
    /// [`BILLING_KIND_MATTER_CLOSE_FLAT`] products raise a matter-close fee.
    pub billing_kind: String,
    pub active: bool,
    /// Optional Xero `ItemCode` mirror; `None` until mirrored.
    pub xero_item_code: Option<String>,
    /// Xero chart-of-accounts code this product's revenue posts to (e.g.
    /// `200` = Sales). The recurring-billing workflow reads it as the
    /// invoice line's `AccountCode`.
    pub account_code: String,
    /// The originating template `code` whose matter-close raises this
    /// product's flat fee (e.g. `onboarding__estate` for Northstar).
    /// Soft reference, not a FK. `None` for products with no
    /// matter-close flat fee (Nautilus, 1337).
    pub matter_close_template_code: Option<String>,
    /// The retainer template `code` whose engagement agreement a matter
    /// under this product opens with (e.g. `onboarding__retainer_nest`).
    /// The demand-side mirror of [`Model::matter_close_template_code`].
    /// Soft reference, not a FK. `None` falls back to the generic
    /// `onboarding__retainer`.
    pub retainer_template_code: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

/// One-time fee billed when the matter closes.
pub const CADENCE_ONCE: &str = "once";
/// A flat fee billed per discrete instance of the service (e.g. one
/// attorney attestation), where a client may buy many over time. Display
/// cadence only — the billing still runs through the matter-close seam.
pub const CADENCE_EACH: &str = "each";
/// Recurring monthly fee.
pub const CADENCE_MONTHLY: &str = "monthly";
/// Recurring yearly fee.
pub const CADENCE_YEARLY: &str = "yearly";
/// Billed per hour.
pub const CADENCE_HOURLY: &str = "hourly";

/// A flat fee raised through the billing seam when the matter closes
/// (the firm countersignature on the closing letter). Only these
/// products raise a matter-close fee.
pub const BILLING_KIND_MATTER_CLOSE_FLAT: &str = "matter_close_flat";
/// A recurring subscription fee (monthly/yearly); not a matter-close flat.
pub const BILLING_KIND_RECURRING: &str = "recurring";
/// Billed by the hour; not a matter-close flat.
pub const BILLING_KIND_HOURLY: &str = "hourly";

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
