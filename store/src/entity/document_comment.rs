//! `document_comments` — one comment a reader anchored to a text range
//! within a [`super::review_document`].
//!
//! The review surface is read-only; a comment is the only thing the
//! client writes there. The anchor is a character-offset range into the
//! document text (`anchor_start`..`anchor_end`) plus the `quoted_text` it
//! covered, so the sidebar can show the quote even if the underlying
//! draft is later re-rendered. The offsets are engine-independent: the
//! read surface computes them client-side, so swapping the viewer never
//! breaks the stored anchors. Staff flip `resolved` once addressed. See
//! [`crate::document_comments`] for the helpers.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "document_comments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::review_document`] — the draft this comment is on.
    pub review_document_id: Uuid,
    /// FK → [`super::person`] — who wrote the comment.
    pub person_id: Uuid,
    /// Start offset (character offset into the document text) of the
    /// commented range.
    pub anchor_start: i32,
    /// End offset (character offset into the document text) of the
    /// commented range.
    pub anchor_end: i32,
    /// The text the range covered when the comment was made.
    pub quoted_text: String,
    /// The comment text the reader wrote.
    pub body: String,
    /// `true` once staff have addressed the comment.
    pub resolved: bool,
    /// FK → [`super::communication`] — the spine row for this comment in the
    /// unified privileged conversation log. `None` for legacy Phase A rows
    /// written before the review POST routed through the communications
    /// spine. See `m20260705_create_communications`.
    pub communication_id: Option<Uuid>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::review_document::Entity",
        from = "Column::ReviewDocumentId",
        to = "super::review_document::Column::Id"
    )]
    ReviewDocument,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
}

impl Related<super::review_document::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ReviewDocument.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

crate::uuid_active_model_behavior!();
