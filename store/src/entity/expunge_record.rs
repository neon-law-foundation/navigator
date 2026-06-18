//! `expunge_records` — the audit trail of governed expunges.
//!
//! A governed expunge rewrites a matter repo's history to remove a
//! privileged / sealed / lawfully-deleted document. This row records the
//! expunge *itself* — who authorized it, when, the category, and the
//! before/after head oids — but never the content removed, so the
//! redaction is auditable without re-exposing it. See
//! [`crate::expunge_records`] and
//! [the design](../../../docs/git-project-repos.md) §9.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Privilege clawback — material committed in error that is privileged.
pub const CATEGORY_PRIVILEGE: &str = "privilege";
/// A court sealing order.
pub const CATEGORY_SEALING: &str = "sealing";
/// A client's lawful deletion request.
pub const CATEGORY_CLIENT_REQUEST: &str = "client_request";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "expunge_records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::project`] — the matter whose repo was rewritten.
    pub project_id: Uuid,
    /// Repo path removed from all history (metadata, not content).
    pub path: String,
    /// `privilege`, `sealing`, or `client_request` — see the `CATEGORY_*`
    /// constants.
    pub category: String,
    /// FK → [`super::person`] — the admin who authorized the expunge.
    pub authorized_by_person_id: Uuid,
    /// `main` oid before the rewrite (`None` if the repo was empty).
    pub head_before: Option<String>,
    /// `main` oid after the rewrite.
    pub head_after: Option<String>,
    /// Optional non-content note (e.g. a docket reference).
    pub note: Option<String>,
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
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::AuthorizedByPersonId",
        to = "super::person::Column::Id"
    )]
    AuthorizedBy,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

crate::uuid_active_model_behavior!();
