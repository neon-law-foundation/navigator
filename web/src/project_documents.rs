//! Project-document HTTP surface:
//!
//! - `POST /portal/projects/:id/documents/upload` — multipart upload
//!   that pipes bytes through [`store::documents::ingest_bytes`].
//! - `GET /portal/projects/:id/documents/:doc_id` — per-document
//!   detail page showing full provenance.
//! - `GET /portal/projects/:id/documents/:doc_id/download` — issues
//!   a 302 to a short-lived signed URL on the storage backend, or
//!   streams bytes through the app on backends that can't sign
//!   (`FsStorage` in local dev).
//!
//! # Authorization model
//!
//! Three layers gate every request before bytes leave the building:
//!
//! 1. **Admin sub-router** — CSRF, session, and OPA policy are
//!    already enforced by middleware before any handler in this
//!    module runs. An unauthenticated request never reaches us.
//! 2. **Cross-project guard** — every handler resolves the document
//!    via [`load_doc_for_project`] and 404s if `document.project_id`
//!    doesn't match the `:id` segment of the URL. A user who guesses
//!    or steals a `doc_id` from another project can't tunnel it
//!    through their own project's URL.
//! 3. **Signed-URL handoff** — only after layers 1+2 pass do we ask
//!    the storage backend for a signed URL. The URL itself carries
//!    an HMAC of the canonical request signed by the GCS service
//!    account's private key; GCS rejects any request to the bucket
//!    that doesn't carry a valid signature. Bytes never proxy
//!    through this pod in production — the browser fetches direct
//!    from GCS, the app's role is to *decide* whether to issue the
//!    URL in the first place.
//!
//! Production uses GCS V4 signing; local dev uses `FsStorage` which
//! has no signing concept, so the handler falls back to streaming
//! bytes through the app. Same Rust code path, two backends.

use std::time::Duration;

use axum::body::Body;
use axum::extract::{Extension, Multipart, Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use sea_orm::EntityTrait;
use store::documents::{source, IngestArgs};
use store::entity::{blob, document, person, project};
use store::Db;
use uuid::Uuid;

use crate::admin::AdminState;
use crate::session::SessionData;
use views::pages::admin::projects as admin_views;

/// Signed-URL validity window for project documents.
///
/// A signed URL is the *only* credential the browser presents to
/// GCS — the URL contains an HMAC-SHA256 signature over the bucket,
/// object key, expiry, and HTTP method, signed by the service
/// account's RSA private key. GCS verifies the signature on every
/// request and rejects unsigned hits to the bucket. That means the
/// TTL is the URL's full security lifetime: anyone who obtains the
/// URL (legitimate user, screenshot, browser history, Slack paste,
/// dev-tools spectator on a shared screen) can fetch the bytes
/// until expiry, with no further auth check.
///
/// One hour is the product call. Trade-off:
///
/// - Shorter (e.g. 5 min, what retainer PDFs use in
///   [`crate::documents`]) tightens the leak window but breaks the
///   "lawyer opens the page, gets pulled into a call, comes back,
///   clicks Download" flow — they'd hit a 403 and have to refresh.
/// - Longer (24h, the user's stated upper bound) survives same-day
///   share-via-Slack but means a URL caught in someone else's
///   browser history is usable for the rest of the day.
/// - One hour fits a typical work session: long enough for normal
///   interruptions, short enough that a leak goes stale before the
///   next coffee break.
///
/// GCS V4 caps signed-URL TTL at 7 days; we're well inside the
/// bound. Bump cautiously: every hour added is another hour a leaked
/// URL stays live.
const SIGNED_URL_TTL: Duration = Duration::from_hours(1);

/// `POST /portal/projects/:id/documents/upload`.
pub async fn upload(
    State(state): State<AdminState>,
    AxumPath(project_id): AxumPath<Uuid>,
    session: Option<Extension<SessionData>>,
    mut multipart: Multipart,
) -> Response {
    // Bail early if the project doesn't exist — the FK would catch
    // this later but the 404 is friendlier than a 500.
    let Ok(Some(_)) = project::Entity::find_by_id(project_id).one(&state.db).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut file_name: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;
    let mut kind: Option<String> = None;
    let mut description: Option<String> = None;

    loop {
        let next = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        };
        let name = next.name().map(String::from);
        match name.as_deref() {
            Some("file") => {
                file_name = next.file_name().map(String::from);
                content_type = next.content_type().map(String::from);
                let raw = match next.bytes().await {
                    Ok(b) => b.to_vec(),
                    Err(_) => return StatusCode::BAD_REQUEST.into_response(),
                };
                bytes = Some(raw);
            }
            Some("kind") => {
                kind = next.text().await.ok();
            }
            Some("description") => {
                description = next.text().await.ok();
            }
            _ => {}
        }
    }

    let Some(bytes) = bytes else {
        return Redirect::to(&format!("/portal/projects/{project_id}")).into_response();
    };
    if bytes.is_empty() {
        return Redirect::to(&format!("/portal/projects/{project_id}")).into_response();
    }

    let file_name = file_name.unwrap_or_else(|| format!("upload-{project_id}"));
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".into());
    let kind = kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| "unclassified".to_string(), str::to_string);
    let description_trimmed = description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let args = IngestArgs {
        project_id,
        source: source::UPLOAD,
        filename: &file_name,
        kind: &kind,
        content_type: &content_type,
        description: description_trimmed,
        source_revision_id: None,
    };

    // File the upload as the staff/admin who uploaded it, so the matter
    // repo's `git log` attributes it to them.
    let (author_name, author_email) =
        uploader_identity(&state.db, session.map(|Extension(s)| s)).await;
    let author = repos::Author {
        name: &author_name,
        email: &author_email,
    };

    if let Err(e) =
        crate::matter_documents::record_document(&state.db, &state.storage, author, &args, &bytes)
            .await
    {
        tracing::error!(
            project_id = %project_id,
            filename = %file_name,
            error = %e,
            "project document upload failed"
        );
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Redirect::to(&format!("/portal/projects/{project_id}")).into_response()
}

/// Resolve the uploader's `(name, email)` for git authorship from their
/// session. Prefers the linked `persons` row (faithful name + email);
/// falls back to the session email, then to a neutral placeholder so a
/// commit is never blocked on a missing identity.
async fn uploader_identity(db: &Db, session: Option<SessionData>) -> (String, String) {
    if let Some(session) = session {
        if let Some(pid) = session.person_id {
            if let Ok(Some(p)) = person::Entity::find_by_id(pid).one(db).await {
                return (p.name, p.email);
            }
        }
        if let Some(email) = session.email {
            return (email.clone(), email);
        }
    }
    ("Navigator staff".to_string(), "staff@localhost".to_string())
}

/// `GET /portal/projects/:id/documents/:doc_id/download`. Resolves
/// the document, blocks cross-project leakage, then either 302s to
/// a signed URL or streams bytes through the app.
pub async fn download(
    State(state): State<AdminState>,
    AxumPath((project_id, doc_id)): AxumPath<(Uuid, Uuid)>,
) -> Response {
    let Some((doc, blob_row)) = load_doc_for_project(&state, project_id, doc_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match state
        .storage
        .signed_url(&blob_row.storage_key, SIGNED_URL_TTL)
        .await
    {
        Ok(url) => Redirect::temporary(&url).into_response(),
        Err(cloud::StorageError::Unsupported(_)) => {
            stream_through(
                state,
                &blob_row.storage_key,
                &blob_row.content_type,
                &doc.filename,
            )
            .await
        }
        Err(cloud::StorageError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(
                error = %e,
                %project_id,
                %doc_id,
                storage_key = %blob_row.storage_key,
                "signed_url failed for project document"
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /portal/projects/:id/documents/:doc_id`. Per-document detail
/// page rendering the full provenance off the `documents` +
/// `blobs` rows.
pub async fn detail(
    State(state): State<AdminState>,
    AxumPath((project_id, doc_id)): AxumPath<(Uuid, Uuid)>,
) -> Markup {
    let Some((doc, blob_row)) = load_doc_for_project(&state, project_id, doc_id).await else {
        return views::not_found_page();
    };

    let sha_short = blob_row
        .sha256_hex
        .get(..12)
        .unwrap_or(&blob_row.sha256_hex);
    let download_href = format!("/portal/projects/{project_id}/documents/{doc_id}/download");
    let back_href = format!("/portal/projects/{project_id}");

    admin_views::document_detail(&admin_views::DocumentDetail {
        project_id,
        doc_id,
        filename: &doc.filename,
        kind: &doc.kind,
        source: &doc.source,
        source_revision_id: doc.source_revision_id.as_deref(),
        received_at: &doc.received_at,
        description: doc.description.as_deref(),
        content_type: &blob_row.content_type,
        byte_size: blob_row.byte_size,
        sha256_hex: &blob_row.sha256_hex,
        sha256_short: sha_short,
        download_href: &download_href,
        back_href: &back_href,
    })
}

/// Look up the document by id and reject if it doesn't belong to the
/// project_id from the URL — this is the cross-project leakage
/// guard. Returns `(document, blob)` because every caller wants both.
async fn load_doc_for_project(
    state: &AdminState,
    project_id: Uuid,
    doc_id: Uuid,
) -> Option<(document::Model, blob::Model)> {
    let doc = document::Entity::find_by_id(doc_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    if doc.project_id != project_id {
        return None;
    }
    let blob_row = blob::Entity::find_by_id(doc.blob_id)
        .one(&state.db)
        .await
        .ok()
        .flatten()?;
    Some((doc, blob_row))
}

/// Stream bytes through the app — fallback when the storage backend
/// has no signed-URL concept (`FsStorage` in local dev). Sets a
/// `Content-Disposition: attachment` so the browser downloads with
/// the original filename rather than the content-addressed
/// `blobs/<sha>` key.
async fn stream_through(
    state: AdminState,
    key: &str,
    content_type: &str,
    filename: &str,
) -> Response {
    match state.storage.get(key).await {
        Ok(obj) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            )
            .body(Body::from(obj.bytes))
            .map_or_else(
                |e| {
                    tracing::error!(error = %e, "build streaming response");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                },
                IntoResponse::into_response,
            ),
        Err(cloud::StorageError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, key, "storage get failed for project document");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
