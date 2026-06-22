//! Git LFS — large file storage for a Project's repo, backed by
//! [`cloud::StorageService`] (GCS in prod, the Fs backend in KIND).
//!
//! See [the design](../../docs/git-project-repos.md) §5. PDFs, docx, and
//! images are committed as small LFS *pointers* (which ride the pack
//! history) while their bytes live in object storage keyed by the
//! pointer's `oid` (a sha256). This is where GCS stays in the picture —
//! the repos themselves live on a POSIX volume, never in a bucket.
//!
//! Three endpoints implement the LFS "basic" transfer:
//!
//! - `POST .../info/lfs/objects/batch` — the client announces the
//!   objects it wants to upload or download; we return an `actions`
//!   href per object pointing back at the transfer endpoints below.
//! - `PUT  .../info/lfs/objects/{oid}` — upload an object's bytes.
//! - `GET  .../info/lfs/objects/{oid}` — download an object's bytes.
//!
//! All three reuse the transport's PAT + project ACL
//! ([`crate::git_http::authorize_project`]): download needs read, upload
//! needs a write-scoped token. Objects are stored under the
//! `lfs/<oid>` key so one bucket holds every Project's LFS objects,
//! addressed by content hash.

use std::collections::HashMap;

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::git_http::authorize_project;
use crate::AppState;

/// LFS JSON content type for batch requests and responses.
const LFS_CONTENT_TYPE: &str = "application/vnd.git-lfs+json";

/// Mount the LFS routes. `:repo` is `<project-id>.git`, matching the
/// transport.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/{repo}/info/lfs/objects/batch", post(batch))
        .route(
            "/projects/{repo}/info/lfs/objects/{oid}",
            get(download).put(upload),
        )
}

/// Storage key for an LFS object: content-addressed by its sha256 oid,
/// shared across every Project's repo.
fn object_key(oid: &str) -> String {
    format!("lfs/{oid}")
}

/// A 64-char lowercase-hex sha256 — the only oid shape LFS uses.
fn is_valid_oid(oid: &str) -> bool {
    oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Lowercase-hex sha256 of `bytes` — an LFS object's oid is the sha256
/// of its content, so this is how we verify an upload's integrity.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

// ---- batch API ------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BatchRequest {
    /// `upload` or `download`.
    operation: String,
    #[serde(default)]
    objects: Vec<ObjectRef>,
}

#[derive(Debug, Deserialize)]
struct ObjectRef {
    oid: String,
    size: i64,
}

#[derive(Debug, Serialize)]
struct BatchResponse {
    transfer: &'static str,
    objects: Vec<ObjectSpec>,
}

#[derive(Debug, Serialize)]
struct ObjectSpec {
    oid: String,
    size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    actions: Option<Actions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ObjectError>,
}

#[derive(Debug, Serialize)]
struct Actions {
    #[serde(skip_serializing_if = "Option::is_none")]
    upload: Option<Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<Action>,
}

#[derive(Debug, Serialize)]
struct Action {
    href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct ObjectError {
    code: u16,
    message: String,
}

async fn batch(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    headers: HeaderMap,
    Json(req): Json<BatchRequest>,
) -> Response {
    let write = match req.operation.as_str() {
        "upload" => true,
        "download" => false,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                format!("unknown operation {other}"),
            )
                .into_response()
        }
    };

    let project_id = match authorize_project(&state, &repo, write, &headers).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let _ = project_id; // ACL only; object keys are content-addressed.

    // Reuse the caller's Authorization header on the transfer requests,
    // so the LFS client presents the same PAT to PUT/GET.
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            let mut h = HashMap::new();
            h.insert("Authorization".to_string(), v.to_string());
            h
        });

    let objects = req
        .objects
        .into_iter()
        .map(|o| {
            let href = object_href(&headers, &repo, &o.oid);
            let action = Action {
                href,
                header: auth_header.clone(),
            };
            let actions = if write {
                Actions {
                    upload: Some(action),
                    download: None,
                }
            } else {
                Actions {
                    upload: None,
                    download: Some(action),
                }
            };
            ObjectSpec {
                oid: o.oid,
                size: o.size,
                actions: Some(actions),
                error: None,
            }
        })
        .collect();

    let body = BatchResponse {
        transfer: "basic",
        objects,
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, LFS_CONTENT_TYPE)],
        Json(body),
    )
        .into_response()
}

/// Absolute URL of an LFS object's transfer endpoint. Scheme follows the
/// load balancer's `X-Forwarded-Proto` (https in prod), defaulting to
/// https; host is the request `Host`.
fn object_href(headers: &HeaderMap, repo: &str, oid: &str) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("https");
    format!("{scheme}://{host}/projects/{repo}/info/lfs/objects/{oid}")
}

// ---- transfer endpoints --------------------------------------------

async fn upload(
    State(state): State<AppState>,
    AxumPath((repo, oid)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(resp) = authorize_project(&state, &repo, true, &headers).await {
        return resp;
    }
    if !is_valid_oid(&oid) {
        return (StatusCode::BAD_REQUEST, "invalid oid").into_response();
    }
    // Integrity: the stored bytes must hash to the claimed oid.
    let actual = sha256_hex(&body);
    if actual != oid {
        return (StatusCode::BAD_REQUEST, "oid does not match content").into_response();
    }
    match state
        .storage
        .put(&object_key(&oid), &body, "application/octet-stream")
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "lfs: storage put failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response()
        }
    }
}

async fn download(
    State(state): State<AppState>,
    AxumPath((repo, oid)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = authorize_project(&state, &repo, false, &headers).await {
        return resp;
    }
    if !is_valid_oid(&oid) {
        return (StatusCode::BAD_REQUEST, "invalid oid").into_response();
    }
    match state.storage.get(&object_key(&oid)).await {
        Ok(obj) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            obj.bytes,
        )
            .into_response(),
        Err(cloud::StorageError::NotFound(_)) => {
            (StatusCode::NOT_FOUND, "object not found").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "lfs: storage get failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oid_validation() {
        assert!(is_valid_oid(&"a".repeat(64)));
        assert!(!is_valid_oid(&"a".repeat(63)));
        assert!(!is_valid_oid(&"g".repeat(64)));
    }

    #[test]
    fn object_key_is_content_addressed() {
        assert_eq!(object_key("deadbeef"), "lfs/deadbeef");
    }
}
