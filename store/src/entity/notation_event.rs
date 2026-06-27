//! `notation_events` table — append-only journal of every
//! state-machine transition for every Notation.
//!
//! Each row is the on-disk shape of a
//! [`workflows::WorkflowEvent`](../../../workflows/src/runtime.rs).
//! One row per transition; rows are **never updated**. The "current
//! state" of a given `(notation_id, machine_kind)` is the
//! `to_state` of the latest row ordered by `id` — see
//! [`latest_for_kind`].
//!
//! Why a journal and not a mutable cursor: Restate is the durable
//! source of truth in production; Postgres holds the projection
//! we query. Mirroring the runtime's event type as an append log
//! keeps the two layers coherent, preserves the full history for
//! audit and debugging, and lets future event kinds (paused,
//! resumed, errored) extend the schema additively.

use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, ColumnTrait, ConnectionTrait, DbErr, QueryFilter, QueryOrder};
use serde::Serialize;
use uuid::Uuid;

/// Machine-kind discriminator stored as text in `machine_kind`.
/// Mirrors `workflows::spec::MachineKind` — kept in sync by the
/// workers writing this table.
pub const MACHINE_QUESTIONNAIRE: &str = "questionnaire";
/// Machine-kind discriminator for the post-intake workflow.
pub const MACHINE_WORKFLOW: &str = "workflow";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "notation_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub notation_id: Uuid,
    /// Lowercase machine-kind token — `"questionnaire"` or
    /// `"workflow"`. Mirrors `workflows::MachineKind::as_str`.
    pub machine_kind: String,
    pub from_state: String,
    pub to_state: String,
    pub condition: String,
    /// Optional JSON payload for variable per-event data — for a
    /// questionnaire `signal` this typically carries the answer
    /// value (`{"answer_value": "..."}`); for a workflow signal
    /// it's usually `None`. Stored as text so the entity stays
    /// portable across SQLite (JSON1 via `json_extract`) and
    /// Postgres (JSONB).
    pub payload: Option<String>,
    /// ISO 8601 timestamp string (RFC 3339).
    pub recorded_at: String,
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

/// Read the latest event for a `(notation_id, machine_kind)`
/// pair. This is the projection the application uses as
/// "current state" — `result.map(|m| m.to_state)`.
///
/// Returns `None` if no event has been recorded for that pair —
/// the state machine hasn't started yet.
pub async fn latest_for_kind(
    db: &impl ConnectionTrait,
    notation_id: Uuid,
    machine_kind: &str,
) -> Result<Option<Model>, DbErr> {
    Entity::find()
        .filter(Column::NotationId.eq(notation_id))
        .filter(Column::MachineKind.eq(machine_kind))
        .order_by_desc(Column::Id)
        .one(db)
        .await
}

/// Whether the `(notation_id, machine_kind)` machine has reached
/// `END`. Equivalent to `latest_for_kind(...).to_state == "END"`.
pub async fn is_complete(
    db: &impl ConnectionTrait,
    notation_id: Uuid,
    machine_kind: &str,
) -> Result<bool, DbErr> {
    Ok(latest_for_kind(db, notation_id, machine_kind)
        .await?
        .is_some_and(|m| m.to_state == "END"))
}

/// One transition's worth of data to journal. Carries everything
/// the `notation_events` row needs in a single struct so
/// [`append_event`] stays under clippy's argument budget and
/// reads as one logical record at the call site.
pub struct TransitionRecord<'a> {
    pub notation_id: Uuid,
    pub machine_kind: &'a str,
    pub from_state: &'a str,
    pub to_state: &'a str,
    pub condition: &'a str,
    /// Opaque JSON text — typically `Some(r#"{"answer_value":"…"}"#)`
    /// for a questionnaire signal that carries a respondent's
    /// answer, `None` for a workflow signal.
    pub payload_json: Option<String>,
    /// RFC 3339 / ISO 8601. Callers from the Restate worker pass
    /// `chrono::Utc::now().to_rfc3339()` so a replay reuses the
    /// captured timestamp via Restate's journal cache.
    pub recorded_at: &'a str,
}

/// Append one row to `notation_events`.
pub async fn append_event<C>(db: &C, record: TransitionRecord<'_>) -> Result<Model, DbErr>
where
    C: ConnectionTrait,
{
    ActiveModel {
        id: ActiveValue::Set(Uuid::now_v7()),
        notation_id: ActiveValue::Set(record.notation_id),
        machine_kind: ActiveValue::Set(record.machine_kind.to_string()),
        from_state: ActiveValue::Set(record.from_state.to_string()),
        to_state: ActiveValue::Set(record.to_state.to_string()),
        condition: ActiveValue::Set(record.condition.to_string()),
        payload: ActiveValue::Set(record.payload_json),
        recorded_at: ActiveValue::Set(record.recorded_at.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
}

/// Encode a questionnaire-answer payload as the JSON the
/// `payload` column expects.
#[must_use]
pub fn answer_payload(answer_value: &str) -> String {
    serde_json::json!({ "answer_value": answer_value }).to_string()
}
