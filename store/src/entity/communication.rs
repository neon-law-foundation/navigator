//! `communications` — one message in a matter's single privileged
//! conversation log, regardless of the door it arrived through.
//!
//! This is the **spine** of a spine+satellites model (not single-table
//! inheritance): the row carries the fields every channel shares, while
//! channel-specific fidelity lives in satellites FK'd back here — the
//! comment anchor in [`super::document_comment`], the RFC headers in the
//! email tables. The `channel` column is the discriminator; adding
//! `sms_inbound`/`sms_outbound` later needs a new literal, not a new
//! column. See [`crate::communications`] for the ingest + query helpers.
//!
//! Privilege is structural: every row is project-scoped client
//! communication, so reads are gated by `project_id`, never surfaced
//! firm-wide. `direction = internal` rows are firm-only.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "communications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::project`] — the matter this message belongs to.
    pub project_id: Uuid,
    /// Channel discriminator: `document_comment`, `email_inbound`,
    /// `email_outbound`, `portal_message`, `sms_inbound`, `sms_outbound`.
    pub channel: String,
    /// `inbound` (from client), `outbound` (to client), or `internal`
    /// (firm note — never shown to the client).
    pub direction: String,
    /// FK → [`super::person`] — who authored it; `None` for system /
    /// unknown sender.
    pub author_person_id: Option<Uuid>,
    /// Email/name of the other party when there is no `persons` row.
    pub counterparty: Option<String>,
    /// Optional subject line (email subject; `None` for comments).
    pub subject: Option<String>,
    /// Normalized message text.
    pub body: String,
    /// External id for idempotent ingest: email `Message-ID`, the comment
    /// id, the SMS provider id. Unique per channel when present.
    pub source_ref: Option<String>,
    /// FK → [`super::blob`] — raw payload (verbatim `.eml`, …); `None` for
    /// comments.
    pub blob_id: Option<Uuid>,
    /// RFC 3339 timestamp of when the message actually happened.
    pub occurred_at: String,
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
        from = "Column::AuthorPersonId",
        to = "super::person::Column::Id"
    )]
    Author,
    #[sea_orm(
        belongs_to = "super::blob::Entity",
        from = "Column::BlobId",
        to = "super::blob::Column::Id"
    )]
    Blob,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::blob::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Blob.def()
    }
}

crate::uuid_active_model_behavior!();
