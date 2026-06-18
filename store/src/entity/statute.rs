//! `statutes` table — the stable identity row for one section of a
//! published legal code (e.g. NRS 86.011).
//!
//! This row carries **no statute text** — only identity (`code`,
//! `chapter`, `section`), the official-source link, a `status` flag,
//! and bookkeeping dates. The text itself lives in append-only
//! [`super::statute_revision`] rows; "current" is the latest revision,
//! derived at read time, never stored here. See
//! `prompts/nrs-statute-scraper-design.md` (the insert-only model).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Whether a section is still published in its chapter (`Active`) or has
/// vanished from the source since we first saw it (`Repealed`). Stored
/// as `TEXT` with a `CHECK (status IN ('active','repealed'))`
/// constraint. A repeal is a bookkeeping flag — the section's revisions
/// are never deleted, so its final observed text is preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text")]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Still present in the chapter as of the last run.
    #[sea_orm(string_value = "active")]
    Active,
    /// No longer present in the chapter; history retained.
    #[sea_orm(string_value = "repealed")]
    Repealed,
}

impl Status {
    /// String form used in URLs and templates.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Repealed => "repealed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "statutes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Jurisdiction the code belongs to (`NV`).
    pub jurisdiction: String,
    /// Code abbreviation (`NRS`).
    pub code: String,
    /// Chapter the section lives in (`86`, `118A`). Kept as the source's
    /// own string so `86A` and `118A` round-trip without surprises.
    pub chapter: String,
    /// Human-readable chapter title (`Limited-Liability Companies`).
    pub chapter_title: String,
    /// The section number, code-qualified the way the source prints it
    /// (`86.011`). Unique within `(code, section)`.
    pub section: String,
    /// Permalink to the section on the official source.
    pub source_url: String,
    /// `Active` while still published, `Repealed` once it vanishes.
    pub status: Status,
    /// Run date (RFC 3339) the section was first observed.
    pub first_seen_at: String,
    /// Run date (RFC 3339) of the most recent run that saw the section,
    /// changed or not. Bumped every run; the cheap "we looked" stamp.
    pub last_checked_at: String,
    /// Run date (RFC 3339) the body last changed (a new revision was
    /// appended). Equals `first_seen_at` until the text first moves.
    pub last_changed_at: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// One section has many append-only revisions.
    #[sea_orm(has_many = "super::statute_revision::Entity")]
    Revision,
}

impl Related<super::statute_revision::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Revision.def()
    }
}

crate::uuid_active_model_behavior!();
