//! Helpers for the `contract_reviews` table — the per-notation work-product
//! satellite for an inbound contract review.
//!
//! The `findings` column is JSONB; this module owns the typed view
//! ([`Finding`]) and the (de)serialization. The lifecycle: [`create`] opens
//! a review at `pending`; [`record_analysis`] writes the deviation findings
//! and risk summary (`analyzed`); the reviewing attorney edits via
//! [`update_findings`] and closes with [`set_status`]
//! (`approved` / `rejected`). Per-finding attribution (who accepted what) is
//! the matter's audit trail and lives in `notation_events`, not here.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::contract_review::{self, STATUS_ANALYZED, STATUS_PENDING};
use crate::Db;

/// One deviation the analysis found between the inbound contract and the
/// client's playbook — the unit the reviewing attorney acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Where in the contract the deviation sits (e.g. `§7.2 Liability`).
    pub clause_ref: String,
    /// How the clause deviates from the playbook position.
    pub deviation: String,
    /// Severity: see [`crate::playbooks`] `SEVERITY_*` constants.
    pub severity: String,
    /// A suggested redline; `None` when none is proposed.
    #[serde(default)]
    pub suggested_redline: Option<String>,
    /// The reviewing attorney's note; `None` until the attorney edits.
    #[serde(default)]
    pub attorney_note: Option<String>,
    /// Whether the reviewing attorney has acted on (accepted) this finding.
    /// Defaults to `false`: nothing is accepted until the attorney acts.
    #[serde(default)]
    pub accepted: bool,
}

/// What to record for one new contract review.
#[derive(Debug, Clone, Copy)]
pub struct NewContractReview {
    pub notation_id: Uuid,
    pub playbook_id: Uuid,
    /// The filed inbound-contract document; `None` until uploaded.
    pub document_id: Option<Uuid>,
}

/// Open one `contract_reviews` row at `status = pending` with no findings,
/// returning its id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn create(db: &Db, new: &NewContractReview) -> Result<Uuid, sea_orm::DbErr> {
    let row = contract_review::ActiveModel {
        notation_id: ActiveValue::Set(new.notation_id),
        playbook_id: ActiveValue::Set(new.playbook_id),
        document_id: ActiveValue::Set(new.document_id),
        status: ActiveValue::Set(STATUS_PENDING.to_string()),
        risk_summary: ActiveValue::Set(None),
        findings: ActiveValue::Set(serde_json::Value::Array(Vec::new())),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Load one contract review by id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<contract_review::Model>, sea_orm::DbErr> {
    contract_review::Entity::find_by_id(id).one(db).await
}

/// The most recent contract review for a notation, if any.
///
/// # Errors
///
/// Propagates any database error.
pub async fn latest_for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Option<contract_review::Model>, sea_orm::DbErr> {
    contract_review::Entity::find()
        .filter(contract_review::Column::NotationId.eq(notation_id))
        .order_by_desc(contract_review::Column::Id)
        .one(db)
        .await
}

/// Record the analysis result: store the risk summary and findings and
/// advance the row to `analyzed`.
///
/// # Errors
///
/// Propagates any database error, or [`sea_orm::DbErr::RecordNotFound`] if
/// the id is unknown.
pub async fn record_analysis(
    db: &Db,
    id: Uuid,
    risk_summary: &str,
    findings: &[Finding],
) -> Result<(), sea_orm::DbErr> {
    let value = serde_json::to_value(findings).map_err(|e| json_to_db_err(&e))?;
    let mut active = load_active(db, id).await?;
    active.risk_summary = ActiveValue::Set(Some(risk_summary.to_string()));
    active.findings = ActiveValue::Set(value);
    active.status = ActiveValue::Set(STATUS_ANALYZED.to_string());
    active.update(db).await?;
    Ok(())
}

/// Replace the findings (the reviewing attorney's per-finding edits).
///
/// # Errors
///
/// Propagates any database error, or [`sea_orm::DbErr::RecordNotFound`].
pub async fn update_findings(
    db: &Db,
    id: Uuid,
    findings: &[Finding],
) -> Result<(), sea_orm::DbErr> {
    let value = serde_json::to_value(findings).map_err(|e| json_to_db_err(&e))?;
    let mut active = load_active(db, id).await?;
    active.findings = ActiveValue::Set(value);
    active.update(db).await?;
    Ok(())
}

/// Replace the risk summary (the reviewing attorney's edit), leaving the
/// findings and status untouched.
///
/// # Errors
///
/// Propagates any database error, or [`sea_orm::DbErr::RecordNotFound`].
pub async fn update_risk_summary(
    db: &Db,
    id: Uuid,
    risk_summary: &str,
) -> Result<(), sea_orm::DbErr> {
    let mut active = load_active(db, id).await?;
    active.risk_summary = ActiveValue::Set(Some(risk_summary.to_string()));
    active.update(db).await?;
    Ok(())
}

/// Set the review status (`approved` / `rejected`).
///
/// # Errors
///
/// Propagates any database error, or [`sea_orm::DbErr::RecordNotFound`].
pub async fn set_status(db: &Db, id: Uuid, status: &str) -> Result<(), sea_orm::DbErr> {
    let mut active = load_active(db, id).await?;
    active.status = ActiveValue::Set(status.to_string());
    active.update(db).await?;
    Ok(())
}

/// The typed findings stored on a contract-review row.
///
/// # Errors
///
/// Returns a JSON error if the stored `findings` value is not a
/// `Vec<Finding>` (a schema/data drift, never expected at runtime).
pub fn findings_of(model: &contract_review::Model) -> Result<Vec<Finding>, serde_json::Error> {
    serde_json::from_value(model.findings.clone())
}

async fn load_active(db: &Db, id: Uuid) -> Result<contract_review::ActiveModel, sea_orm::DbErr> {
    let model = contract_review::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("contract_review {id}")))?;
    Ok(model.into())
}

fn json_to_db_err(e: &serde_json::Error) -> sea_orm::DbErr {
    sea_orm::DbErr::Custom(format!("contract_review findings JSON: {e}"))
}
