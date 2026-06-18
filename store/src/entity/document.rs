//! `documents` table — a named, project-scoped reference to a
//! `blob` plus the metadata callers see.
//!
//! Every document belongs to exactly one project (`project_id` is
//! `NOT NULL`) and carries the provenance it was ingested with:
//!
//! - `source` — inbound channel literal (`upload`, `drive_sync`,
//!   future: `email`, `fax`).
//! - `source_revision_id` — upstream revision id. Drive's
//!   `headRevisionId` for Drive sync, Message-ID for email, fax
//!   sequence number, etc. `None` until the inbound workflow that
//!   owns this document finishes writing it; immutable once set.
//! - `received_at` — when the inbound channel got the artifact (not
//!   when we recorded it).
//! - `description` — optional one-line staff-view caption.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "documents")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub blob_id: Uuid,
    pub filename: String,
    pub kind: String,
    /// Inbound channel — `upload`, `drive_sync`, …
    pub source: String,
    /// Upstream revision id (Drive `headRevisionId`, email
    /// `Message-ID`, fax sequence number). `None` until the workflow
    /// that owns this document finishes writing it; immutable once
    /// set.
    pub source_revision_id: Option<String>,
    /// RFC 3339 timestamp from the inbound channel.
    pub received_at: String,
    /// Optional staff-view caption (e.g., "Letter from Acme Bank
    /// dated 2026-05-23").
    pub description: Option<String>,
    /// Git commit oid (in the Project's repo) that filed this document.
    /// `None` until it is committed to the repo. See migration
    /// `m20260628_add_git_commit_oid_to_documents`.
    pub git_commit_oid: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::blob::Entity",
        from = "Column::BlobId",
        to = "super::blob::Column::Id"
    )]
    Blob,
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
}

impl Related<super::blob::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Blob.def()
    }
}

crate::uuid_active_model_behavior!();
