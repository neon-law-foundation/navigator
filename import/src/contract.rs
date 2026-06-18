//! The bulk-import wire contract — the serde shapes a caller submits.
//!
//! Version 1 carries organizations and the people who work at them.
//! Projects (the on-prem-install engagement with its onboarding /
//! offboarding lifecycle) are deliberately *not* in v1: contacts land
//! first, a Project is opened per real engagement later. The envelope
//! is versioned so that block can be added without breaking callers.

use serde::{Deserialize, Serialize};

/// The only contract version this engine understands.
pub const SUPPORTED_VERSION: u32 = 1;

/// Link role written to `person_entity_roles` when a person's row does
/// not name one. These are the org's point(s) of contact for the
/// engagement, not its officers — see the design doc.
pub const DEFAULT_ENTITY_ROLE: &str = "client_contact";

/// A whole bulk-import submission.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Payload {
    /// Contract version. Must equal [`SUPPORTED_VERSION`].
    #[serde(default = "default_version")]
    pub version: u32,
    /// Free-text provenance (e.g. `"legal-aid-outreach-2026-06"`).
    /// Not persisted to a column — it rides into the `OTel` trace so the
    /// import's origin is queryable in telemetry. Optional.
    #[serde(default)]
    pub source: Option<String>,
    /// The organizations to find-or-create as `entities`.
    #[serde(default)]
    pub organizations: Vec<OrgRecord>,
    /// The people to find-or-create as `persons`, each linked to one
    /// organization by its `key`.
    #[serde(default)]
    pub people: Vec<PersonRecord>,
}

/// One organization → one `entities` row.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrgRecord {
    /// Stable in-file identifier the people reference. Cross-reference
    /// and dedupe handle only — never persisted.
    pub key: String,
    /// Legal name, used as the `entities.name` dedupe key.
    pub name: String,
    /// Entity-type name resolved to `entity_types.id` (e.g.
    /// `"501(c)(3) Non-Profit"`). Must already exist as reference data.
    pub entity_type: String,
    /// Two-letter jurisdiction code resolved to `jurisdictions.id`
    /// (e.g. `"WA"`). Case-insensitive.
    pub jurisdiction: String,
    /// Main phone line. Optional.
    #[serde(default)]
    pub phone: Option<String>,
    /// Website URL, canonicalized (https, no query/fragment) before it
    /// is stored. Optional.
    #[serde(default)]
    pub url: Option<String>,
}

/// One person → one `persons` row, linked to one organization.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersonRecord {
    /// Stable in-file identifier. Cross-reference handle only — never
    /// persisted.
    pub key: String,
    /// Full name.
    pub name: String,
    /// Email — the unique upsert key for the `persons` row.
    pub email: String,
    /// Role at the organization (e.g. `"Executive Director"`). Optional.
    #[serde(default)]
    pub title: Option<String>,
    /// Direct phone line. Optional.
    #[serde(default)]
    pub phone: Option<String>,
    /// The `key` of the organization in this payload's `organizations`
    /// list that this person works at.
    pub organization: String,
    /// Link role written to `person_entity_roles`. Defaults to
    /// [`DEFAULT_ENTITY_ROLE`].
    #[serde(default = "default_entity_role")]
    pub entity_role: String,
}

fn default_version() -> u32 {
    SUPPORTED_VERSION
}

fn default_entity_role() -> String {
    DEFAULT_ENTITY_ROLE.to_string()
}
