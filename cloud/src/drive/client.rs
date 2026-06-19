//! REST client for Google Drive v3.
//!
//! [`DriveClient`] holds an [`Arc<dyn DriveAuth>`] (commit 1) and a
//! `reqwest::Client`. Every request goes through
//! [`DriveClient::do_get_with_retry`] which applies jittered
//! exponential backoff on `429 Too Many Requests` and `5xx`.
//!
//! Two public listing methods ship in this commit:
//!
//! - [`DriveClient::list_shared_drives`] — `GET /drive/v3/drives`.
//!   The first call after `cli drive login` to find the
//!   `NeonLaw` shared drive's id.
//! - [`DriveClient::list_folder_files`] — `GET /drive/v3/files` with
//!   `q="<folder_id>" in parents and trashed=false`. The per-Project
//!   sync's read path.
//!
//! Both methods follow `nextPageToken` until exhausted so callers
//! get a flat `Vec`. For the `NeonLaw` shared drive sizes today
//! (tens of matters, hundreds of files) this is fine; if it ever
//! grows past low-thousands we'd swap to a streaming `Stream` shape.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DriveAuth, DriveError};

/// Default base URL for the Drive REST API. Tests override this
/// with a `wiremock` URL via [`DriveClient::with_base_url`].
pub const GOOGLE_DRIVE_BASE_URL: &str = "https://www.googleapis.com";

/// Default backoff base in milliseconds. Real backoffs are
/// `base * 2^attempt + jitter(0..250)`. Tests override this to keep
/// the suite fast.
const DEFAULT_BACKOFF_BASE_MS: u64 = 500;

/// Default maximum retry attempts on `429` / `5xx` before giving up.
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Google-native MIME types we know how to export, mapped to the
/// export target MIME we ask Drive for. Locked here so the
/// representation we ingest is consistent across surfaces.
///
/// - Docs → `text/markdown` (closest to authoring intent, lints
///   cleanly with the workspace's S101 + N-family rules).
/// - Sheets → `text/csv` (one sheet per file; multi-sheet workbooks
///   only export the first tab — accept that until someone asks
///   for `application/x-vnd.oasis.opendocument.spreadsheet`).
/// - Slides → `application/pdf` (markdown export is a poor fit for
///   slide decks; PDF preserves layout for matter archives).
///
/// Returns `None` for binary files (use `alt=media`) and for
/// unsupported Google-native types (Forms, Sites — skip these
/// during sync; Drive's API doesn't expose their bytes).
#[must_use]
pub fn export_mime_for(google_mime: &str) -> Option<&'static str> {
    match google_mime {
        "application/vnd.google-apps.document" => Some("text/markdown"),
        "application/vnd.google-apps.spreadsheet" => Some("text/csv"),
        "application/vnd.google-apps.presentation" => Some("application/pdf"),
        _ => None,
    }
}

/// `true` when this MIME is a Google-native type that needs the
/// `/export` endpoint instead of `alt=media`. Catches the
/// not-supported-by-us cases (`form`, `site`, `drawing`) too so
/// the sync planner skips them with an explicit reason rather
/// than 4xx'ing mid-download.
#[must_use]
pub fn is_google_native(mime: &str) -> bool {
    mime.starts_with("application/vnd.google-apps.")
}

/// MIME type identifying a Drive *folder* (which is a "file" in
/// Drive's data model). Use this to skip directory entries when
/// you've asked for a folder's children but only want files.
pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

/// One shared drive — id + name. Returned by
/// [`DriveClient::list_shared_drives`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveSummary {
    pub id: String,
    pub name: String,
}

/// One file (or folder) under a shared drive. Returned by
/// [`DriveClient::list_folder_files`].
///
/// MIME type tells you the kind:
///
/// - `application/vnd.google-apps.folder` — a sub-folder.
/// - `application/vnd.google-apps.document` / `.spreadsheet` /
///   `.presentation` — a Google-native file (no `size`, no
///   `sha256_checksum`; export needed for bytes).
/// - Anything else — a binary file. `size` and `sha256_checksum`
///   are populated; `head_revision_id` identifies the live version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// Byte size; `None` for Google-native files (Docs / Sheets /
    /// Slides) which have no static byte representation.
    pub size: Option<i64>,
    /// `headRevisionId` from the Drive metadata. Stable id of the
    /// live version of a binary file. `None` for Google-native.
    pub head_revision_id: Option<String>,
    /// Drive-computed SHA-256, hex-encoded. `None` for Google-native;
    /// for binaries it lets us dedup before downloading.
    pub sha256_checksum: Option<String>,
    /// Parent folder ids. Drive supports multiple parents in
    /// principle; in our shared drive we expect exactly one.
    pub parents: Vec<String>,
}

/// Bytes returned by [`DriveClient::download_file`], together with
/// the resolved content type. For binaries this is the original
/// MIME from Drive; for Google-native files it's the export target
/// chosen by [`export_mime_for`].
#[derive(Debug, Clone)]
pub struct DownloadedBytes {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// HTTP client for the Drive REST API.
pub struct DriveClient {
    auth: Arc<dyn DriveAuth>,
    http: reqwest::Client,
    base_url: String,
    backoff_base_ms: u64,
    max_retries: u32,
}

impl std::fmt::Debug for DriveClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriveClient")
            .field("base_url", &self.base_url)
            .field("backoff_base_ms", &self.backoff_base_ms)
            .field("max_retries", &self.max_retries)
            .finish_non_exhaustive()
    }
}

impl DriveClient {
    /// Build a client that talks to the real Drive API.
    #[must_use]
    pub fn new(auth: Arc<dyn DriveAuth>) -> Self {
        Self {
            auth,
            http: reqwest::Client::new(),
            base_url: GOOGLE_DRIVE_BASE_URL.to_string(),
            backoff_base_ms: DEFAULT_BACKOFF_BASE_MS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// Override the base URL — tests point this at a `wiremock`
    /// server. The path `/drive/v3/...` is appended.
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Tighten the retry loop for tests so the suite doesn't sleep
    /// for seconds on every retry case. Test-only — production
    /// callers stay on the defaults.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_backoff_config(mut self, base_ms: u64, max_retries: u32) -> Self {
        self.backoff_base_ms = base_ms;
        self.max_retries = max_retries;
        self
    }

    /// `GET /drive/v3/drives`. Returns every shared drive the
    /// authenticated identity can see, paged through to completion.
    pub async fn list_shared_drives(&self) -> Result<Vec<DriveSummary>, DriveError> {
        let url = format!("{}/drive/v3/drives", self.base_url);
        let pages = self
            .paginated_get(
                &url,
                &[
                    ("pageSize", "100"),
                    ("fields", "drives(id,name),nextPageToken"),
                ],
            )
            .await?;
        let mut out = Vec::new();
        for page in pages {
            let arr = page.get("drives").and_then(Value::as_array);
            if let Some(arr) = arr {
                for v in arr {
                    let id = string_field(v, "id")?;
                    let name = string_field(v, "name")?;
                    out.push(DriveSummary { id, name });
                }
            }
        }
        Ok(out)
    }

    /// `GET /drive/v3/files` filtered to children of `folder_id`
    /// inside `drive_id`. Excludes trashed files.
    pub async fn list_folder_files(
        &self,
        drive_id: &str,
        folder_id: &str,
    ) -> Result<Vec<DriveFile>, DriveError> {
        let url = format!("{}/drive/v3/files", self.base_url);
        let q = format!("'{folder_id}' in parents and trashed = false");
        let fields =
            "nextPageToken,files(id,name,mimeType,size,headRevisionId,sha256Checksum,parents)";
        let pages = self
            .paginated_get(
                &url,
                &[
                    ("corpora", "drive"),
                    ("driveId", drive_id),
                    ("supportsAllDrives", "true"),
                    ("includeItemsFromAllDrives", "true"),
                    ("q", q.as_str()),
                    ("pageSize", "100"),
                    ("fields", fields),
                ],
            )
            .await?;
        let mut out = Vec::new();
        for page in pages {
            let arr = page.get("files").and_then(Value::as_array);
            if let Some(arr) = arr {
                for v in arr {
                    out.push(parse_drive_file(v)?);
                }
            }
        }
        Ok(out)
    }

    /// Download a file's bytes. The caller passes the `mime_type`
    /// they already learned from [`Self::list_folder_files`] so
    /// this method doesn't have to round-trip metadata.
    ///
    /// Routing:
    ///
    /// - [`FOLDER_MIME`] → returns
    ///   [`DriveError::InvalidConfig`] (folders aren't files).
    /// - Google-native with an entry in [`export_mime_for`] →
    ///   `GET /drive/v3/files/{id}/export?mimeType=<target>`.
    /// - Google-native without an entry (Forms, Sites, Drawings) →
    ///   [`DriveError::InvalidConfig`] — sync planner is expected to
    ///   skip these explicitly rather than reach this branch.
    /// - Anything else (binary) → `GET /drive/v3/files/{id}?alt=media`.
    pub async fn download_file(
        &self,
        file_id: &str,
        mime_type: &str,
    ) -> Result<DownloadedBytes, DriveError> {
        if mime_type == FOLDER_MIME {
            return Err(DriveError::InvalidConfig(format!(
                "cannot download folder {file_id} as a file"
            )));
        }
        let (url, query, resolved) = if is_google_native(mime_type) {
            let target = export_mime_for(mime_type).ok_or_else(|| {
                DriveError::InvalidConfig(format!(
                    "Drive type `{mime_type}` for file {file_id} has no export target — \
                     sync planner should have skipped it"
                ))
            })?;
            (
                format!("{}/drive/v3/files/{}/export", self.base_url, file_id),
                vec![("mimeType", target.to_string())],
                target.to_string(),
            )
        } else {
            (
                format!("{}/drive/v3/files/{}", self.base_url, file_id),
                vec![
                    ("alt", "media".to_string()),
                    ("supportsAllDrives", "true".to_string()),
                ],
                mime_type.to_string(),
            )
        };

        let resp = self.do_get_with_retry(&url, &query).await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(DriveError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?.to_vec();
        Ok(DownloadedBytes {
            bytes,
            content_type: resolved,
        })
    }

    /// Walk a `nextPageToken`-paginated endpoint and collect every
    /// response body. Each entry in the returned `Vec` is one raw
    /// JSON page; the caller extracts the array field it needs.
    async fn paginated_get(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<Vec<Value>, DriveError> {
        let mut pages = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut q: Vec<(&str, String)> =
                query.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
            if let Some(ref tok) = page_token {
                q.push(("pageToken", tok.clone()));
            }
            let resp = self.do_get_with_retry(url, &q).await?;
            let status = resp.status();
            let body = resp.text().await?;
            if !status.is_success() {
                return Err(DriveError::Api {
                    status: status.as_u16(),
                    body,
                });
            }
            let parsed: Value = serde_json::from_str(&body)?;
            let next = parsed
                .get("nextPageToken")
                .and_then(Value::as_str)
                .map(str::to_string);
            pages.push(parsed);
            match next {
                Some(t) if !t.is_empty() => {
                    page_token = Some(t);
                }
                _ => break,
            }
        }
        Ok(pages)
    }

    /// Issue a `GET` with bearer auth and a jittered
    /// exponential-backoff retry on 429/5xx. The token is re-fetched
    /// from `DriveAuth` on every attempt so a mid-retry expiry
    /// triggers a refresh.
    async fn do_get_with_retry(
        &self,
        url: &str,
        query: &[(&str, String)],
    ) -> Result<reqwest::Response, DriveError> {
        let mut attempt: u32 = 0;
        loop {
            let token = self.auth.access_token().await?;
            let resp = self
                .http
                .get(url)
                .bearer_auth(&token)
                .query(query)
                .send()
                .await?;
            let status = resp.status();
            if status.as_u16() == 429 || status.is_server_error() {
                if attempt >= self.max_retries {
                    let body = resp.text().await.unwrap_or_default();
                    return if status.as_u16() == 429 {
                        Err(DriveError::RateLimited)
                    } else {
                        Err(DriveError::Api {
                            status: status.as_u16(),
                            body,
                        })
                    };
                }
                let jitter: u64 = rand::random::<u64>() % 250;
                let backoff = self.backoff_base_ms.saturating_mul(1u64 << attempt) + jitter;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                attempt += 1;
                continue;
            }
            return Ok(resp);
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawDriveFile {
    id: String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    /// Drive returns `size` as a stringified i64. `serde` parses it
    /// transparently via the `with` adapter below.
    #[serde(default, deserialize_with = "deserialize_stringified_i64")]
    size: Option<i64>,
    #[serde(default, rename = "headRevisionId")]
    head_revision_id: Option<String>,
    #[serde(default, rename = "sha256Checksum")]
    sha256_checksum: Option<String>,
    #[serde(default)]
    parents: Vec<String>,
}

fn parse_drive_file(v: &Value) -> Result<DriveFile, DriveError> {
    let raw: RawDriveFile = serde_json::from_value(v.clone())?;
    Ok(DriveFile {
        id: raw.id,
        name: raw.name,
        mime_type: raw.mime_type,
        size: raw.size,
        head_revision_id: raw.head_revision_id,
        sha256_checksum: raw.sha256_checksum,
        parents: raw.parents,
    })
}

fn deserialize_stringified_i64<'de, D>(de: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(de)?;
    match opt {
        Some(s) => s.parse::<i64>().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

fn string_field(v: &Value, key: &str) -> Result<String, DriveError> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| DriveError::Api {
            status: 200,
            body: format!("drive response missing string field `{key}`: {v}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::auth::CliRefreshTokenAuth;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_auth() -> Arc<dyn DriveAuth> {
        // Pre-cache a bogus access token via a wiremock that responds
        // to the OAuth token endpoint. The simpler route: hand-roll a
        // stub auth.
        struct Static(String);
        #[async_trait::async_trait]
        impl DriveAuth for Static {
            async fn access_token(&self) -> Result<String, DriveError> {
                Ok(self.0.clone())
            }
        }
        Arc::new(Static("ya29.test-token".into()))
    }

    fn test_client(server: &MockServer) -> DriveClient {
        DriveClient::new(test_auth())
            .with_base_url(server.uri())
            .with_backoff_config(1, 3)
    }

    #[tokio::test]
    async fn list_shared_drives_single_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .and(header("authorization", "Bearer ya29.test-token"))
            .and(query_param("pageSize", "100"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "drives": [
                    { "id": "0AAA", "name": "NeonLaw" },
                    { "id": "0BBB", "name": "Personal" }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let drives = test_client(&server).list_shared_drives().await.unwrap();
        assert_eq!(
            drives,
            vec![
                DriveSummary {
                    id: "0AAA".into(),
                    name: "NeonLaw".into()
                },
                DriveSummary {
                    id: "0BBB".into(),
                    name: "Personal".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn list_shared_drives_paginates_through_multiple_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "drives": [{ "id": "0AAA", "name": "NeonLaw" }],
                "nextPageToken": "tok-2"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .and(query_param("pageToken", "tok-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "drives": [{ "id": "0BBB", "name": "Personal" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let drives = test_client(&server).list_shared_drives().await.unwrap();
        assert_eq!(drives.len(), 2);
        assert_eq!(drives[0].id, "0AAA");
        assert_eq!(drives[1].id, "0BBB");
    }

    #[tokio::test]
    async fn list_shared_drives_retries_on_429_then_succeeds() {
        let server = MockServer::start().await;
        // First call: 429. Second call: 200. wiremock supports a
        // priority+up_to_n_times pattern: register the 429 mock with
        // higher priority and an expectation of 1, then the 200 with
        // lower priority.
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "drives": [{ "id": "0AAA", "name": "NeonLaw" }]
            })))
            .with_priority(5)
            .mount(&server)
            .await;

        let drives = test_client(&server).list_shared_drives().await.unwrap();
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].id, "0AAA");
    }

    #[tokio::test]
    async fn list_shared_drives_returns_rate_limited_after_retries_exhausted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let err = test_client(&server).list_shared_drives().await.unwrap_err();
        assert!(matches!(err, DriveError::RateLimited), "got {err:?}");
    }

    #[tokio::test]
    async fn list_folder_files_passes_drive_and_q_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("corpora", "drive"))
            .and(query_param("driveId", "0AAA"))
            .and(query_param("supportsAllDrives", "true"))
            .and(query_param("includeItemsFromAllDrives", "true"))
            .and(query_param(
                "q",
                "'folder-123' in parents and trashed = false",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "files": [
                    {
                        "id": "f1",
                        "name": "intake.pdf",
                        "mimeType": "application/pdf",
                        "size": "1024",
                        "headRevisionId": "rev-1",
                        "sha256Checksum": "deadbeef",
                        "parents": ["folder-123"]
                    },
                    {
                        "id": "f2",
                        "name": "Notes",
                        "mimeType": "application/vnd.google-apps.document",
                        "parents": ["folder-123"]
                    }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let files = test_client(&server)
            .list_folder_files("0AAA", "folder-123")
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "intake.pdf");
        assert_eq!(files[0].size, Some(1024));
        assert_eq!(files[0].head_revision_id.as_deref(), Some("rev-1"));
        assert_eq!(files[0].sha256_checksum.as_deref(), Some("deadbeef"));
        assert_eq!(files[1].name, "Notes");
        assert_eq!(files[1].mime_type, "application/vnd.google-apps.document");
        assert!(files[1].size.is_none());
    }

    #[tokio::test]
    async fn list_folder_files_paginates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "files": [
                    { "id": "f1", "name": "a.pdf", "mimeType": "application/pdf", "parents": [] }
                ],
                "nextPageToken": "tok-2"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("pageToken", "tok-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "files": [
                    { "id": "f2", "name": "b.pdf", "mimeType": "application/pdf", "parents": [] }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let files = test_client(&server)
            .list_folder_files("0AAA", "folder-x")
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].id, "f1");
        assert_eq!(files[1].id, "f2");
    }

    #[tokio::test]
    async fn unsuccessful_status_surfaces_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/drives"))
            .respond_with(
                ResponseTemplate::new(403).set_body_string(
                    r#"{"error":{"code":403,"message":"insufficient permission"}}"#,
                ),
            )
            .mount(&server)
            .await;

        let err = test_client(&server).list_shared_drives().await.unwrap_err();
        match err {
            DriveError::Api { status, body } => {
                assert_eq!(status, 403);
                assert!(body.contains("insufficient permission"));
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn download_binary_uses_alt_media() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/file-abc"))
            .and(query_param("alt", "media"))
            .and(query_param("supportsAllDrives", "true"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(&b"%PDF-1.4 fake pdf bytes"[..])
                    .insert_header("content-type", "application/pdf"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let dl = test_client(&server)
            .download_file("file-abc", "application/pdf")
            .await
            .unwrap();
        assert_eq!(dl.content_type, "application/pdf");
        assert!(dl.bytes.starts_with(b"%PDF-1.4"));
    }

    #[tokio::test]
    async fn download_google_doc_exports_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/doc-abc/export"))
            .and(query_param("mimeType", "text/markdown"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(&b"# Heading\n\nBody"[..]))
            .expect(1)
            .mount(&server)
            .await;

        let dl = test_client(&server)
            .download_file("doc-abc", "application/vnd.google-apps.document")
            .await
            .unwrap();
        assert_eq!(dl.content_type, "text/markdown");
        assert_eq!(dl.bytes, b"# Heading\n\nBody");
    }

    #[tokio::test]
    async fn download_google_sheet_exports_csv() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/sh-1/export"))
            .and(query_param("mimeType", "text/csv"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(&b"a,b\n1,2\n"[..]))
            .expect(1)
            .mount(&server)
            .await;

        let dl = test_client(&server)
            .download_file("sh-1", "application/vnd.google-apps.spreadsheet")
            .await
            .unwrap();
        assert_eq!(dl.content_type, "text/csv");
    }

    #[tokio::test]
    async fn download_google_slides_exports_pdf() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/sl-1/export"))
            .and(query_param("mimeType", "application/pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(&b"%PDF-1.7 fake"[..]))
            .expect(1)
            .mount(&server)
            .await;

        let dl = test_client(&server)
            .download_file("sl-1", "application/vnd.google-apps.presentation")
            .await
            .unwrap();
        assert_eq!(dl.content_type, "application/pdf");
    }

    #[tokio::test]
    async fn download_folder_mime_is_rejected() {
        let server = MockServer::start().await;
        // Note: no mock — request should never go out.
        let err = test_client(&server)
            .download_file("fld-1", FOLDER_MIME)
            .await
            .unwrap_err();
        assert!(matches!(err, DriveError::InvalidConfig(_)), "{err:?}");
    }

    #[tokio::test]
    async fn download_unsupported_google_native_is_rejected() {
        let server = MockServer::start().await;
        let err = test_client(&server)
            .download_file("form-1", "application/vnd.google-apps.form")
            .await
            .unwrap_err();
        assert!(matches!(err, DriveError::InvalidConfig(_)), "{err:?}");
    }

    #[tokio::test]
    async fn download_surfaces_4xx_as_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string(r#"{"error":{"code":404}}"#))
            .mount(&server)
            .await;

        let err = test_client(&server)
            .download_file("missing", "application/pdf")
            .await
            .unwrap_err();
        match err {
            DriveError::Api { status, .. } => assert_eq!(status, 404),
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn export_mime_for_covers_three_native_types() {
        assert_eq!(
            export_mime_for("application/vnd.google-apps.document"),
            Some("text/markdown")
        );
        assert_eq!(
            export_mime_for("application/vnd.google-apps.spreadsheet"),
            Some("text/csv")
        );
        assert_eq!(
            export_mime_for("application/vnd.google-apps.presentation"),
            Some("application/pdf")
        );
        assert_eq!(export_mime_for("application/pdf"), None);
        assert_eq!(export_mime_for("application/vnd.google-apps.form"), None);
    }

    #[test]
    fn is_google_native_matches_google_apps_prefix() {
        assert!(is_google_native("application/vnd.google-apps.document"));
        assert!(is_google_native("application/vnd.google-apps.folder"));
        assert!(!is_google_native("application/pdf"));
        assert!(!is_google_native("text/markdown"));
    }

    // Sanity check that the CliRefreshTokenAuth path still wires
    // up correctly into a DriveClient — this catches a future
    // regression where the trait's lifetimes drift.
    #[tokio::test]
    async fn drive_client_accepts_cli_refresh_token_auth() {
        let auth = CliRefreshTokenAuth::with_token_uri(
            "cid".into(),
            "csec".into(),
            "rt".into(),
            "http://127.0.0.1:1/token".into(),
        );
        let _client = DriveClient::new(Arc::new(auth));
    }
}
