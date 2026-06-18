//! Helpers for the `playbooks` table — a client Entity's stored
//! contract-negotiation positions.
//!
//! The `positions` column is JSONB; this module owns the typed view
//! ([`Position`]) and the (de)serialization, so `web` and the
//! contract-review analysis reach a `Vec<Position>` rather than a raw
//! `serde_json::Value`. A playbook is scoped to the client Entity, so one
//! playbook serves every matter for that client. See
//! [`m20260721_create_contract_review_tables`](super::migration).

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::playbook;
use crate::Db;

/// Severity a deviation from this position carries: `low` | `medium` |
/// `high`. Descriptive — used to rank findings, not enforced.
pub const SEVERITY_LOW: &str = "low";
pub const SEVERITY_MEDIUM: &str = "medium";
pub const SEVERITY_HIGH: &str = "high";

/// One stored position in a playbook — the firm's stance on a single
/// contract topic, with its fallback and walk-away lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// The contract topic this position governs (e.g. `Limitation of
    /// liability`, `Auto-renewal`, `Governing law`).
    pub topic: String,
    /// The preferred outcome the client wants.
    pub preferred: String,
    /// The acceptable fallback if the preferred outcome is refused.
    pub fallback: String,
    /// The line past which the client should not sign.
    pub walkaway: String,
    /// Severity of a deviation: see the `SEVERITY_*` constants.
    pub severity: String,
}

/// What to record for one new playbook.
#[derive(Debug, Clone)]
pub struct NewPlaybook<'a> {
    pub entity_id: Uuid,
    /// Human label, unique per Entity (e.g. `SaaS vendor MSA`).
    pub name: &'a str,
    pub positions: &'a [Position],
}

/// Insert one active `playbooks` row, returning its id.
///
/// # Errors
///
/// Propagates any database error (including a unique-constraint violation
/// on `(entity_id, name)`).
pub async fn create(db: &Db, new: &NewPlaybook<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let positions = serde_json::to_value(new.positions).map_err(|e| json_to_db_err(&e))?;
    let row = playbook::ActiveModel {
        entity_id: ActiveValue::Set(new.entity_id),
        name: ActiveValue::Set(new.name.to_string()),
        positions: ActiveValue::Set(positions),
        active: ActiveValue::Set(true),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Load one playbook by id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<playbook::Model>, sea_orm::DbErr> {
    playbook::Entity::find_by_id(id).one(db).await
}

/// All playbooks for an Entity, name order.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_entity(db: &Db, entity_id: Uuid) -> Result<Vec<playbook::Model>, sea_orm::DbErr> {
    playbook::Entity::find()
        .filter(playbook::Column::EntityId.eq(entity_id))
        .order_by_asc(playbook::Column::Name)
        .all(db)
        .await
}

/// Replace a playbook's positions (the admin editor saves the whole set).
///
/// # Errors
///
/// Propagates any database error.
pub async fn update_positions(
    db: &Db,
    id: Uuid,
    positions: &[Position],
) -> Result<(), sea_orm::DbErr> {
    let value = serde_json::to_value(positions).map_err(|e| json_to_db_err(&e))?;
    let mut active: playbook::ActiveModel = playbook::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("playbook {id}")))?
        .into();
    active.positions = ActiveValue::Set(value);
    active.update(db).await?;
    Ok(())
}

/// The typed positions stored on a playbook row.
///
/// # Errors
///
/// Returns a JSON error if the stored `positions` value is not a
/// `Vec<Position>` (a schema/data drift, never expected at runtime).
pub fn positions_of(model: &playbook::Model) -> Result<Vec<Position>, serde_json::Error> {
    serde_json::from_value(model.positions.clone())
}

/// Bridge a JSON (de)serialization failure into a `DbErr` so the helpers
/// keep one error type. Such a failure means the stored JSON drifted from
/// the typed shape — a programming/data error, not a user error.
fn json_to_db_err(e: &serde_json::Error) -> sea_orm::DbErr {
    sea_orm::DbErr::Custom(format!("playbook positions JSON: {e}"))
}
