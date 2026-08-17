//! `filings` — one durable record per outbound compliance submission, and
//! every query against the table.
//!
//! # This table lives in SurrealDB
//!
//! `filings` moved with wave five of #1093 (ENG-121), in the satellite-ring
//! slice.
//!
//! Written by the workflow worker inside a submission step's `ctx.run`
//! (`mailroom_send`, `certified_mail`, `e_filing`, `filing__*`), so the row
//! is the replay-idempotent proof of what was filed. A row exists only
//! after the matter passed `lawyer_review` — the workflow spec guarantees no
//! submission state is reachable without a review first
//! (`workflows::lawyer_review_precedes_submission`).

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

/// The table these rows live in.
const TABLE: &str = "filing";

/// One durable filing record.
///
/// The application-facing shape: plain Rust types, no engine handles.
/// [`FilingRow`] is the seam that turns it into (and back out of) what the
/// SDK reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Filing {
    pub id: Uuid,
    pub notation_id: Uuid,
    /// Submission step kind / state prefix (`mailroom_send`, `certified_mail`,
    /// `e_filing`, `filing`).
    pub kind: String,
    /// Recipient office or party (e.g. `Nevada Secretary of State`).
    pub office: String,
    /// Provider/office tracking reference; `None` until one is known.
    pub reference: Option<String>,
    /// Human-readable summary of what was submitted.
    pub summary: String,
    /// RFC 3339 timestamp the submission side effect fired.
    pub submitted_at: String,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct FilingRow {
    id: surrealdb::types::RecordId,
    notation_id: surrealdb::types::RecordId,
    kind: String,
    office: String,
    reference: Option<String>,
    summary: String,
    submitted_at: String,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl FilingRow {
    /// `None` when a record id is not a native UUID key — a row written by
    /// something that bypassed [`crate::surreal::record_id`].
    fn into_filing(self) -> Option<Filing> {
        Some(Filing {
            id: record_uuid(&self.id)?,
            notation_id: record_uuid(&self.notation_id)?,
            kind: self.kind,
            office: self.office,
            reference: self.reference,
            summary: self.summary,
            submitted_at: self.submitted_at,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares, so one field list describes the row
/// and a new column cannot reach [`FilingRow`] from only one query.
const SELECT: &str =
    "id, notation_id, kind, office, reference, summary, submitted_at, inserted_at, updated_at";

/// What to record for one submission. `submitted_at` is stamped by the
/// caller (the worker stamps it inside the journaled step so a replay
/// reuses the same timestamp).
#[derive(Debug, Clone)]
pub struct NewFiling<'a> {
    pub notation_id: Uuid,
    pub kind: &'a str,
    pub office: &'a str,
    pub summary: &'a str,
    pub reference: Option<&'a str>,
    pub submitted_at: &'a str,
}

/// Insert one `filings` row, returning its id. No unique index guards this
/// insert, so no retry wraps it — each call mints its own fresh id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn record(db: &SurrealDb, new: &NewFiling<'_>) -> Result<Uuid, surrealdb::Error> {
    let id = Uuid::now_v7();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             notation_id = $notation_id, kind = $kind, office = $office, \
             reference = $reference, summary = $summary, submitted_at = $submitted_at \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "notation_id",
            record_id(crate::notations::TABLE, new.notation_id),
        ))
        .bind(("kind", new.kind.to_string()))
        .bind(("office", new.office.to_string()))
        .bind(("reference", new.reference.map(str::to_string)))
        .bind(("summary", new.summary.to_string()))
        .bind(("submitted_at", new.submitted_at.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<FilingRow> = response.take(0)?;
    Ok(row.and_then(FilingRow::into_filing).map_or(id, |f| f.id))
}

/// All filings recorded for a notation, oldest first.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_notation(
    db: &SurrealDb,
    notation_id: Uuid,
) -> Result<Vec<Filing>, surrealdb::Error> {
    let mut response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE notation_id = $notation ORDER BY id ASC"
        ))
        .bind(("notation", record_id(crate::notations::TABLE, notation_id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<FilingRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(FilingRow::into_filing)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{for_notation, record, NewFiling};
    use crate::surreal::test_support::mem;
    use crate::test_support::seed_notation;

    #[tokio::test]
    async fn record_inserts_a_filing_row_readable_by_notation() {
        let surreal = mem().await;
        let notation_id = seed_notation(&surreal).await;
        let id = record(
            &surreal,
            &NewFiling {
                notation_id,
                kind: "mailroom_send",
                office: "Nevada Secretary of State",
                summary: "Annual report mailed",
                reference: None,
                submitted_at: "2026-06-01T00:00:00Z",
            },
        )
        .await
        .unwrap();

        let rows = for_notation(&surreal, notation_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].kind, "mailroom_send");
        assert_eq!(rows[0].office, "Nevada Secretary of State");
        assert!(rows[0].reference.is_none());
    }
}
