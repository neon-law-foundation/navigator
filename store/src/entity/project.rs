//! `projects` table — a unit of work tracked across notations and
//! retainers.
//!
//! Each Project is also an append-only, single-branch git repository (its
//! documents, versioned via Git LFS — see
//! [the design](../../../docs/git-project-repos.md)). The ref is *always*
//! `main`, enforced by the bare repo's `pre-receive` hook; the one source
//! of truth for the name is the `repos::DEFAULT_BRANCH` constant, so there
//! is no per-row branch column. `git_initialized_at` records lazy repo
//! creation.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "projects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    /// `open`, `closed`, `archived`.
    pub status: String,
    /// The legal organization (or `Human` entity) this matter is opened
    /// against. **Required** — a matter always tracks a pre-existing
    /// entity; the matter-open service validates the entity exists before
    /// opening. Enforced `NOT NULL` by
    /// `m20260712_projects_entity_id_not_null`.
    pub entity_id: Uuid,
    /// The matter's scope narrative — "this project's story." Captured at
    /// matter-open and seeded as the retainer's position-0 custom clause
    /// (System provenance, an attorney-editable draft). `None` for a plain
    /// project create or a matter opened before
    /// `m20260711_add_description_to_projects`.
    pub description: Option<String>,
    /// RFC 3339 timestamp when the bare repo was first created on the
    /// volume. `None` = not yet initialized (the repo is created lazily
    /// on first git access).
    pub git_initialized_at: Option<String>,
    /// RFC 3339 timestamp the matter was closed — the start of the 10-year
    /// retention window for its privileged conversation log. `None` while
    /// open. See migration `m20260706_add_closed_at_to_projects` and
    /// [`crate::communications::purge_expired_matters`].
    pub closed_at: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::entity::Entity",
        from = "Column::EntityId",
        to = "super::entity::Column::Id"
    )]
    Entity,
}

crate::uuid_active_model_behavior!();
