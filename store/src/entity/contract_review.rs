//! `contract_reviews` — the per-notation work-product satellite for an
//! inbound contract review.
//!
//! The review-*in* mirror of [`super::review_document`]: the `findings`
//! JSONB array is the deviation report the analysis step produces and the
//! reviewing attorney edits before approving. `document_id` points at the
//! filed inbound-contract [`super::document`] row; `risk_summary` is filled
//! once analyzed. Per-finding attorney attribution (who accepted what) is
//! the matter's audit trail and lives in `notation_events` — this row is the
//! editable working copy. Typed view of a finding:
//! [`crate::contract_reviews::Finding`].

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Created, contract filed, not yet analyzed.
pub const STATUS_PENDING: &str = "pending";
/// Analysis produced findings; awaiting attorney review.
pub const STATUS_ANALYZED: &str = "analyzed";
/// The reviewing attorney approved; the memo renders next.
pub const STATUS_APPROVED: &str = "approved";
/// The reviewing attorney rejected the review.
pub const STATUS_REJECTED: &str = "rejected";

// NB: no `Eq` derive — `Json` (serde_json::Value) is `PartialEq` but not
// `Eq`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "contract_reviews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::notation`] — the review matter.
    pub notation_id: Uuid,
    /// FK → [`super::playbook`] — positions the contract was measured against.
    pub playbook_id: Uuid,
    /// FK → [`super::document`] — the filed inbound contract; `None` until
    /// the contract is uploaded.
    pub document_id: Option<Uuid>,
    /// `pending`, `analyzed`, `approved`, or `rejected` — see the `STATUS_*`
    /// constants.
    pub status: String,
    /// Plain-language risk summary; `None` until analyzed.
    pub risk_summary: Option<String>,
    /// JSONB array of findings. Typed view:
    /// [`crate::contract_reviews::Finding`].
    #[sea_orm(column_type = "JsonBinary")]
    pub findings: Json,
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
    #[sea_orm(
        belongs_to = "super::playbook::Entity",
        from = "Column::PlaybookId",
        to = "super::playbook::Column::Id"
    )]
    Playbook,
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

impl Related<super::playbook::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Playbook.def()
    }
}

crate::uuid_active_model_behavior!();
