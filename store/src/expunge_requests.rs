//! Helpers for the `expunge_requests` table — a client's request to
//! delete one of their matter documents, awaiting attorney
//! authorization.
//!
//! A client can only *ask*: [`create`] inserts a `pending` row. A
//! lawyer/admin then resolves it — [`authorize`] (after running the
//! admin-gated expunge, passing the resulting audit-row id) or
//! [`deny`]. The executed expunge is always category `client_request`.
//! See [`crate::expunge_records`] and the design §9.
//!
//! # This table lives in SurrealDB
//!
//! `expunge_requests` moved with wave six of #1093 (ENG-160), in the
//! expungement slice alongside [`crate::expunge_records`].

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use thiserror::Error;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, SurrealDb};

/// What can go wrong reading or writing a deletion request.
///
/// A typed error rather than a bare [`surrealdb::Error`] because `portal`
/// and `webapp` consume this module and do not depend on the SurrealDB
/// crate — the same shape [`crate::persons::PersonError`] uses.
#[derive(Debug, Error)]
pub enum ExpungeRequestError {
    /// A database operation failed.
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
}

/// The table these rows live in.
const TABLE: &str = "expunge_request";
const PERSON_TABLE: &str = "person";

/// Awaiting lawyer/admin review — the default for a new request.
pub const STATUS_PENDING: &str = "pending";
/// Authorized — the admin-gated expunge has run; `expunge_record_id` is
/// set.
pub const STATUS_AUTHORIZED: &str = "authorized";
/// Denied by a lawyer/admin; nothing was deleted.
pub const STATUS_DENIED: &str = "denied";

/// One client deletion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpungeRequest {
    pub id: Uuid,
    /// The matter the document belongs to.
    pub project_id: Uuid,
    /// The document the client wants deleted.
    pub asset_id: Uuid,
    /// The client who requested deletion.
    pub requested_by_person_id: Uuid,
    /// One of the `STATUS_*` constants.
    pub status: String,
    /// Optional non-content note from the client (their stated reason).
    pub note: Option<String>,
    /// The lawyer/admin who resolved it. `None` while pending.
    pub resolved_by_person_id: Option<Uuid>,
    /// The audit row from the executed expunge. `None` unless authorized.
    pub expunge_record_id: Option<Uuid>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct ExpungeRequestRow {
    id: surrealdb::types::RecordId,
    project_id: surrealdb::types::RecordId,
    asset_id: surrealdb::types::RecordId,
    requested_by_person_id: surrealdb::types::RecordId,
    status: String,
    note: Option<String>,
    resolved_by_person_id: Option<surrealdb::types::RecordId>,
    expunge_record_id: Option<surrealdb::types::RecordId>,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl ExpungeRequestRow {
    /// `None` when a record id is not a native UUID key — a row written by
    /// something that bypassed [`crate::surreal::record_id`]. The two
    /// optional links map through the same check, so a malformed link
    /// reads as absent rather than dropping the whole request.
    fn into_request(self) -> Option<ExpungeRequest> {
        Some(ExpungeRequest {
            id: record_uuid(&self.id)?,
            project_id: record_uuid(&self.project_id)?,
            asset_id: record_uuid(&self.asset_id)?,
            requested_by_person_id: record_uuid(&self.requested_by_person_id)?,
            status: self.status,
            note: self.note,
            resolved_by_person_id: self.resolved_by_person_id.as_ref().and_then(record_uuid),
            expunge_record_id: self.expunge_record_id.as_ref().and_then(record_uuid),
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares.
const SELECT: &str = "id, project_id, asset_id, requested_by_person_id, status, note, \
                      resolved_by_person_id, expunge_record_id, inserted_at, updated_at";

/// What to record for one client deletion request.
#[derive(Debug, Clone)]
pub struct NewExpungeRequest<'a> {
    pub project_id: Uuid,
    pub asset_id: Uuid,
    /// The client asking for deletion.
    pub requested_by_person_id: Uuid,
    /// Optional non-content note (the client's stated reason).
    pub note: Option<&'a str>,
}

/// Insert one `expunge_requests` row at `status = pending`, returning its
/// id. The request never deletes anything on its own — a lawyer/admin must
/// authorize it.
///
/// # Errors
/// Propagates any database error.
pub async fn create(
    db: &SurrealDb,
    new: &NewExpungeRequest<'_>,
) -> Result<Uuid, ExpungeRequestError> {
    let id = Uuid::now_v7();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             project_id = $project_id, asset_id = $asset_id, \
             requested_by_person_id = $requested_by, status = $status, note = $note \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "project_id",
            record_id(crate::projects::PROJECT_TABLE, new.project_id),
        ))
        .bind(("asset_id", record_id(crate::assets::TABLE, new.asset_id)))
        .bind((
            "requested_by",
            record_id(PERSON_TABLE, new.requested_by_person_id),
        ))
        .bind(("status", STATUS_PENDING.to_string()))
        .bind(("note", new.note.map(str::to_string)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<ExpungeRequestRow> = response.take(0)?;
    Ok(row
        .and_then(ExpungeRequestRow::into_request)
        .map_or(id, |r| r.id))
}

/// Load one request by id.
///
/// # Errors
/// Propagates any database error.
pub async fn by_id(
    db: &SurrealDb,
    id: Uuid,
) -> Result<Option<ExpungeRequest>, ExpungeRequestError> {
    let mut response = db
        .query(format!("SELECT {SELECT} FROM $id"))
        .bind(("id", record_id(TABLE, id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<ExpungeRequestRow> = response.take(0)?;
    Ok(row.and_then(ExpungeRequestRow::into_request))
}

/// The pending request for a document, if any. Used to show the client
/// "deletion requested" instead of offering the control again.
///
/// # Errors
/// Propagates any database error.
pub async fn pending_for_document(
    db: &SurrealDb,
    asset_id: Uuid,
) -> Result<Option<ExpungeRequest>, ExpungeRequestError> {
    let mut response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} \
             WHERE asset_id = $asset AND status = $status LIMIT 1"
        ))
        .bind(("asset", record_id(crate::assets::TABLE, asset_id)))
        .bind(("status", STATUS_PENDING.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<ExpungeRequestRow> = response.take(0)?;
    Ok(rows.into_iter().find_map(ExpungeRequestRow::into_request))
}

/// Every pending request across all matters, oldest first — the lawyer
/// authorization queue.
///
/// # Errors
/// Propagates any database error.
pub async fn list_pending(db: &SurrealDb) -> Result<Vec<ExpungeRequest>, ExpungeRequestError> {
    let mut response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE status = $status ORDER BY id ASC"
        ))
        .bind(("status", STATUS_PENDING.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<ExpungeRequestRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(ExpungeRequestRow::into_request)
        .collect())
}

/// Whether any request — of any `status`, not only pending — is scoped to
/// this Project. The matter-delete guard.
///
/// A resolved request is as load-bearing as an open one: it records that
/// somebody asked for material to be expunged and what the firm decided,
/// so it must outlive nothing quietly.
///
/// # Errors
/// Propagates any database error.
pub async fn exists_for_project(
    db: &SurrealDb,
    project_id: Uuid,
) -> Result<bool, ExpungeRequestError> {
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

/// Mark a request `authorized`, recording who resolved it and the audit
/// row id from the executed expunge. Returns the updated row, or
/// `Ok(None)` if no row matched.
///
/// # Errors
/// Propagates any database error.
pub async fn authorize(
    db: &SurrealDb,
    id: Uuid,
    resolved_by_person_id: Uuid,
    expunge_record_id: Uuid,
) -> Result<Option<ExpungeRequest>, ExpungeRequestError> {
    resolve(
        db,
        id,
        STATUS_AUTHORIZED,
        resolved_by_person_id,
        Some(expunge_record_id),
    )
    .await
}

/// Mark a request `denied`, recording who resolved it. Nothing is
/// deleted. Returns the updated row, or `Ok(None)` if no row matched.
///
/// # Errors
/// Propagates any database error.
pub async fn deny(
    db: &SurrealDb,
    id: Uuid,
    resolved_by_person_id: Uuid,
) -> Result<Option<ExpungeRequest>, ExpungeRequestError> {
    resolve(db, id, STATUS_DENIED, resolved_by_person_id, None).await
}

/// The shared body of [`authorize`] and [`deny`] — the two differ only in
/// the status they land on and whether an audit row is linked.
///
/// `UPDATE $id ... WHERE` (rather than a read-then-write) is what keeps a
/// concurrent second resolution from clobbering the first: the statement
/// matches nothing when the row is already resolved, so the loser gets
/// `Ok(None)` instead of overwriting the winner's `resolved_by`.
async fn resolve(
    db: &SurrealDb,
    id: Uuid,
    status: &str,
    resolved_by_person_id: Uuid,
    expunge_record_id: Option<Uuid>,
) -> Result<Option<ExpungeRequest>, ExpungeRequestError> {
    let mut response = db
        .query(format!(
            "UPDATE $id SET \
             status = $status, resolved_by_person_id = $resolved_by, \
             expunge_record_id = $record, updated_at = time::now() \
             WHERE status = $pending \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind(("status", status.to_string()))
        .bind((
            "resolved_by",
            record_id(PERSON_TABLE, resolved_by_person_id),
        ))
        .bind((
            "record",
            expunge_record_id.map(|r| record_id(crate::expunge_records::TABLE, r)),
        ))
        .bind(("pending", STATUS_PENDING.to_string()))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let rows: Vec<ExpungeRequestRow> = response.take(0)?;
    Ok(rows.into_iter().find_map(ExpungeRequestRow::into_request))
}

#[cfg(test)]
mod tests {
    use super::{
        authorize, by_id, create, deny, list_pending, pending_for_document, NewExpungeRequest,
        STATUS_AUTHORIZED, STATUS_DENIED, STATUS_PENDING,
    };
    use crate::expunge_records::{self, NewExpunge, CATEGORY_CLIENT_REQUEST};
    use crate::persons::{self, NewPerson};
    use crate::surreal::test_support::mem;
    use crate::surreal::SurrealDb;
    use uuid::Uuid;

    /// Seed a (person, project, document) chain and return their ids.
    async fn seed(surreal: &SurrealDb) -> (Uuid, Uuid, Uuid) {
        let client = persons::create(surreal, &NewPerson::new("Libra", "libra@example.com"))
            .await
            .unwrap()
            .id;
        let proj = crate::test_support::seed_project_surreal(surreal, "Matter").await;
        let tmp = tempfile::tempdir().unwrap();
        let storage: std::sync::Arc<dyn cloud::StorageService> = std::sync::Arc::new(
            cloud::FsStorage::new(tmp.path().to_path_buf())
                .await
                .unwrap(),
        );
        let doc = crate::documents::ingest_bytes(
            surreal,
            &storage,
            &crate::documents::IngestArgs {
                project_id: proj,
                source: "upload",
                filename: "privileged.pdf",
                kind: "unclassified",
                content_type: "application/pdf",
                description: None,
                secondary_storage_key: None,
                visibility: crate::documents::visibility::INTERNAL,
            },
            format!("privileged {}", Uuid::now_v7()).as_bytes(),
        )
        .await
        .unwrap()
        .asset_id;
        (client, proj, doc)
    }

    /// A standalone audit row to link an authorization to.
    async fn audit_row(surreal: &SurrealDb, project_id: Uuid, admin: Uuid) -> Uuid {
        expunge_records::record(
            surreal,
            &NewExpunge {
                project_id,
                path: "privileged.pdf",
                category: CATEGORY_CLIENT_REQUEST,
                authorized_by_person_id: admin,
                head_before: None,
                head_after: None,
                note: None,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_defaults_to_pending_and_is_findable() {
        let surreal = mem().await;
        let (client, proj, doc) = seed(&surreal).await;

        let id = create(
            &surreal,
            &NewExpungeRequest {
                project_id: proj,
                asset_id: doc,
                requested_by_person_id: client,
                note: Some("please remove this"),
            },
        )
        .await
        .unwrap();

        let row = by_id(&surreal, id).await.unwrap().unwrap();
        assert_eq!(row.status, STATUS_PENDING);
        assert_eq!(row.note.as_deref(), Some("please remove this"));
        assert!(row.resolved_by_person_id.is_none());
        assert_eq!(row.asset_id, doc);
        assert_eq!(row.requested_by_person_id, client);

        assert_eq!(
            pending_for_document(&surreal, doc)
                .await
                .unwrap()
                .map(|r| r.id),
            Some(id)
        );
        assert_eq!(list_pending(&surreal).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn authorize_links_the_audit_row_and_clears_the_queue() {
        let surreal = mem().await;
        let (client, proj, doc) = seed(&surreal).await;
        let admin = persons::create(&surreal, &NewPerson::new("Nick", "nick@neonlaw.com"))
            .await
            .unwrap()
            .id;
        let id = create(
            &surreal,
            &NewExpungeRequest {
                project_id: proj,
                asset_id: doc,
                requested_by_person_id: client,
                note: None,
            },
        )
        .await
        .unwrap();
        let record_id = audit_row(&surreal, proj, admin).await;

        let updated = authorize(&surreal, id, admin, record_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, STATUS_AUTHORIZED);
        assert_eq!(updated.resolved_by_person_id, Some(admin));
        assert_eq!(updated.expunge_record_id, Some(record_id));
        // No longer pending → off the queue and not offered to the client.
        assert!(pending_for_document(&surreal, doc).await.unwrap().is_none());
        assert!(list_pending(&surreal).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deny_resolves_without_deleting() {
        let surreal = mem().await;
        let (client, proj, doc) = seed(&surreal).await;
        let lawyer = persons::create(&surreal, &NewPerson::new("Lawyer", "lawyer@neonlaw.com"))
            .await
            .unwrap()
            .id;
        let id = create(
            &surreal,
            &NewExpungeRequest {
                project_id: proj,
                asset_id: doc,
                requested_by_person_id: client,
                note: None,
            },
        )
        .await
        .unwrap();

        let updated = deny(&surreal, id, lawyer).await.unwrap().unwrap();
        assert_eq!(updated.status, STATUS_DENIED);
        assert_eq!(updated.resolved_by_person_id, Some(lawyer));
        assert!(updated.expunge_record_id.is_none());
        assert!(list_pending(&surreal).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn resolving_an_already_resolved_request_is_a_no_op() {
        // The lawyer queue is a shared surface, so two reviewers can reach the
        // same pending request. The second resolution must not overwrite the
        // first — the audit trail records who actually decided it.
        let surreal = mem().await;
        let (client, proj, doc) = seed(&surreal).await;
        let first = persons::create(&surreal, &NewPerson::new("Lawyer", "lawyer@neonlaw.com"))
            .await
            .unwrap()
            .id;
        let second = persons::create(&surreal, &NewPerson::new("Nick", "nick@neonlaw.com"))
            .await
            .unwrap()
            .id;
        let id = create(
            &surreal,
            &NewExpungeRequest {
                project_id: proj,
                asset_id: doc,
                requested_by_person_id: client,
                note: None,
            },
        )
        .await
        .unwrap();

        assert!(deny(&surreal, id, first).await.unwrap().is_some());
        let record_id = audit_row(&surreal, proj, second).await;
        assert!(
            authorize(&surreal, id, second, record_id)
                .await
                .unwrap()
                .is_none(),
            "a resolved request must not be re-resolved"
        );

        let row = by_id(&surreal, id).await.unwrap().unwrap();
        assert_eq!(row.status, STATUS_DENIED);
        assert_eq!(row.resolved_by_person_id, Some(first));
    }

    #[tokio::test]
    async fn resolving_an_unknown_request_is_none() {
        let surreal = mem().await;
        let lawyer = persons::create(&surreal, &NewPerson::new("Lawyer", "lawyer@neonlaw.com"))
            .await
            .unwrap()
            .id;
        assert!(deny(&surreal, Uuid::now_v7(), lawyer)
            .await
            .unwrap()
            .is_none());
    }
}
