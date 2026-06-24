//! `persons` table — a directory of human contacts.
//!
//! The system-wide tier — `client`, `staff`, or `admin` — lives on
//! this row in the [`Role`] column, not on the IdP token. The
//! Keycloak (or Google) id_token carries only `sub` and `email`;
//! when the callback runs, we look up the matching `persons` row
//! and read `role` from it. That's what OPA evaluates against.
//! See [`docs/access-model.md`](../../../../docs/access-model.md).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// System-wide authorization tier for a [`Model`]. Stored as `TEXT`
/// in the database with a `CHECK (role IN ('client','staff','admin'))`
/// constraint. Anonymous callers have no row at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A person the firm represents on at least one matter. Sees
    /// only projects with a matching `person_project_roles` row.
    #[sea_orm(string_value = "client")]
    Client,
    /// A firm employee. Same per-project scoping as `Client`; the
    /// tier difference shows up in what they can *do* on a visible
    /// project, not in what's visible.
    #[sea_orm(string_value = "staff")]
    Staff,
    /// A firm employee with system-administration authority. Bypasses
    /// project-scoping entirely.
    #[sea_orm(string_value = "admin")]
    Admin,
}

impl Role {
    /// String form used in OPA inputs and the URL-encoded admin form.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Staff => "staff",
            Self::Admin => "admin",
        }
    }

    /// `true` for `Staff` and `Admin` — the two firm-side tiers that
    /// gate `/portal/admin/*` CRUD. Used by route-layer guards that need a
    /// staff-or-better check without re-encoding the matrix.
    #[must_use]
    pub fn is_staff_tier(self) -> bool {
        matches!(self, Self::Staff | Self::Admin)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "persons")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    #[sea_orm(unique)]
    pub email: String,
    /// OIDC `sub` claim — stable identifier from the IdP (Keycloak,
    /// Google, etc.). `None` for seeded persons not yet linked.
    #[sea_orm(unique, nullable)]
    pub oidc_subject: Option<String>,
    /// System-wide tier. Defaults to `Client` for both seeded rows
    /// and freshly-upserted users — explicit promotion to `Staff` or
    /// `Admin` is always opt-in.
    pub role: Role,
    /// The contact's role at their organization (e.g. "Executive
    /// Director"). Free text; `None` until set by the importer or an
    /// admin edit.
    #[sea_orm(nullable)]
    pub title: Option<String>,
    /// The contact's direct phone line. `None` until set.
    #[sea_orm(nullable)]
    pub phone: Option<String>,
    /// Xero `ContactID` (GUID) once this person has been mirrored to
    /// Xero Contacts via the billing seam (one-way, Navigator → Xero).
    /// `None` until first synced. Backs the admin people-detail Xero
    /// deep-link and makes the contacts upsert idempotent after the
    /// first resolve. See `m20260707_create_xero_invoices`.
    #[sea_orm(nullable)]
    pub xero_contact_id: Option<String>,
    /// BCP-47 locale the questionnaire renders in for this person
    /// (`en`, `es`, …). Defaults to `en` at the DB level, so every
    /// existing/seeded person keeps the English experience. See
    /// `m20260623_add_intake_language`.
    pub preferred_language: String,
    /// Optional public profile image URL. Used only on consented public
    /// attribution surfaces such as testimonials.
    pub profile_image_url: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

crate::uuid_active_model_behavior!();
