//! `store::documents` — write-side primitive for the canonical
//! `documents` + `blobs` lane.
//!
//! [`ingest_bytes`] is the one entry point. Given a project,
//! bytes, and provenance, it:
//!
//! 1. Computes a SHA-256 of the bytes.
//! 2. Looks up an existing `blobs` row with that SHA. If present,
//!    the new `documents` row references it — the same byte stored
//!    twice doesn't pay storage twice.
//! 3. Otherwise: writes the bytes through [`cloud::StorageService`]
//!    using `blobs/<sha>` as the storage key (content-addressed,
//!    immutable), then inserts the `blobs` row.
//! 4. Inserts a `documents` row pointing at the (new or reused)
//!    blob, carrying its inbound-channel provenance (`source`,
//!    `source_revision_id`, `received_at`) and optional
//!    staff-view `description`.
//!
//! Steps 2–4 run inside a single transaction so a partial ingest
//! never leaves the DB pointing at storage keys that don't exist.

use std::sync::Arc;

use chrono::Utc;
use cloud::{StorageError, StorageService};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    TransactionTrait,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entity::{blob, document};
use crate::Db;

/// Inbound-channel literals written to `documents.source`. Centralized
/// here so handlers, the Drive orchestrator, and tests agree on the
/// same strings; mismatches turn into silent dedup-planner bugs.
pub mod source {
    pub const UPLOAD: &str = "upload";
    pub const DRIVE_SYNC: &str = "drive_sync";
    /// An attachment received on inbound `support@` mail (see
    /// `web::email_threads`).
    pub const EMAIL: &str = "email";
}

/// Errors surfaced by [`ingest_bytes`].
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// Inputs to [`ingest_bytes`]. Held in a struct (rather than a long
/// positional list) so future fields don't break callers.
#[derive(Debug, Clone)]
pub struct IngestArgs<'a> {
    /// Project the document belongs to.
    pub project_id: Uuid,
    /// Inbound channel name — `upload`, `drive_sync`, `email`,
    /// `fax`, `scan`. Goes into `documents.source`.
    pub source: &'a str,
    /// Caller-visible filename. Goes into `documents.filename`.
    pub filename: &'a str,
    /// Document classification — `retainer`, `intake`, `invoice`,
    /// or `unclassified` for sync-time pulls. Goes into
    /// `documents.kind`.
    pub kind: &'a str,
    /// MIME content type of `bytes`.
    pub content_type: &'a str,
    /// Optional staff-view caption; goes into `documents.description`.
    pub description: Option<&'a str>,
    /// Source-system revision identifier. For a Drive sync this is
    /// the file's `headRevisionId`; for email it could be the
    /// `Message-ID`. Stored in `documents.source_revision_id`.
    pub source_revision_id: Option<&'a str>,
}

/// What [`ingest_bytes`] writes, returned for the caller to log /
/// reference / show in a UI.
#[derive(Debug, Clone)]
pub struct IngestedDocument {
    pub document_id: Uuid,
    pub blob_id: Uuid,
    pub sha256_hex: String,
    pub byte_size: i64,
    /// `true` when the bytes were already stored under another
    /// document — `blob_id` points at the pre-existing row, no new
    /// storage write happened.
    pub blob_reused: bool,
}

/// Ingest one artifact: write the bytes (if new), insert
/// `blob` + `document` rows, return the ids.
pub async fn ingest_bytes(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    args: &IngestArgs<'_>,
    bytes: &[u8],
) -> Result<IngestedDocument, IngestError> {
    let sha_hex = sha256_hex(bytes);
    let byte_size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);

    let txn = db.begin().await?;

    let existing = blob::Entity::find()
        .filter(blob::Column::Sha256Hex.eq(sha_hex.clone()))
        .one(&txn)
        .await?;

    let (blob_id, blob_reused) = if let Some(b) = existing {
        (b.id, true)
    } else {
        let storage_key = format!("blobs/{sha_hex}");
        // Write bytes before inserting the row so a crash mid-ingest
        // never leaves a `blobs` row pointing at a key that doesn't
        // exist. Storage `put` is expected to be idempotent w.r.t.
        // re-writes of the same bytes (both FsStorage and GcsStorage
        // overwrite-on-put).
        storage.put(&storage_key, bytes, args.content_type).await?;
        let new_id =
            insert_blob_row(&txn, &storage_key, args.content_type, byte_size, &sha_hex).await?;
        (new_id, false)
    };

    let document_id = insert_document_row(&txn, args, blob_id).await?;

    txn.commit().await?;

    Ok(IngestedDocument {
        document_id,
        blob_id,
        sha256_hex: sha_hex,
        byte_size,
        blob_reused,
    })
}

/// Stamp the git commit that filed `document_id` into the Project's
/// repo. Idempotent and tolerant: a missing document is a no-op (the
/// commit linkage is a best-effort audit detail, never a hard
/// dependency of the durable blob+row write). See
/// [the design](../../docs/git-project-repos.md) §7.
///
/// # Errors
/// [`sea_orm::DbErr`] if the update fails.
pub async fn set_git_commit_oid(
    db: &Db,
    document_id: Uuid,
    oid: &str,
) -> Result<(), sea_orm::DbErr> {
    let Some(doc) = document::Entity::find_by_id(document_id).one(db).await? else {
        return Ok(());
    };
    let mut active: document::ActiveModel = doc.into();
    active.git_commit_oid = ActiveValue::Set(Some(oid.to_string()));
    active.update(db).await?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

async fn insert_blob_row<C: ConnectionTrait>(
    txn: &C,
    storage_key: &str,
    content_type: &str,
    byte_size: i64,
    sha_hex: &str,
) -> Result<Uuid, sea_orm::DbErr> {
    let id = Uuid::now_v7();
    let row = blob::ActiveModel {
        id: ActiveValue::Set(id),
        storage_key: ActiveValue::Set(storage_key.to_string()),
        content_type: ActiveValue::Set(content_type.to_string()),
        byte_size: ActiveValue::Set(byte_size),
        sha256_hex: ActiveValue::Set(sha_hex.to_string()),
        ..Default::default()
    };
    row.insert(txn).await?;
    Ok(id)
}

async fn insert_document_row<C: ConnectionTrait>(
    txn: &C,
    args: &IngestArgs<'_>,
    blob_id: Uuid,
) -> Result<Uuid, sea_orm::DbErr> {
    let id = Uuid::now_v7();
    let row = document::ActiveModel {
        id: ActiveValue::Set(id),
        project_id: ActiveValue::Set(args.project_id),
        blob_id: ActiveValue::Set(blob_id),
        filename: ActiveValue::Set(args.filename.to_string()),
        kind: ActiveValue::Set(args.kind.to_string()),
        source: ActiveValue::Set(args.source.to_string()),
        source_revision_id: ActiveValue::Set(args.source_revision_id.map(String::from)),
        received_at: ActiveValue::Set(Utc::now().to_rfc3339()),
        description: ActiveValue::Set(args.description.map(String::from)),
        ..Default::default()
    };
    row.insert(txn).await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cloud::FsStorage;
    use sea_orm::EntityTrait;

    async fn fixtures() -> (Db, Arc<dyn StorageService>, tempfile::TempDir, Uuid) {
        let db = crate::test_support::pg().await;
        let tmp = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageService> =
            Arc::new(FsStorage::new(tmp.path().to_path_buf()).await.unwrap());

        // Need a project to attach to. Insert a minimal one.
        let project_id = Uuid::now_v7();
        crate::entity::project::ActiveModel {
            id: ActiveValue::Set(project_id),
            name: ActiveValue::Set("Test Matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(&db).await),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        (db, storage, tmp, project_id)
    }

    #[tokio::test]
    async fn ingest_writes_blob_and_document_with_provenance() {
        let (db, storage, _tmp, project_id) = fixtures().await;
        let args = IngestArgs {
            project_id,
            source: "upload",
            filename: "retainer.pdf",
            kind: "retainer",
            content_type: "application/pdf",
            description: Some("client-signed retainer"),
            source_revision_id: None,
        };

        let out = ingest_bytes(&db, &storage, &args, b"hello world")
            .await
            .unwrap();

        assert!(!out.blob_reused);
        assert_eq!(out.byte_size, 11);
        // sha256("hello world") =
        //   b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        assert_eq!(
            out.sha256_hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        let b = blob::Entity::find_by_id(out.blob_id)
            .one(&db)
            .await
            .unwrap()
            .expect("blob row");
        assert_eq!(b.content_type, "application/pdf");
        assert_eq!(b.byte_size, 11);
        assert_eq!(b.storage_key, format!("blobs/{}", out.sha256_hex));

        let d = document::Entity::find_by_id(out.document_id)
            .one(&db)
            .await
            .unwrap()
            .expect("document row");
        assert_eq!(d.filename, "retainer.pdf");
        assert_eq!(d.kind, "retainer");
        assert_eq!(d.project_id, project_id);
        assert_eq!(d.blob_id, out.blob_id);
        assert_eq!(d.source, "upload");
        assert_eq!(d.description.as_deref(), Some("client-signed retainer"));
        assert!(d.source_revision_id.is_none());
        assert!(
            !d.received_at.is_empty(),
            "received_at must be stamped on insert"
        );

        let stored = storage.get(&b.storage_key).await.unwrap();
        assert_eq!(stored.bytes, b"hello world");
    }

    #[tokio::test]
    async fn ingest_dedupes_by_sha_when_bytes_match() {
        let (db, storage, _tmp, project_id) = fixtures().await;
        let bytes = b"same bytes";
        let mk = |fname: &'static str| IngestArgs {
            project_id,
            source: "upload",
            filename: fname,
            kind: "intake",
            content_type: "text/plain",
            description: None,
            source_revision_id: None,
        };

        let first = ingest_bytes(&db, &storage, &mk("a.txt"), bytes)
            .await
            .unwrap();
        assert!(!first.blob_reused);

        let second = ingest_bytes(&db, &storage, &mk("b.txt"), bytes)
            .await
            .unwrap();
        assert!(second.blob_reused);
        assert_eq!(second.blob_id, first.blob_id, "blob row must be reused");
        assert_ne!(
            second.document_id, first.document_id,
            "documents are distinct even when they share a blob"
        );

        let docs = document::Entity::find().all(&db).await.unwrap();
        assert_eq!(docs.len(), 2);
        let blobs = blob::Entity::find().all(&db).await.unwrap();
        assert_eq!(blobs.len(), 1);
    }

    #[tokio::test]
    async fn ingest_records_source_revision_id_on_document() {
        let (db, storage, _tmp, project_id) = fixtures().await;
        let args = IngestArgs {
            project_id,
            source: "drive_sync",
            filename: "intake.pdf",
            kind: "intake",
            content_type: "application/pdf",
            description: Some("from Drive folder"),
            source_revision_id: Some("rev-abc-123"),
        };

        let out = ingest_bytes(&db, &storage, &args, b"any bytes")
            .await
            .unwrap();
        let row = document::Entity::find_by_id(out.document_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.source, "drive_sync");
        assert_eq!(row.source_revision_id.as_deref(), Some("rev-abc-123"));
        assert_eq!(row.description.as_deref(), Some("from Drive folder"));
    }

    #[tokio::test]
    async fn ingest_different_bytes_produces_different_blob() {
        let (db, storage, _tmp, project_id) = fixtures().await;
        let mk = |fname: &'static str| IngestArgs {
            project_id,
            source: "upload",
            filename: fname,
            kind: "intake",
            content_type: "text/plain",
            description: None,
            source_revision_id: None,
        };
        let a = ingest_bytes(&db, &storage, &mk("a.txt"), b"alpha")
            .await
            .unwrap();
        let b = ingest_bytes(&db, &storage, &mk("b.txt"), b"bravo")
            .await
            .unwrap();
        assert_ne!(a.blob_id, b.blob_id);
        assert_ne!(a.sha256_hex, b.sha256_hex);
    }
}
