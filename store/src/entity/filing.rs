//! `filings` table — one durable record per outbound compliance
//! submission (mail to a party or a filing with a government office).
//!
//! Written by the workflow worker inside a submission step's `ctx.run`
//! (`mailroom_send`, `certified_mail`, `e_filing`, `filing__*`), so the
//! row is the replay-idempotent proof of what was filed. A row exists
//! only after the matter passed `staff_review` — the workflow spec
//! guarantees no submission state is reachable without a review first
//! (`workflows::staff_review_precedes_submission`). See
//! [`crate::filings`] for the insert helper.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "filings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::notation`] — the matter that was filed.
    pub notation_id: Uuid,
    /// Submission step kind that fired this record: `mailroom_send`,
    /// `certified_mail`, `e_filing`, or `filing` (the state prefix).
    pub kind: String,
    /// Recipient office / party (e.g. `Nevada Secretary of State`).
    pub office: String,
    /// Provider/office tracking reference; `None` until one is known.
    pub reference: Option<String>,
    /// Human-readable summary of what was submitted.
    pub summary: String,
    /// RFC 3339 timestamp the submission side effect fired.
    pub submitted_at: String,
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
}

impl Related<super::notation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Notation.def()
    }
}

crate::uuid_active_model_behavior!();
