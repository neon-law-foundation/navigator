//! Governed expunge — the admin-only primitive that lawfully removes a
//! document from a matter (design §9).
//!
//! This is the one operation that rewrites a matter repo's history. It
//! exists for a **privilege clawback**, a **sealing order**, or a
//! client's **lawful deletion** request — never as routine editing. It
//! ties the three pieces together in order:
//!
//! 1. Verify the authorizer is an **admin** (the gate is baked into the
//!    primitive, not left to the caller).
//! 2. Rewrite the repo's history to remove the path
//!    ([`repos::RepoStore::expunge_path`]).
//! 3. Delete the file's bytes from object storage (`blobs/<sha>`,
//!    `lfs/<oid>`, or a fixed key like `notations/<id>/...`) so the
//!    content is gone from the data lake too.
//! 4. Record the expunge itself — who, when, category — but **not** the
//!    content, so the redaction stays auditable
//!    ([`store::expunge_records`]).
//!
//! Rewriting history invalidates existing clones; that is an accepted,
//! documented consequence of a lawful expunge.

use std::sync::Arc;

use cloud::StorageService;
use sea_orm::EntityTrait;
use store::entity::expunge_record;
use store::entity::person::{self, Role};
use store::Db;
use uuid::Uuid;

/// What can go wrong during an expunge.
#[derive(Debug, thiserror::Error)]
pub enum ExpungeError {
    /// The authorizing person is not an `admin`.
    #[error("not authorized: only an admin may expunge a matter document")]
    NotAdmin,
    /// Unknown expunge category.
    #[error("unknown expunge category `{0}` (expected privilege | sealing | client_request)")]
    BadCategory(String),
    /// The repo-history rewrite failed.
    #[error("repo: {0}")]
    Repo(#[from] repos::RepoError),
    /// Deleting the object bytes failed.
    #[error("storage: {0}")]
    Storage(String),
    /// A database operation failed.
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    /// The blocking git task panicked.
    #[error("expunge task: {0}")]
    Join(String),
}

/// One governed-expunge request.
pub struct ExpungeRequest<'a> {
    /// The matter whose repo holds the document.
    pub project_id: Uuid,
    /// The repo path to remove from all history (e.g. `notice.pdf`).
    pub path: &'a str,
    /// One of the [`expunge_record`] `CATEGORY_*` constants.
    pub category: &'a str,
    /// The admin authorizing the expunge.
    pub authorized_by: Uuid,
    /// `StorageService` key of the file's bytes to delete — `blobs/<sha>`
    /// for an ingested document, `lfs/<oid>` for an LFS object, or a
    /// fixed key like `notations/<id>/signed-document.pdf`. `None` if
    /// there is nothing to remove from object storage.
    pub storage_key: Option<&'a str>,
    /// Optional non-content note (e.g. a docket reference).
    pub note: Option<&'a str>,
}

/// Run a governed expunge. Returns the id of the audit row.
///
/// # Errors
/// [`ExpungeError::NotAdmin`] if the authorizer isn't an admin,
/// [`ExpungeError::BadCategory`] for an unknown category, or the
/// underlying repo / storage / database error.
pub async fn expunge(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    req: ExpungeRequest<'_>,
) -> Result<Uuid, ExpungeError> {
    // (1) Admin-only — the gate lives in the primitive itself.
    match person::Entity::find_by_id(req.authorized_by)
        .one(db)
        .await?
    {
        Some(p) if p.role == Role::Admin => {}
        _ => return Err(ExpungeError::NotAdmin),
    }
    if ![
        expunge_record::CATEGORY_PRIVILEGE,
        expunge_record::CATEGORY_SEALING,
        expunge_record::CATEGORY_CLIENT_REQUEST,
    ]
    .contains(&req.category)
    {
        return Err(ExpungeError::BadCategory(req.category.to_string()));
    }

    // (2) Rewrite history — shells git, so off the async pool.
    let repo_store = repos::RepoStore::from_env()?;
    let project_id = req.project_id;
    let path = req.path.to_string();
    let outcome = tokio::task::spawn_blocking(move || repo_store.expunge_path(project_id, &path))
        .await
        .map_err(|e| ExpungeError::Join(e.to_string()))??;

    // (3) Delete the bytes from object storage. A missing object is
    //     fine (already gone); anything else is a hard error.
    if let Some(key) = req.storage_key {
        match storage.delete(key).await {
            Ok(()) | Err(cloud::StorageError::NotFound(_)) => {}
            Err(e) => return Err(ExpungeError::Storage(e.to_string())),
        }
    }

    // (4) Record the expunge — who / when / category, not content.
    let id = store::expunge_records::record(
        db,
        &store::expunge_records::NewExpunge {
            project_id: req.project_id,
            path: req.path,
            category: req.category,
            authorized_by_person_id: req.authorized_by,
            head_before: outcome.head_before.as_deref(),
            head_after: outcome.head_after.as_deref(),
            note: req.note,
        },
    )
    .await?;

    tracing::warn!(
        project_id = %req.project_id,
        category = req.category,
        authorized_by = %req.authorized_by,
        "governed expunge completed"
    );
    Ok(id)
}
