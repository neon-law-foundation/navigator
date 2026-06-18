//! Provision the GCS buckets `web` writes to:
//!
//! - `<project>-assets` — public marketing photography only (this is
//!   the one bucket with an `allUsers:objectViewer` binding, granted
//!   out-of-band). Standard.
//! - `<project>-documents` — **private** client documents; holds the
//!   content-addressed `blobs/<sha>` objects `web` writes. Standard,
//!   no public binding. Kept separate from `-assets` so confidential
//!   client data is never co-mingled into the public bucket.
//! - `<project>-logs` — long-lived audit / access logs. Nearline.
//!
//! All are private, single-region (location follows
//! `SetupConfig::region`, default `us-west4`), uniform bucket level
//! access. Storage class is STANDARD on assets and documents; NEARLINE
//! on logs (logs are read rarely, mostly written). The `-source`
//! (git bundles) and `-exports` (archives snapshots) buckets are
//! created out-of-band; see `cloud/README.md`.
//!
//! ## Idempotency
//!
//! Re-running `setup` against a project that already has these
//! buckets must succeed. We POST `storage.buckets.insert`
//! unconditionally and treat HTTP **409 Conflict** as success —
//! that's the response GCS returns when a bucket with the same name
//! already exists in the same project. Anything else outside the
//! 2xx range bubbles up as an error.

use serde::Serialize;

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketKind {
    Assets,
    Documents,
    Logs,
}

impl BucketKind {
    #[must_use]
    pub const fn storage_class(self) -> &'static str {
        match self {
            Self::Assets | Self::Documents => "STANDARD",
            Self::Logs => "NEARLINE",
        }
    }

    /// Classify a bucket by its name suffix. Returns `Assets` by
    /// default for unrecognized names — the caller is the source of
    /// truth on what it's creating. `-documents` and `-logs` are
    /// matched explicitly so neither is silently treated as `Assets`.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        if name.ends_with(super::LOGS_BUCKET_SUFFIX) {
            Self::Logs
        } else if name.ends_with(super::DOCUMENTS_BUCKET_SUFFIX) {
            Self::Documents
        } else {
            Self::Assets
        }
    }
}

#[derive(Serialize)]
struct CreateBucketBody<'a> {
    name: &'a str,
    location: &'a str,
    #[serde(rename = "storageClass")]
    storage_class: &'a str,
    #[serde(rename = "iamConfiguration")]
    iam_configuration: IamConfig,
}

#[derive(Serialize)]
struct IamConfig {
    #[serde(rename = "uniformBucketLevelAccess")]
    uniform_bucket_level_access: UniformAccess,
}

#[derive(Serialize)]
struct UniformAccess {
    enabled: bool,
}

/// Outcome of a single `ensure_bucket` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    /// Bucket did not exist; we created it.
    Created,
    /// Bucket already existed (HTTP 409 from `buckets.insert`).
    AlreadyExists,
}

/// Idempotently ensure a bucket exists in `project_id` at `location`.
pub async fn ensure_bucket(
    client: &GcpClient,
    project_id: &str,
    name: &str,
    location: &str,
) -> SetupResult<EnsureOutcome> {
    let kind = BucketKind::from_name(name);
    let body = CreateBucketBody {
        name,
        location,
        storage_class: kind.storage_class(),
        iam_configuration: IamConfig {
            uniform_bucket_level_access: UniformAccess { enabled: true },
        },
    };
    let body_json = serde_json::to_value(&body).map_err(|source| SetupError::Json {
        what: "create bucket request body",
        source,
    })?;
    let resp = client
        .post_json(
            GcpService::Storage,
            &format!("/storage/v1/b?project={project_id}"),
            &body_json,
        )
        .await?;
    let status = resp.status_u16();
    match status {
        200..=299 => Ok(EnsureOutcome::Created),
        409 => Ok(EnsureOutcome::AlreadyExists),
        other => Err(SetupError::BadStatus {
            operation: format!("create bucket {name}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::{ensure_bucket, EnsureOutcome};

    fn client_pointed_at(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Storage, server.uri())
    }

    #[tokio::test]
    async fn creates_bucket_when_post_returns_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .and(query_param("project", "proj"))
            .and(body_partial_json(json!({
                "name": "proj-assets",
                "location": "us-west4",
                "storageClass": "STANDARD",
                "iamConfiguration": {
                    "uniformBucketLevelAccess": { "enabled": true }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "proj-assets"})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let outcome = ensure_bucket(&client, "proj", "proj-assets", "us-west4")
            .await
            .unwrap();
        assert_eq!(outcome, EnsureOutcome::Created);
    }

    #[tokio::test]
    async fn treats_409_conflict_as_already_exists() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": { "code": 409, "message": "You already own this bucket." }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let outcome = ensure_bucket(&client, "proj", "proj-assets", "us-west4")
            .await
            .unwrap();
        assert_eq!(outcome, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn second_run_is_idempotent() {
        let server = MockServer::start().await;
        // First POST: bucket doesn't exist → 200.
        // Second POST: bucket exists → 409.
        // Wiremock matchers are FIFO when stacked under
        // `.up_to_n_times`, which is exactly the cadence we want
        // to model a real second run.
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "proj-assets"})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let first = ensure_bucket(&client, "proj", "proj-assets", "us-west4")
            .await
            .unwrap();
        let second = ensure_bucket(&client, "proj", "proj-assets", "us-west4")
            .await
            .unwrap();
        assert_eq!(first, EnsureOutcome::Created);
        assert_eq!(second, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn logs_bucket_uses_nearline_storage_class() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .and(body_partial_json(json!({
                "name": "proj-logs",
                "storageClass": "NEARLINE"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "proj-logs"})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        ensure_bucket(&client, "proj", "proj-logs", "us-west4")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn documents_bucket_uses_standard_storage_class() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .and(body_partial_json(json!({
                "name": "proj-documents",
                "storageClass": "STANDARD",
                "iamConfiguration": {
                    "uniformBucketLevelAccess": { "enabled": true }
                }
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"name": "proj-documents"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        ensure_bucket(&client, "proj", "proj-documents", "us-west4")
            .await
            .unwrap();
    }

    #[test]
    fn from_name_classifies_each_suffix() {
        use super::BucketKind;
        assert_eq!(BucketKind::from_name("proj-assets"), BucketKind::Assets);
        assert_eq!(
            BucketKind::from_name("proj-documents"),
            BucketKind::Documents
        );
        assert_eq!(BucketKind::from_name("proj-logs"), BucketKind::Logs);
        // Unknown names default to Assets (STANDARD).
        assert_eq!(BucketKind::from_name("proj-whatever"), BucketKind::Assets);
    }

    #[tokio::test]
    async fn unexpected_status_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/storage/v1/b"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let client = client_pointed_at(&server);
        let err = ensure_bucket(&client, "p", "x", "us-west4")
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("500"), "got {err}");
    }
}
