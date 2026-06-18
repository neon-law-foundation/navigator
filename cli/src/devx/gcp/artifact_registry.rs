//! Provision the Artifact Registry repo Navigator pushes container
//! images to: a single DOCKER-format repo in `us-west4` (Las Vegas),
//! named `navigator`, with a native cleanup policy that deletes
//! images older than three days.
//!
//! The Artifact Registry URL we push to once this exists is
//! `us-west4-docker.pkg.dev/<project>/navigator/<image>:<tag>`.
//!
//! ## Why Artifact Registry, not Container Registry
//!
//! `gcr.io` (Container Registry) was sunset in May 2025; traffic
//! redirects to Artifact Registry. New code must target GAR.
//!
//! ## Cleanup policy
//!
//! GAR has native cleanup policies — no Cloud Function needed.
//! `condition.olderThan` takes a duration string in seconds; we set
//! `259200s` (3 days). The policy runs continuously on the GAR side.
//!
//! ## Idempotency
//!
//! `repositories.create` returns 409 Conflict when the repo already
//! exists; we treat that as success. This first cut does not
//! reconcile cleanup-policy drift on a repo created previously with
//! a different policy — re-running `ensure_repo` against an existing
//! repo is a no-op, not an update.

use serde_json::json;

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::{lro, DEFAULT_REGION};

/// The single docker repo we create. Hardcoded for now; if we need
/// flexibility, lift this onto a config struct.
pub const REPO_ID: &str = "navigator";

/// Three days in seconds. Matches the cleanup-policy
/// `condition.olderThan` field.
pub const CLEANUP_OLDER_THAN_SECONDS: u64 = 3 * 24 * 60 * 60;

/// Outcome of a single `ensure_repo` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    Created,
    AlreadyExists,
}

/// Idempotently ensure the Navigator GAR repo exists in `project_id`.
pub async fn ensure_repo(client: &GcpClient, project_id: &str) -> SetupResult<EnsureOutcome> {
    let body = json!({
        "format": "DOCKER",
        "description": "Navigator container images",
        "cleanupPolicies": {
            "delete-older-than-3d": {
                "action": "DELETE",
                "condition": {
                    "olderThan": format!("{CLEANUP_OLDER_THAN_SECONDS}s"),
                },
            },
        },
    });
    let path = format!(
        "/v1/projects/{project_id}/locations/{DEFAULT_REGION}/repositories?repositoryId={REPO_ID}",
    );
    let resp = client
        .post_json(GcpService::ArtifactRegistry, &path, &body)
        .await?;
    let status = resp.status_u16();
    match status {
        200..=299 => {
            let op: serde_json::Value =
                serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
                    what: "create repository response",
                    source,
                })?;
            lro::wait(client, GcpService::ArtifactRegistry, &op, "/v1/{name}").await?;
            Ok(EnsureOutcome::Created)
        }
        409 => Ok(EnsureOutcome::AlreadyExists),
        other => Err(SetupError::BadStatus {
            operation: format!("create artifact-registry repo {REPO_ID}"),
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
    use super::{ensure_repo, EnsureOutcome, CLEANUP_OLDER_THAN_SECONDS, REPO_ID};

    fn client_pointed_at(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ArtifactRegistry, server.uri())
    }

    #[tokio::test]
    async fn creates_repo_with_cleanup_policy_when_post_returns_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .and(query_param("repositoryId", REPO_ID))
            .and(body_partial_json(json!({
                "format": "DOCKER",
                "cleanupPolicies": {
                    "delete-older-than-3d": {
                        "action": "DELETE",
                        "condition": { "olderThan": format!("{CLEANUP_OLDER_THAN_SECONDS}s") }
                    }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/abc",
                "done": true
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let outcome = ensure_repo(&client, "proj").await.unwrap();
        assert_eq!(outcome, EnsureOutcome::Created);
    }

    #[tokio::test]
    async fn treats_409_conflict_as_already_exists() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": { "code": 409, "message": "Repository already exists" }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let outcome = ensure_repo(&client, "proj").await.unwrap();
        assert_eq!(outcome, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn second_run_is_idempotent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/abc",
                "done": true
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let first = ensure_repo(&client, "proj").await.unwrap();
        let second = ensure_repo(&client, "proj").await.unwrap();
        assert_eq!(first, EnsureOutcome::Created);
        assert_eq!(second, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn unexpected_status_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let client = client_pointed_at(&server);
        let err = ensure_repo(&client, "proj").await.unwrap_err();
        assert!(format!("{err}").contains("500"), "got {err}");
    }

    #[tokio::test]
    async fn dry_run_records_one_post_and_no_polling() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ArtifactRegistry, "http://127.0.0.1:1")
            .with_dry_run();
        ensure_repo(&client, "proj").await.unwrap();
        let calls = client.recorded_calls();
        assert_eq!(
            calls.len(),
            1,
            "dry-run should record one POST, got {calls:?}"
        );
        assert_eq!(calls[0].method, "POST");
        assert!(
            calls[0]
                .url
                .contains("/repositories?repositoryId=navigator"),
            "got {}",
            calls[0].url
        );
        let body = calls[0].body.as_deref().unwrap();
        assert!(body.contains("DOCKER"), "body should set format: {body}");
        assert!(
            body.contains("delete-older-than-3d"),
            "body should set cleanup policy: {body}"
        );
        assert!(
            body.contains("259200s"),
            "body should set 3-day olderThan: {body}"
        );
    }
}
