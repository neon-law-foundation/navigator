//! `review_documents` — one attorney-reviewed draft a client reads (and
//! comments on) before signing.
//!
//! A matter (notation) can have several: an estate plan is a will, a
//! trust, and health + financial directives. Each row holds the draft as
//! HTML — TipTap renders it read-only on the portal review surface. The
//! `status` gate keeps a draft hidden from the client until an attorney
//! approves it (`draft` → `pending_review`), then records the client's
//! sign-off (`pending_review` → `approved`). See
//! [`crate::review_documents`] for the helpers and
//! [`super::document_comment`] for the comment thread that hangs off it.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Hidden from the client; the generation workflow parks a freshly
/// generated draft here until an attorney approves it.
pub const STATUS_DRAFT: &str = "draft";
/// Visible to the scoped client, who may read and comment.
pub const STATUS_PENDING_REVIEW: &str = "pending_review";
/// The client has signed off; the draft is ready for signature.
pub const STATUS_APPROVED: &str = "approved";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "review_documents")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::notation`] — the matter this draft belongs to.
    pub notation_id: Uuid,
    /// Document kind within the matter: `will`, `trust`,
    /// `directive_health`, `directive_financial`, …
    pub kind: String,
    /// Human-readable title shown to the client.
    pub title: String,
    /// Attorney-reviewed draft body as sanitized HTML.
    pub body_html: String,
    /// `draft`, `pending_review`, or `approved` — see the `STATUS_*`
    /// constants.
    pub status: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::notation::Entity",
        from = "Column::NotationId",
        to = "super::notation::Column::Id"
    )]
    Notation,
    #[sea_orm(has_many = "super::document_comment::Entity")]
    DocumentComment,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

impl Related<super::document_comment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DocumentComment.def()
    }
}

crate::uuid_active_model_behavior!();
