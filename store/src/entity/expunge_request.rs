//! `expunge_requests` — a client's request to delete one of their matter
//! documents, awaiting attorney authorization.
//!
//! The governed-expunge primitive is admin-only (see
//! [the design](../../../docs/git-project-repos.md) §9 and
//! [`crate::expunge_records`]); a client can only *ask*. This row records
//! that ask: a client requests deletion (`status = pending`); a
//! staff/admin **authorizes** it (running the admin-gated expunge, with
//! `expunge_record_id` linking the resulting audit row) or **denies** it.
//! The executed expunge is always category `client_request`.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Awaiting staff/admin review — the default for a new request.
pub const STATUS_PENDING: &str = "pending";
/// Authorized — the admin-gated expunge has run; `expunge_record_id` is
/// set.
pub const STATUS_AUTHORIZED: &str = "authorized";
/// Denied by a staff/admin; nothing was deleted.
pub const STATUS_DENIED: &str = "denied";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "expunge_requests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::project`] — the matter the document belongs to.
    pub project_id: Uuid,
    /// FK → [`super::document`] — the document the client wants deleted.
    pub document_id: Uuid,
    /// FK → [`super::person`] — the client who requested deletion.
    pub requested_by_person_id: Uuid,
    /// `pending`, `authorized`, or `denied` — see the `STATUS_*` constants.
    pub status: String,
    /// Optional non-content note from the client (their stated reason).
    pub note: Option<String>,
    /// FK → [`super::person`] — the staff/admin who resolved it. `None`
    /// while pending.
    pub resolved_by_person_id: Option<Uuid>,
    /// FK → [`super::expunge_record`] — the audit row from the executed
    /// expunge. `None` unless the request was authorized.
    pub expunge_record_id: Option<Uuid>,
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
        belongs_to = "super::document::Entity",
        from = "Column::DocumentId",
        to = "super::document::Column::Id"
    )]
    Document,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Document.def()
    }
}

crate::uuid_active_model_behavior!();
