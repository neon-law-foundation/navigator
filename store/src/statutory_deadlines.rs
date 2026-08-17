//! Durable Project-level legal clocks.
//!
//! The replay key is a natural key: repeating a workflow decision updates the
//! same deadline rather than creating a second statutory window.

use chrono::NaiveDate;
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::projects;
use crate::surreal::{record_id, record_uuid, SurrealDb};

pub const STATUS_OPEN: &str = "open";
pub const STATUS_SATISFIED: &str = "satisfied";
pub const STATUS_CLOSED: &str = "closed";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatutoryDeadline {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub trigger_on: NaiveDate,
    pub due_on: NaiveDate,
    pub statute: String,
    pub source: String,
    pub status: String,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(SurrealValue)]
struct DeadlineRow {
    id: surrealdb::types::RecordId,
    project_id: surrealdb::types::RecordId,
    kind: String,
    trigger_on: String,
    due_on: String,
    statute: String,
    source: String,
    status: String,
    inserted_at: String,
    updated_at: String,
}

impl DeadlineRow {
    fn into_deadline(self) -> Option<StatutoryDeadline> {
        Some(StatutoryDeadline {
            id: record_uuid(&self.id)?,
            project_id: record_uuid(&self.project_id)?,
            kind: self.kind,
            trigger_on: self.trigger_on.parse().ok()?,
            due_on: self.due_on.parse().ok()?,
            statute: self.statute,
            source: self.source,
            status: self.status,
            inserted_at: self.inserted_at,
            updated_at: self.updated_at,
        })
    }
}

const DEADLINE_SELECT: &str =
    "id, project_id, kind, trigger_on, due_on, statute, source, status, inserted_at, updated_at";

#[derive(Debug, thiserror::Error)]
pub enum DeadlineError {
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    #[error(transparent)]
    Project(#[from] projects::ProjectStoreError),
    #[error("writing a statutory deadline returned no usable row")]
    WriteReturnedNothing,
}

#[derive(Debug, Clone)]
pub struct NewStatutoryDeadline<'a> {
    pub project_id: Uuid,
    pub kind: &'a str,
    pub trigger_on: NaiveDate,
    pub due_on: NaiveDate,
    pub statute: &'a str,
    pub source: &'a str,
}

/// Record one deadline idempotently. The project read-back is the
/// reference check: a typed record link does not prove the target exists.
pub async fn record(
    surreal: &SurrealDb,
    new: &NewStatutoryDeadline<'_>,
) -> Result<StatutoryDeadline, DeadlineError> {
    // The matter opener's command boundary validates the project before
    // this append, so the record link is deliberately not re-validated
    // here.
    let now = chrono::Utc::now().to_rfc3339();
    let existing = find_replay(surreal, new).await?;
    let id = existing
        .as_ref()
        .map_or_else(Uuid::now_v7, |deadline| deadline.id);
    let statement = if existing.is_some() {
        format!(
            "UPDATE $id SET due_on = $due_on, statute = $statute, status = $status, \
             updated_at = $now RETURN {DEADLINE_SELECT}"
        )
    } else {
        format!(
            "CREATE $id SET project_id = $project_id, kind = $kind, trigger_on = $trigger_on, \
             due_on = $due_on, statute = $statute, source = $source, status = $status, \
             inserted_at = $now, updated_at = $now RETURN {DEADLINE_SELECT}"
        )
    };
    let mut response = surreal
        .query(statement)
        .bind(("id", record_id("statutory_deadline", id)))
        .bind(("project_id", record_id("project", new.project_id)))
        .bind(("kind", new.kind.to_string()))
        .bind(("trigger_on", new.trigger_on.to_string()))
        .bind(("due_on", new.due_on.to_string()))
        .bind(("statute", new.statute.to_string()))
        .bind(("source", new.source.to_string()))
        .bind(("status", STATUS_OPEN.to_string()))
        .bind(("now", now))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<DeadlineRow> = response.take(0)?;
    row.and_then(DeadlineRow::into_deadline)
        .ok_or(DeadlineError::WriteReturnedNothing)
}

async fn find_replay(
    surreal: &SurrealDb,
    new: &NewStatutoryDeadline<'_>,
) -> Result<Option<StatutoryDeadline>, DeadlineError> {
    let mut response = surreal
        .query(format!(
            "SELECT {DEADLINE_SELECT} FROM ONLY statutory_deadline \
             WHERE project_id = $project_id AND kind = $kind AND trigger_on = $trigger_on \
             AND source = $source LIMIT 1"
        ))
        .bind(("project_id", record_id("project", new.project_id)))
        .bind(("kind", new.kind.to_string()))
        .bind(("trigger_on", new.trigger_on.to_string()))
        .bind(("source", new.source.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<DeadlineRow> = response.take(0)?;
    Ok(row.and_then(DeadlineRow::into_deadline))
}

/// Record every deadline in one triage decision.
pub async fn record_all(
    surreal: &SurrealDb,
    deadlines: &[NewStatutoryDeadline<'_>],
) -> Result<Vec<StatutoryDeadline>, DeadlineError> {
    let mut rows = Vec::with_capacity(deadlines.len());
    for deadline in deadlines {
        rows.push(record(surreal, deadline).await?);
    }
    Ok(rows)
}

/// Deadlines currently recorded for a Project, ordered by due date then id.
pub async fn by_project(
    surreal: &SurrealDb,
    project_id: Uuid,
) -> Result<Vec<StatutoryDeadline>, DeadlineError> {
    let mut response = surreal
        .query(format!(
            "SELECT {DEADLINE_SELECT} FROM statutory_deadline WHERE project_id = $project_id \
             ORDER BY due_on, id"
        ))
        .bind(("project_id", record_id("project", project_id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<DeadlineRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(DeadlineRow::into_deadline)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{create, NewProject};
    use crate::test_support::mem_surreal;

    #[tokio::test]
    async fn replay_key_updates_one_deadline() {
        let surreal = mem_surreal().await;
        let project = create(
            &surreal,
            &NewProject {
                code: "deadline-replay".into(),
                name: "Deadline replay".into(),
                status: "open".into(),
                entity_id: Uuid::now_v7(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let original = NewStatutoryDeadline {
            project_id: project.id,
            kind: "fcra",
            trigger_on: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            due_on: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            statute: "15 USC 1681i",
            source: "notice:1",
        };
        let first = record(&surreal, &original).await.unwrap();
        let changed = NewStatutoryDeadline {
            due_on: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            ..original
        };
        let second = record(&surreal, &changed).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.due_on, changed.due_on);
        assert_eq!(by_project(&surreal, project.id).await.unwrap().len(), 1);
    }
}
