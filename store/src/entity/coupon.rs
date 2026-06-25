//! `coupons` table — a reusable, named discount applied to a subscription
//! at sign-up. See [`m20260720_create_coupons`](super::super::migration).
//!
//! Xero has no coupon object, so a coupon is a Neon Law Navigator concept: it holds
//! the *intent* of a standing discount. Applying it resolves to one of
//! [`billing::LineDiscount`]'s two shapes and snapshots that onto the
//! subscription's own discount columns — see [`super::subscription`] — so
//! a later coupon edit never re-prices an existing client. At most one of
//! `discount_percent` / `discount_amount_cents` is set.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "coupons")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The redeemable code (`FRIEND99`). Unique, case-sensitive.
    #[sea_orm(unique)]
    pub code: String,
    /// Optional whole-percent discount off list (`0..=100`).
    pub discount_percent: Option<i32>,
    /// Optional flat discount off list, in cents.
    pub discount_amount_cents: Option<i64>,
    /// Optional product `code` this coupon is scoped to (`nexus`). `None`
    /// applies to any product.
    pub product_code: Option<String>,
    /// Optional RFC 3339 (UTC) expiry. `None` = never expires.
    pub expires_at: Option<String>,
    /// Optional cap on how many subscriptions may apply this coupon.
    /// `None` = unlimited.
    pub max_redemptions: Option<i32>,
    /// How many times this coupon has been applied so far.
    pub redeemed_count: i32,
    /// Whether the coupon may currently be applied.
    pub active: bool,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
