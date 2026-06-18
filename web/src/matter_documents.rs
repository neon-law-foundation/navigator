//! The one funnel for filing a document into a matter.
//!
//! Every document surface — inbound-email attachment, portal upload,
//! e-sign completion — routes through [`record_document`], which:
//!
//! 1. **Persists** the bytes durably via [`store::documents::ingest_bytes`]
//!    (content-addressed `blobs` row + `documents` row; the bytes land
//!    in `cloud::StorageService`, i.e. GCS in prod). This is the
//!    primary, must-succeed write.
//! 2. **Commits** the bytes into the Project's append-only repo authored
//!    as the acting person ([`repos::RepoStore::commit_as`]), so
//!    `git log` is the matter's audit trail (design §7), and stamps the
//!    resulting commit oid on the `documents` row.
//! 3. **Captures** a git-commit event into the BigQuery data lake — a
//!    Snappy Parquet object under `git-commits/data/dt=<date>/<oid>.parquet`,
//!    mirroring the [`crate::email_events`] pattern — so the matter
//!    record is reconstructable from the lake (GCS blobs + this event)
//!    even if the single-writer repo volume is ever lost.
//!
//! Steps 2–3 are **additive and non-fatal**: if the repo root is
//! unconfigured (`NAVIGATOR_GIT_REPO_ROOT` unset) or a git/storage call
//! fails, the document still persists and the surface still succeeds.
//! The repo layer is the new audit surface, not a new way for a live
//! flow (inbound email is in prod) to break.

use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use cloud::StorageService;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use repos::Author;
use store::documents::{self, IngestArgs, IngestedDocument};
use store::Db;
use uuid::Uuid;

/// File a document into a matter through the full pipeline (persist →
/// commit → lake capture). Returns the durable ingest result; the repo
/// commit is reflected on `documents.git_commit_oid` and is best-effort.
///
/// # Errors
/// Only a failure of the **durable** persistence
/// ([`store::documents::ingest_bytes`]) is returned as an error — the
/// repo commit and lake capture are non-fatal and logged.
pub async fn record_document(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    author: Author<'_>,
    args: &IngestArgs<'_>,
    bytes: &[u8],
) -> Result<IngestedDocument, documents::IngestError> {
    // (1) Durable primary write — bytes to GCS, blob + document rows.
    let ingested = documents::ingest_bytes(db, storage, args, bytes).await?;

    // (2)+(3) Additive audit layer. A failure here never fails the call.
    let link = DocLink {
        document_id: ingested.document_id,
        blob_sha256: &ingested.sha256_hex,
    };
    let message = format!("{}: {}", args.source, args.filename);
    match do_commit_and_capture(
        db,
        storage,
        args.project_id,
        author,
        args.source,
        args.kind,
        &message,
        &[(args.filename, bytes)],
        Some(link),
    )
    .await
    {
        Ok(Some(oid)) => {
            if let Err(e) = documents::set_git_commit_oid(db, ingested.document_id, &oid).await {
                tracing::warn!(error = %e, document_id = %ingested.document_id,
                    "filed document committed to repo but git_commit_oid stamp failed");
            }
        }
        Ok(None) => {} // repo layer not configured; document still filed
        Err(e) => {
            tracing::warn!(error = %e, document_id = %ingested.document_id, project_id = %args.project_id,
                "document filed but repo commit / lake capture failed (non-fatal)");
        }
    }

    Ok(ingested)
}

/// Commit one or more **already-stored** files into a Project's repo as
/// `author`, and capture the commit event to the data lake. Returns the
/// commit oid.
///
/// For surfaces whose bytes already live in object storage under their
/// own key — the e-sign signed PDF + Certificate of Completion at
/// `notations/<id>/...`, which the retrieval path reads by that exact
/// key — so this **does not** create a `documents`/`blobs` row or touch
/// that key. It only adds the repo-history + lake audit layer.
///
/// Non-fatal by contract: returns `None` (logged) when the repo layer is
/// unconfigured or any git/storage step fails, since the caller's
/// primary work already succeeded.
#[allow(clippy::too_many_arguments)]
pub async fn commit_files(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    project_id: Uuid,
    author: Author<'_>,
    source: &str,
    kind: &str,
    message: &str,
    files: &[(&str, &[u8])],
) -> Option<String> {
    match do_commit_and_capture(
        db, storage, project_id, author, source, kind, message, files, None,
    )
    .await
    {
        Ok(oid) => oid,
        Err(e) => {
            tracing::warn!(error = %e, %project_id, "repo commit / lake capture failed (non-fatal)");
            None
        }
    }
}

/// Optional link from a commit event back to the `documents` row it
/// filed. Absent for surfaces that store bytes outside the documents
/// table (e-sign).
struct DocLink<'a> {
    document_id: Uuid,
    blob_sha256: &'a str,
}

/// Commit `files` to the Project's repo as `author` and capture the
/// event to the lake. `Ok(None)` when the repo layer is not configured.
#[allow(clippy::too_many_arguments)]
async fn do_commit_and_capture(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    project_id: Uuid,
    author: Author<'_>,
    source: &str,
    kind: &str,
    message: &str,
    files: &[(&str, &[u8])],
    link: Option<DocLink<'_>>,
) -> anyhow::Result<Option<String>> {
    let store = match repos::RepoStore::from_env() {
        Ok(s) => s,
        Err(repos::RepoError::RootUnset) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    // commit_as shells git + touches the filesystem — run off the async
    // worker pool, so own the inputs.
    let author_name = author.name.to_string();
    let author_email = author.email.to_string();
    let message_owned = message.to_string();
    let owned_files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(p, b)| ((*p).to_string(), b.to_vec()))
        .collect();

    let oid = tokio::task::spawn_blocking(move || {
        let refs: Vec<(&str, &[u8])> = owned_files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        store.commit_as(
            project_id,
            Author {
                name: &author_name,
                email: &author_email,
            },
            &message_owned,
            &refs,
        )
    })
    .await??;

    // (3) Lake capture — mirror the matter record outside the PVC.
    let paths = files.iter().map(|(p, _)| *p).collect::<Vec<_>>().join(", ");
    let committed_at = chrono::Utc::now().to_rfc3339();
    let event = CommitEvent {
        project_id,
        commit_oid: &oid,
        author_name: author.name,
        author_email: author.email,
        message,
        paths: &paths,
        kind,
        source,
        document_id: link.as_ref().map(|l| l.document_id),
        blob_sha256: link.as_ref().map(|l| l.blob_sha256),
        committed_at: &committed_at,
    };
    capture_commit_event(db, storage, &event).await?;

    Ok(Some(oid))
}

/// One git-commit event destined for the data lake. All-string by
/// convention (BigQuery reads `STRING`, casts when needed), matching
/// the `archives` snapshot and `email_events` schemes. `document_id` /
/// `blob_sha256` are absent for commits not backed by a `documents` row.
struct CommitEvent<'a> {
    project_id: Uuid,
    commit_oid: &'a str,
    author_name: &'a str,
    author_email: &'a str,
    message: &'a str,
    /// Comma-joined paths the commit touched.
    paths: &'a str,
    kind: &'a str,
    source: &'a str,
    document_id: Option<Uuid>,
    blob_sha256: Option<&'a str>,
    committed_at: &'a str,
}

/// Encode a commit event as a one-row Snappy Parquet object and write it
/// to `git-commits/data/dt=<YYYY-MM-DD>/<oid>.parquet`. Idempotent by
/// commit oid (a replay overwrites the same key). `db` is unused today
/// but kept so a future version can enrich the row from related tables.
async fn capture_commit_event(
    _db: &Db,
    storage: &Arc<dyn StorageService>,
    event: &CommitEvent<'_>,
) -> anyhow::Result<()> {
    let parquet = encode_commit_parquet(event)?;
    let date = chrono::Utc::now().format("%Y-%m-%d");
    let key = format!("git-commits/data/dt={date}/{}.parquet", event.commit_oid);
    storage
        .put(&key, &parquet, "application/vnd.apache.parquet")
        .await?;
    Ok(())
}

fn encode_commit_parquet(event: &CommitEvent<'_>) -> anyhow::Result<Vec<u8>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("project_id", DataType::Utf8, false),
        Field::new("commit_oid", DataType::Utf8, false),
        Field::new("author_name", DataType::Utf8, true),
        Field::new("author_email", DataType::Utf8, true),
        Field::new("message", DataType::Utf8, true),
        Field::new("paths", DataType::Utf8, true),
        Field::new("kind", DataType::Utf8, true),
        Field::new("source", DataType::Utf8, true),
        Field::new("document_id", DataType::Utf8, true),
        Field::new("blob_sha256", DataType::Utf8, true),
        Field::new("committed_at", DataType::Utf8, true),
    ]));
    let col = |v: &str| -> ArrayRef { Arc::new(StringArray::from(vec![v.to_string()])) };
    let opt = |v: Option<String>| -> ArrayRef { Arc::new(StringArray::from(vec![v])) };
    let columns: Vec<ArrayRef> = vec![
        col(&event.project_id.to_string()),
        col(event.commit_oid),
        col(event.author_name),
        col(event.author_email),
        col(event.message),
        col(event.paths),
        col(event.kind),
        col(event.source),
        opt(event.document_id.map(|id| id.to_string())),
        opt(event.blob_sha256.map(ToString::to_string)),
        col(event.committed_at),
    ];
    let batch = RecordBatch::try_new(schema, columns)?;

    let mut buf = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(buf)
}
