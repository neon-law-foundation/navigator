//! Identity-Aware Proxy automation for `navigator-web`.
//!
//! Two operator tasks become REST round-trips here:
//!
//! - **`audience`** — look up the numeric ID GKE's HTTPS LB
//!   assigned to the `navigator-web` Compute backend service and
//!   format the audience string `web::iap::IapConfig` expects:
//!   `/projects/<PROJECT_NUMBER>/global/backendServices/<SERVICE_ID>`.
//!
//! - **`grant`** — add a principal to
//!   `roles/iap.httpsResourceAccessor` on the same backend service
//!   via IAP's `setIamPolicy` endpoint. Idempotent: if the member +
//!   role pair is already present, the policy is left untouched.
//!
//! The brand / OAuth-client provisioning the original runbook
//! described is **not** required: a `BackendConfig` with
//! `iap.enabled: true` and no `oauthclientCredentials` makes IAP
//! provision a Google-managed OAuth client on its own (GKE 1.29.4+).
//!
//! ## API surfaces
//!
//! - Cloud Resource Manager v3 — `GET /v3/projects/{ID}` returns
//!   `name: "projects/<NUMBER>"`. The trailing segment is the
//!   project number IAP IAM paths require.
//! - Compute Engine v1 — `GET /compute/v1/projects/{ID}/global/
//!   backendServices/{NAME}` returns `id: "<NUMERIC>"`.
//! - IAP v1 — `:getIamPolicy` / `:setIamPolicy` on
//!   `projects/<NUMBER>/iap_web/compute/services/{NAME}` (POST, no
//!   body for get; new policy in body for set).

use serde_json::{json, Value};

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};

/// Compute backend-service name our GKE Ingress creates. Tied to
/// the `navigator-web` Service in `k8s/overlays/gke/`.
pub const DEFAULT_SERVICE_NAME: &str = "navigator-web";

/// IAM role that lets a principal pass through IAP.
pub const IAP_ROLE: &str = "roles/iap.httpsResourceAccessor";

/// Look up the project's numeric ID. The IAP IAM REST path requires
/// the *number*, not the alphanumeric ID.
pub async fn get_project_number(client: &GcpClient, project_id: &str) -> SetupResult<String> {
    let path = format!("/v3/projects/{project_id}");
    let resp = client.get(GcpService::CloudResourceManager, &path).await?;
    let status = resp.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: format!("get project {project_id}"),
            status,
            body: resp.into_text(),
        });
    }
    let body: Value =
        serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
            what: "get project response",
            source,
        })?;
    body.get("name")
        .and_then(Value::as_str)
        .and_then(|n| n.strip_prefix("projects/"))
        .map(str::to_string)
        .ok_or_else(|| SetupError::BadStatus {
            operation: format!("parse project number from get-project {project_id}"),
            status: 200,
            body: body.to_string(),
        })
}

/// Look up the numeric ID GKE assigned to the `service_name` global
/// Compute backend service. The LB must be provisioned first; this
/// returns `BadStatus { status: 404 }` until then.
pub async fn get_backend_service_id(
    client: &GcpClient,
    project_id: &str,
    service_name: &str,
) -> SetupResult<String> {
    let path = format!("/compute/v1/projects/{project_id}/global/backendServices/{service_name}");
    let resp = client.get(GcpService::Compute, &path).await?;
    let status = resp.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: format!("get backend service {service_name}"),
            status,
            body: resp.into_text(),
        });
    }
    let body: Value =
        serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
            what: "get backend service response",
            source,
        })?;
    body.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| SetupError::BadStatus {
            operation: format!("parse id from backend service {service_name}"),
            status: 200,
            body: body.to_string(),
        })
}

/// Format the audience string `web::iap::IapConfig` validates against.
#[must_use]
pub fn format_iap_audience(project_number: &str, backend_service_id: &str) -> String {
    format!("/projects/{project_number}/global/backendServices/{backend_service_id}")
}

/// Outcome of an `ensure_iap_iam_binding` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingOutcome {
    Added,
    AlreadyPresent,
}

/// Idempotently add `member` to `roles/iap.httpsResourceAccessor`
/// on the IAP-protected backend service. Reads the current policy,
/// returns `AlreadyPresent` if nothing would change, otherwise
/// writes the merged policy back.
pub async fn ensure_iap_iam_binding(
    client: &GcpClient,
    project_number: &str,
    service_name: &str,
    member: &str,
) -> SetupResult<BindingOutcome> {
    let path = format!(
        "/v1/projects/{project_number}/iap_web/compute/services/{service_name}:getIamPolicy"
    );
    let resp = client.post_json(GcpService::Iap, &path, &json!({})).await?;
    let status = resp.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: format!("getIamPolicy for {service_name}"),
            status,
            body: resp.into_text(),
        });
    }
    let mut policy: Value =
        serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
            what: "getIamPolicy response",
            source,
        })?;

    if policy_contains_member(&policy, IAP_ROLE, member) {
        return Ok(BindingOutcome::AlreadyPresent);
    }
    upsert_member(&mut policy, IAP_ROLE, member);

    let set_path = format!(
        "/v1/projects/{project_number}/iap_web/compute/services/{service_name}:setIamPolicy"
    );
    let resp = client
        .post_json(GcpService::Iap, &set_path, &json!({ "policy": policy }))
        .await?;
    let status = resp.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: format!("setIamPolicy for {service_name}"),
            status,
            body: resp.into_text(),
        });
    }
    Ok(BindingOutcome::Added)
}

fn policy_contains_member(policy: &Value, role: &str, member: &str) -> bool {
    policy
        .get("bindings")
        .and_then(Value::as_array)
        .is_some_and(|bindings| {
            bindings.iter().any(|b| {
                b.get("role").and_then(Value::as_str) == Some(role)
                    && b.get("members")
                        .and_then(Value::as_array)
                        .is_some_and(|m| m.iter().any(|x| x.as_str() == Some(member)))
            })
        })
}

fn upsert_member(policy: &mut Value, role: &str, member: &str) {
    let obj = policy
        .as_object_mut()
        .expect("policy should be a JSON object");
    let bindings = obj
        .entry("bindings".to_string())
        .or_insert_with(|| json!([]));
    let arr = bindings
        .as_array_mut()
        .expect("bindings should be an array");
    for b in arr.iter_mut() {
        if b.get("role").and_then(Value::as_str) == Some(role) {
            let members = b
                .as_object_mut()
                .unwrap()
                .entry("members".to_string())
                .or_insert_with(|| json!([]));
            members
                .as_array_mut()
                .unwrap()
                .push(Value::String(member.to_string()));
            return;
        }
    }
    arr.push(json!({ "role": role, "members": [member] }));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::{
        ensure_iap_iam_binding, format_iap_audience, get_backend_service_id, get_project_number,
        BindingOutcome, DEFAULT_SERVICE_NAME, IAP_ROLE,
    };

    fn client_pointed_at(server: &MockServer, services: &[GcpService]) -> GcpClient {
        let mut c = GcpClient::new(Arc::new(StaticToken("t".into())));
        for s in services {
            c = c.with_base_url(*s, server.uri());
        }
        c
    }

    #[tokio::test]
    async fn get_project_number_extracts_numeric_segment_from_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v3/projects/my-proj"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "projects/123456789012",
                "projectId": "my-proj",
                "displayName": "My Proj"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_pointed_at(&server, &[GcpService::CloudResourceManager]);
        let n = get_project_number(&client, "my-proj").await.unwrap();
        assert_eq!(n, "123456789012");
    }

    #[tokio::test]
    async fn get_backend_service_id_returns_compute_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/compute/v1/projects/proj/global/backendServices/{DEFAULT_SERVICE_NAME}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "9988776655",
                "name": DEFAULT_SERVICE_NAME
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = client_pointed_at(&server, &[GcpService::Compute]);
        let id = get_backend_service_id(&client, "proj", DEFAULT_SERVICE_NAME)
            .await
            .unwrap();
        assert_eq!(id, "9988776655");
    }

    #[tokio::test]
    async fn get_backend_service_id_404_surfaces_as_bad_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/compute/v1/projects/proj/global/backendServices/{DEFAULT_SERVICE_NAME}"
            )))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;
        let client = client_pointed_at(&server, &[GcpService::Compute]);
        let err = get_backend_service_id(&client, "proj", DEFAULT_SERVICE_NAME)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("404"), "got {err}");
    }

    #[test]
    fn format_iap_audience_matches_iap_signed_aud_format() {
        let s = format_iap_audience("123456789012", "9988776655");
        assert_eq!(
            s,
            "/projects/123456789012/global/backendServices/9988776655"
        );
    }

    #[tokio::test]
    async fn ensure_iap_iam_binding_adds_when_member_absent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/123/iap_web/compute/services/{DEFAULT_SERVICE_NAME}:getIamPolicy"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "bindings": [
                    { "role": "roles/iap.httpsResourceAccessor",
                      "members": ["group:already@example.com"] }
                ],
                "etag": "abc"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/123/iap_web/compute/services/{DEFAULT_SERVICE_NAME}:setIamPolicy"
            )))
            .and(body_partial_json(json!({
                "policy": {
                    "bindings": [{
                        "role": IAP_ROLE,
                        "members": ["group:already@example.com", "group:new@example.com"]
                    }]
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server, &[GcpService::Iap]);
        let outcome = ensure_iap_iam_binding(
            &client,
            "123",
            DEFAULT_SERVICE_NAME,
            "group:new@example.com",
        )
        .await
        .unwrap();
        assert_eq!(outcome, BindingOutcome::Added);
    }

    #[tokio::test]
    async fn ensure_iap_iam_binding_skips_set_iam_policy_when_already_present() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/123/iap_web/compute/services/{DEFAULT_SERVICE_NAME}:getIamPolicy"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "bindings": [
                    { "role": IAP_ROLE, "members": ["group:already@example.com"] }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        // No setIamPolicy mock — call must not fire.

        let client = client_pointed_at(&server, &[GcpService::Iap]);
        let outcome = ensure_iap_iam_binding(
            &client,
            "123",
            DEFAULT_SERVICE_NAME,
            "group:already@example.com",
        )
        .await
        .unwrap();
        assert_eq!(outcome, BindingOutcome::AlreadyPresent);
    }

    #[tokio::test]
    async fn ensure_iap_iam_binding_creates_role_when_policy_has_no_bindings() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/123/iap_web/compute/services/{DEFAULT_SERVICE_NAME}:getIamPolicy"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v1/projects/123/iap_web/compute/services/{DEFAULT_SERVICE_NAME}:setIamPolicy"
            )))
            .and(body_partial_json(json!({
                "policy": {
                    "bindings": [{ "role": IAP_ROLE, "members": ["serviceAccount:s@p.iam.gserviceaccount.com"] }]
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_pointed_at(&server, &[GcpService::Iap]);
        let outcome = ensure_iap_iam_binding(
            &client,
            "123",
            DEFAULT_SERVICE_NAME,
            "serviceAccount:s@p.iam.gserviceaccount.com",
        )
        .await
        .unwrap();
        assert_eq!(outcome, BindingOutcome::Added);
    }

    #[tokio::test]
    async fn dry_run_audience_lookup_records_one_get_per_endpoint() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::CloudResourceManager, "http://127.0.0.1:1")
            .with_base_url(GcpService::Compute, "http://127.0.0.1:1")
            .with_dry_run();
        // Dry-run returns an empty JSON body, so the parse step
        // will fail to find "name"/"id"; just verify the URLs got
        // recorded.
        let _ = get_project_number(&client, "proj").await;
        let _ = get_backend_service_id(&client, "proj", DEFAULT_SERVICE_NAME).await;
        let calls = client.recorded_calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].url.contains("/v3/projects/proj"));
        assert!(calls[1]
            .url
            .contains("/compute/v1/projects/proj/global/backendServices/navigator-web"));
    }
}
