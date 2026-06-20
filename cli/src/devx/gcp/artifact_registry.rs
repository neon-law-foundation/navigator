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
//! GAR has native cleanup policies — no Cloud Function needed. We
//! attach two, evaluated together (KEEP wins over DELETE for any
//! version it matches):
//!
//! - `delete-older-than-3d` — DELETE anything older than `259200s`
//!   (3 days). `condition.olderThan` is a duration string in seconds.
//! - `keep-most-recent` — KEEP the most recent
//!   [`KEEP_MOST_RECENT_VERSIONS`] versions of *every* image,
//!   regardless of age. Without it an infrequently-rebuilt image — a
//!   `CronJob` *trigger* whose tag a long-lived `CronJob` pins — ages past
//!   3 days, gets deleted out from under the cluster, and the `CronJob`
//!   then `ImagePullBackOff`s forever. The keep rule pins each image's
//!   latest builds so that can't happen.
//!
//! ## Idempotency / reconcile
//!
//! `repositories.create` returns 409 Conflict when the repo already
//! exists. On 409 we PATCH the existing repo's `cleanupPolicies` so a
//! policy change in this file converges the live repo — earlier this
//! was a silent no-op, which is exactly how a too-aggressive delete
//! policy outlived the intent to protect trigger images. Re-running
//! `ensure_repo` stays safe and idempotent.

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

/// How many of the most recent versions of each image the
/// `keep-most-recent` policy protects from deletion, regardless of
/// age. Sized with headroom for the rarely-rebuilt `CronJob` trigger
/// images: their latest build survives even when it is months old.
pub const KEEP_MOST_RECENT_VERSIONS: u64 = 10;

/// The cleanup policies attached to the repo, shared by create and
/// reconcile so the two paths can never drift. KEEP is evaluated
/// before DELETE: a version among the most recent
/// [`KEEP_MOST_RECENT_VERSIONS`] of its package survives even when it
/// is older than the delete threshold.
fn cleanup_policies() -> serde_json::Value {
    json!({
        "keep-most-recent": {
            "action": "KEEP",
            "mostRecentVersions": {
                "keepCount": KEEP_MOST_RECENT_VERSIONS,
            },
        },
        "delete-older-than-3d": {
            "action": "DELETE",
            "condition": {
                "olderThan": format!("{CLEANUP_OLDER_THAN_SECONDS}s"),
            },
        },
    })
}

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
        "cleanupPolicies": cleanup_policies(),
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
        // The repo already exists. Create never updates an existing
        // repo's cleanup policies, so reconcile them explicitly — this
        // is what carries a policy change in this file onto the live
        // prod repo instead of leaving it frozen at its original shape.
        409 => {
            reconcile_cleanup_policies(client, project_id).await?;
            Ok(EnsureOutcome::AlreadyExists)
        }
        other => Err(SetupError::BadStatus {
            operation: format!("create artifact-registry repo {REPO_ID}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

/// PATCH an existing repo so a cleanup-policy change in this file
/// reaches a repo created by an earlier run. `repositories.patch`
/// returns the updated `Repository` synchronously (no LRO); the
/// `updateMask` scopes the write to `cleanupPolicies` and nothing
/// else, so it never disturbs the repo's format or description.
async fn reconcile_cleanup_policies(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    let body = json!({ "cleanupPolicies": cleanup_policies() });
    let path = format!(
        "/v1/projects/{project_id}/locations/{DEFAULT_REGION}/repositories/{REPO_ID}?updateMask=cleanupPolicies",
    );
    let resp = client
        .patch_json(GcpService::ArtifactRegistry, &path, &body)
        .await?;
    match resp.status_u16() {
        200..=299 => Ok(()),
        other => Err(SetupError::BadStatus {
            operation: format!("update cleanup policies on repo {REPO_ID}"),
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
    use super::{
        ensure_repo, EnsureOutcome, CLEANUP_OLDER_THAN_SECONDS, KEEP_MOST_RECENT_VERSIONS, REPO_ID,
    };

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
                    "keep-most-recent": {
                        "action": "KEEP",
                        "mostRecentVersions": { "keepCount": KEEP_MOST_RECENT_VERSIONS }
                    },
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
        // 409 now reconciles the cleanup policies via PATCH.
        Mock::given(method("PATCH"))
            .and(path(
                "/v1/projects/proj/locations/us-west4/repositories/navigator",
            ))
            .and(query_param("updateMask", "cleanupPolicies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "navigator" })))
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
        // The second run hits 409 and reconciles via PATCH.
        Mock::given(method("PATCH"))
            .and(path(
                "/v1/projects/proj/locations/us-west4/repositories/navigator",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "navigator" })))
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
            "body should set the delete policy: {body}"
        );
        assert!(
            body.contains("259200s"),
            "body should set 3-day olderThan: {body}"
        );
        assert!(
            body.contains("keep-most-recent"),
            "body should set the keep policy: {body}"
        );
        assert!(
            body.contains("\"keepCount\":10"),
            "body should keep the most recent versions: {body}"
        );
    }

    #[tokio::test]
    async fn reconciles_cleanup_policy_when_repo_already_exists() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(409))
            .expect(1)
            .mount(&server)
            .await;
        // The reconcile PATCH must carry the keep policy and the
        // updateMask that scopes the write to cleanupPolicies only.
        Mock::given(method("PATCH"))
            .and(path(
                "/v1/projects/proj/locations/us-west4/repositories/navigator",
            ))
            .and(query_param("updateMask", "cleanupPolicies"))
            .and(body_partial_json(json!({
                "cleanupPolicies": {
                    "keep-most-recent": {
                        "action": "KEEP",
                        "mostRecentVersions": { "keepCount": KEEP_MOST_RECENT_VERSIONS }
                    }
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "navigator" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let outcome = ensure_repo(&client, "proj").await.unwrap();
        assert_eq!(outcome, EnsureOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn reconcile_patch_failure_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/locations/us-west4/repositories"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(
                "/v1/projects/proj/locations/us-west4/repositories/navigator",
            ))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let client = client_pointed_at(&server);
        let err = ensure_repo(&client, "proj").await.unwrap_err();
        assert!(format!("{err}").contains("500"), "got {err}");
    }
}
