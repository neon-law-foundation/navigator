//! Record governed expunges (design §9).
//!
//! The legal council's requirement: when a matter repo's history is
//! rewritten to remove privileged / sealed / lawfully-deleted material,
//! the expunge *itself* is recorded — who authorized it, when, and the
//! category — so the redaction is auditable. This module is the
//! write/read seam for that audit log; the `portal::expunge` orchestrator
//! calls [`record`] after the repo rewrite + LFS deletion.
//!
//! # This table lives in SurrealDB
//!
//! `expunge_records` moved with wave six of #1093 (ENG-160), in the
//! expungement slice alongside [`crate::expunge_requests`].

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use thiserror::Error;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

/// What can go wrong reading or writing the audit trail.
///
/// A typed error rather than a bare [`surrealdb::Error`] because `portal`
/// and `webapp` consume this module and do not depend on the SurrealDB
/// crate — the same shape [`crate::persons::PersonError`] uses.
#[derive(Debug, Error)]
pub enum ExpungeRecordError {
    /// A database operation failed.
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    /// The write was refused because `category` is outside the
    /// `CATEGORY_*` set — the schema ASSERT rejecting an unclassifiable
    /// audit row.
    #[error("unknown expunge category `{0}` (expected privilege | sealing | client_request)")]
    BadCategory(String),
}

/// The table these rows live in.
pub(crate) const TABLE: &str = "expunge_record";
const PERSON_TABLE: &str = "person";

/// Privilege clawback — material committed in error that is privileged.
pub const CATEGORY_PRIVILEGE: &str = "privilege";
/// A court sealing order.
pub const CATEGORY_SEALING: &str = "sealing";
/// A client's lawful deletion request.
pub const CATEGORY_CLIENT_REQUEST: &str = "client_request";

/// One governed-expunge audit row.
///
/// The application-facing shape: plain Rust types, no engine handles.
/// [`ExpungeRecordRow`] is the seam that turns it into (and back out of)
/// what the SDK reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpungeRecord {
    pub id: Uuid,
    /// The matter whose repo was rewritten.
    pub project_id: Uuid,
    /// Repo path removed from all history (metadata, not content).
    pub path: String,
    /// One of the `CATEGORY_*` constants.
    pub category: String,
    /// The [`crate::persons`] row that authorized the expunge.
    pub authorized_by_person_id: Uuid,
    /// `main` oid before the rewrite (`None` if the repo was empty).
    pub head_before: Option<String>,
    /// `main` oid after the rewrite.
    pub head_after: Option<String>,
    /// Optional non-content note (e.g. a docket reference).
    pub note: Option<String>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct ExpungeRecordRow {
    id: surrealdb::types::RecordId,
    project_id: surrealdb::types::RecordId,
    path: String,
    category: String,
    authorized_by_person_id: surrealdb::types::RecordId,
    head_before: Option<String>,
    head_after: Option<String>,
    note: Option<String>,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl ExpungeRecordRow {
    /// `None` when a record id is not a native UUID key — a row written by
    /// something that bypassed [`crate::surreal::record_id`].
    fn into_record(self) -> Option<ExpungeRecord> {
        Some(ExpungeRecord {
            id: record_uuid(&self.id)?,
            project_id: record_uuid(&self.project_id)?,
            path: self.path,
            category: self.category,
            authorized_by_person_id: record_uuid(&self.authorized_by_person_id)?,
            head_before: self.head_before,
            head_after: self.head_after,
            note: self.note,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares, so one field list describes the row
/// and a new column cannot reach [`ExpungeRecordRow`] from only one query.
const SELECT: &str = "id, project_id, path, category, authorized_by_person_id, \
                      head_before, head_after, note, inserted_at, updated_at";

/// Inputs to [`record`].
#[derive(Debug, Clone)]
pub struct NewExpunge<'a> {
    /// The matter whose repo was rewritten.
    pub project_id: Uuid,
    /// The repo path removed (metadata, not content).
    pub path: &'a str,
    /// One of the `CATEGORY_*` constants.
    pub category: &'a str,
    /// The admin who authorized the expunge.
    pub authorized_by_person_id: Uuid,
    /// `main` oid before the rewrite.
    pub head_before: Option<&'a str>,
    /// `main` oid after the rewrite.
    pub head_after: Option<&'a str>,
    /// Optional non-content note (e.g. a docket reference).
    pub note: Option<&'a str>,
}

/// Insert one expunge audit row, returning its id. No unique index guards
/// this insert, so no retry wraps it — each call mints its own fresh id,
/// and a second expunge of the same path is a second auditable event.
///
/// # Errors
///
/// [`ExpungeRecordError::BadCategory`] when the caller passes a category
/// outside the `CATEGORY_*` set, or any other database error.
pub async fn record(db: &SurrealDb, new: &NewExpunge<'_>) -> Result<Uuid, ExpungeRecordError> {
    let id = Uuid::now_v7();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             project_id = $project_id, path = $path, category = $category, \
             authorized_by_person_id = $authorized_by, head_before = $head_before, \
             head_after = $head_after, note = $note \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "project_id",
            record_id(crate::projects::PROJECT_TABLE, new.project_id),
        ))
        .bind(("path", new.path.to_string()))
        .bind(("category", new.category.to_string()))
        .bind((
            "authorized_by",
            record_id(PERSON_TABLE, new.authorized_by_person_id),
        ))
        .bind(("head_before", new.head_before.map(str::to_string)))
        .bind(("head_after", new.head_after.map(str::to_string)))
        .bind(("note", new.note.map(str::to_string)))
        .await
        .and_then(surrealdb::IndexedResults::check)
        .map_err(|error| classify(error, new.category))?;
    let row: Option<ExpungeRecordRow> = response.take(0)?;
    Ok(row
        .and_then(ExpungeRecordRow::into_record)
        .map_or(id, |r| r.id))
}

/// Load one audit row by id — the completed-expunge screen reads it back
/// from the id its redirect carries.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &SurrealDb, id: Uuid) -> Result<Option<ExpungeRecord>, ExpungeRecordError> {
    let mut response = db
        .query(format!("SELECT {SELECT} FROM $id"))
        .bind(("id", record_id(TABLE, id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<ExpungeRecordRow> = response.take(0)?;
    Ok(row.and_then(ExpungeRecordRow::into_record))
}

/// Every expunge recorded against a matter, oldest first — the matter's
/// redaction history. Rides `expunge_record_project`.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_project(
    db: &SurrealDb,
    project_id: Uuid,
) -> Result<Vec<ExpungeRecord>, ExpungeRecordError> {
    let mut response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE project_id = $project ORDER BY id ASC"
        ))
        .bind((
            "project",
            record_id(crate::projects::PROJECT_TABLE, project_id),
        ))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<ExpungeRecordRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(ExpungeRecordRow::into_record)
        .collect())
}

/// Whether any expunge audit row is scoped to this Project — the
/// matter-delete guard.
///
/// # Errors
/// Propagates any database error.
pub async fn exists_for_project(
    db: &SurrealDb,
    project_id: Uuid,
) -> Result<bool, ExpungeRecordError> {
    let mut response = db
        .query(format!(
            "SELECT VALUE id FROM {TABLE} WHERE project_id = $project LIMIT 1"
        ))
        .bind((
            "project",
            record_id(crate::projects::PROJECT_TABLE, project_id),
        ))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let ids: Vec<surrealdb::types::RecordId> = response.take(0)?;
    Ok(!ids.is_empty())
}

/// Name the `category` ASSERT specifically, so a caller that sent an
/// unrecognized category gets that answer instead of an opaque database
/// error it cannot act on.
fn classify(error: surrealdb::Error, category: &str) -> ExpungeRecordError {
    if error.to_string().contains("category") {
        return ExpungeRecordError::BadCategory(category.to_string());
    }
    ExpungeRecordError::Db(error)
}

#[cfg(test)]
mod tests {
    use super::{
        by_id, for_project, record, NewExpunge, CATEGORY_CLIENT_REQUEST, CATEGORY_SEALING,
    };
    use crate::surreal::test_support::mem;
    use crate::test_support::seed_project_surreal;

    #[tokio::test]
    async fn record_persists_the_audit_row() {
        let surreal = mem().await;
        let admin = crate::persons::create(
            &surreal,
            &crate::persons::NewPerson::new("Nick", "nick@neonlaw.com"),
        )
        .await
        .unwrap()
        .id;
        let proj = seed_project_surreal(&surreal, "matter").await;

        let id = record(
            &surreal,
            &NewExpunge {
                project_id: proj,
                path: "privileged.pdf",
                category: CATEGORY_SEALING,
                authorized_by_person_id: admin,
                head_before: Some("a".repeat(40).as_str()),
                head_after: Some("b".repeat(40).as_str()),
                note: Some("sealed per docket 24-CV-1"),
            },
        )
        .await
        .unwrap();

        let row = by_id(&surreal, id).await.unwrap().expect("expunge row");
        assert_eq!(row.project_id, proj);
        assert_eq!(row.category, CATEGORY_SEALING);
        assert_eq!(row.authorized_by_person_id, admin);
        assert_eq!(row.path, "privileged.pdf");
        assert_eq!(row.note.as_deref(), Some("sealed per docket 24-CV-1"));
        assert_eq!(row.head_before.as_deref(), Some("a".repeat(40).as_str()));
    }

    #[tokio::test]
    async fn a_category_outside_the_constants_is_refused() {
        // The audit trail's whole value is that every row classifies, and
        // the Surreal ASSERT is what refuses one that does not.
        let surreal = mem().await;
        let admin = crate::persons::create(
            &surreal,
            &crate::persons::NewPerson::new("Nick", "nick@neonlaw.com"),
        )
        .await
        .unwrap()
        .id;
        let proj = seed_project_surreal(&surreal, "matter").await;

        let err = record(
            &surreal,
            &NewExpunge {
                project_id: proj,
                path: "privileged.pdf",
                category: "because-i-said-so",
                authorized_by_person_id: admin,
                head_before: None,
                head_after: None,
                note: None,
            },
        )
        .await
        .expect_err("an unrecognized category must not reach the audit trail");
        assert!(
            matches!(err, super::ExpungeRecordError::BadCategory(ref c) if c == "because-i-said-so"),
            "the caller needs to know the category was the problem; got: {err}"
        );
    }

    #[tokio::test]
    async fn for_project_returns_a_matters_redactions_oldest_first() {
        let surreal = mem().await;
        let admin = crate::persons::create(
            &surreal,
            &crate::persons::NewPerson::new("Nick", "nick@neonlaw.com"),
        )
        .await
        .unwrap()
        .id;
        let mine = seed_project_surreal(&surreal, "mine").await;
        let other = seed_project_surreal(&surreal, "other").await;

        let mut expected = Vec::new();
        for path in ["first.pdf", "second.pdf"] {
            expected.push(
                record(
                    &surreal,
                    &NewExpunge {
                        project_id: mine,
                        path,
                        category: CATEGORY_SEALING,
                        authorized_by_person_id: admin,
                        head_before: None,
                        head_after: None,
                        note: None,
                    },
                )
                .await
                .unwrap(),
            );
        }
        // A redaction on a different matter must not appear.
        record(
            &surreal,
            &NewExpunge {
                project_id: other,
                path: "theirs.pdf",
                category: CATEGORY_SEALING,
                authorized_by_person_id: admin,
                head_before: None,
                head_after: None,
                note: None,
            },
        )
        .await
        .unwrap();

        let rows = for_project(&surreal, mine).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            expected,
            "UUIDv7 ids are time-sortable, so ORDER BY id is oldest first"
        );
        assert!(for_project(&surreal, uuid::Uuid::now_v7())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn by_id_is_none_for_an_unknown_row() {
        let surreal = mem().await;
        assert!(by_id(&surreal, uuid::Uuid::now_v7())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn every_category_constant_satisfies_the_assert() {
        // The Rust constants and the schema ASSERT are two lists of the same
        // set; this test is what keeps them from drifting apart.
        let surreal = mem().await;
        let admin = crate::persons::create(
            &surreal,
            &crate::persons::NewPerson::new("Nick", "nick@neonlaw.com"),
        )
        .await
        .unwrap()
        .id;
        let proj = seed_project_surreal(&surreal, "matter").await;

        for category in [
            super::CATEGORY_PRIVILEGE,
            CATEGORY_SEALING,
            CATEGORY_CLIENT_REQUEST,
        ] {
            record(
                &surreal,
                &NewExpunge {
                    project_id: proj,
                    path: "privileged.pdf",
                    category,
                    authorized_by_person_id: admin,
                    head_before: None,
                    head_after: None,
                    note: None,
                },
            )
            .await
            .unwrap_or_else(|e| panic!("category `{category}` must be accepted: {e}"));
        }
    }
}
