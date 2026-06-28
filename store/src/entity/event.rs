//! `events` table — public events (show-and-tells) tracked for
//! registration.
//!
//! The Markdown files under `web/content/events/` remain the display
//! source of truth. This row tracks event existence (keyed by the
//! Markdown `slug`) and the registrant emails, so registration can move
//! off Luma. Wall-clock `starts_at` / `ends_at` are stored without a
//! time zone alongside the IANA `timezone` they belong to.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Markdown file slug — the natural key the sync upserts on. Unique.
    #[sea_orm(unique)]
    pub slug: String,
    /// Public-facing slug used in the registration URL.
    pub public_slug: String,
    /// Event kind. CHECK-constrained to `show_and_tell` at the schema.
    pub event_type: String,
    /// Local wall-clock start time (no tz); zone is `timezone`.
    pub starts_at: DateTime,
    /// Local wall-clock end time (no tz); zone is `timezone`.
    pub ends_at: DateTime,
    /// IANA timezone name the start/end wall times are in.
    pub timezone: String,
    /// When true the event is unpublished and rejects registration.
    pub draft: bool,
    /// Registrant emails. Stores only the email (data minimization).
    pub registrations: Vec<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
