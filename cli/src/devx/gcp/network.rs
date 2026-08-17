//! Provision the Neon Law Navigator VPC.
//!
//! ## Scope
//!
//! Each deployment gets one custom-mode VPC and one explicitly named regional
//! subnet with Private Google Access. GKE Autopilot selects both names. Every
//! managed service the workloads reach is a public Google endpoint, so
//! private-services-access peering is not required.
//!
//! Should a private-IP managed service ever arrive, the additions go here:
//! subnet → global address (`PURPOSE=VPC_PEERING`) →
//! `servicenetworking.connections.create`. All three follow the
//! same insert-then-poll-LRO pattern the other steps use.
//!
//! ## Idempotency
//!
//! `compute.networks.insert` returns HTTP **409 Conflict** when a
//! network with the same name already exists — same trick as
//! buckets. The LRO poll is skipped on 409.
//! A newly enabled Compute API can briefly return `SERVICE_DISABLED`
//! after the Service Usage operation completes. The first VPC insert
//! retries only that exact propagation response; unrelated 403s still
//! fail immediately.

use std::time::Duration;

use serde_json::json;

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::{lro, services, SetupConfig};

/// Default VPC network name. Overridable via `NAVIGATOR_VPC_NAME`.
pub const DEFAULT_NETWORK_NAME: &str = "navigator-vpc";
/// Default regional subnetwork name. Overridable via
/// `NAVIGATOR_SUBNETWORK_NAME`.
pub const DEFAULT_SUBNETWORK_NAME: &str = "navigator-subnet";
/// Compute's service activation can lag the completed Service Usage LRO.
const API_ACTIVATION_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// Bound activation propagation retries to two minutes.
const API_ACTIVATION_MAX_ATTEMPTS: usize = 25;

pub async fn ensure_network(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<()> {
    ensure_named_network(client, project_id, &config.vpc_name).await
}

/// Ensure a named custom-mode VPC without inheriting the production setup
/// configuration. Every deployment calls this seam so it cannot
/// acquire cluster or Config Sync settings by construction.
pub async fn ensure_named_network(
    client: &GcpClient,
    project_id: &str,
    network_name: &str,
) -> SetupResult<()> {
    ensure_named_network_with_retry(
        client,
        project_id,
        network_name,
        API_ACTIVATION_RETRY_INTERVAL,
        API_ACTIVATION_MAX_ATTEMPTS,
    )
    .await
}

async fn ensure_named_network_with_retry(
    client: &GcpClient,
    project_id: &str,
    network_name: &str,
    retry_interval: Duration,
    max_attempts: usize,
) -> SetupResult<()> {
    let body = json!({
        "name": network_name,
        "autoCreateSubnetworks": false,
        "routingConfig": { "routingMode": "REGIONAL" }
    });
    for attempt in 1..=max_attempts.max(1) {
        let resp = client
            .post_json(
                GcpService::Compute,
                &format!("/compute/v1/projects/{project_id}/global/networks"),
                &body,
            )
            .await?;
        let status = resp.status_u16();
        let response_body = resp.into_text();
        match status {
            409 => return Ok(()),
            200..=299 => {
                let op: serde_json::Value =
                    serde_json::from_str(&response_body).map_err(|source| SetupError::Json {
                        what: "network insert response",
                        source,
                    })?;
                lro::wait(
                    client,
                    GcpService::Compute,
                    &op,
                    &format!("/compute/v1/projects/{project_id}/global/operations/{{name}}"),
                )
                .await?;
                return Ok(());
            }
            403 if services::activation_is_propagating(
                &response_body,
                "compute.googleapis.com",
            ) && attempt < max_attempts.max(1) =>
            {
                eprintln!(
                    "gcp api [compute.googleapis.com] activation is still propagating for \
                     {project_id}; retrying VPC {network_name} ({attempt}/{max_attempts})"
                );
                if retry_interval.is_zero() {
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(retry_interval).await;
                }
            }
            other => {
                return Err(SetupError::BadStatus {
                    operation: format!("create VPC {network_name}"),
                    status: other,
                    body: response_body,
                });
            }
        }
    }
    unreachable!("the retry loop always executes at least once")
}

/// Ensure the regional subnet a custom-mode VPC needs before GKE can select
/// it. Staging calls this directly rather than falling back to the project's
/// default network.
pub async fn ensure_named_subnetwork(
    client: &GcpClient,
    project_id: &str,
    region: &str,
    network_name: &str,
    subnetwork_name: &str,
) -> SetupResult<()> {
    let body = json!({
        "name": subnetwork_name,
        "network": format!("projects/{project_id}/global/networks/{network_name}"),
        "ipCidrRange": "10.82.0.0/20",
        "region": region,
        "privateIpGoogleAccess": true,
    });
    let resp = client
        .post_json(
            GcpService::Compute,
            &format!("/compute/v1/projects/{project_id}/regions/{region}/subnetworks"),
            &body,
        )
        .await?;
    let status = resp.status_u16();
    match status {
        409 => Ok(()),
        200..=299 => {
            let op: serde_json::Value =
                serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
                    what: "subnetwork insert response",
                    source,
                })?;
            lro::wait(
                client,
                GcpService::Compute,
                &op,
                &format!("/compute/v1/projects/{project_id}/regions/{region}/operations/{{name}}"),
            )
            .await
            .map(|_| ())
        }
        other => Err(SetupError::BadStatus {
            operation: format!("create subnet {subnetwork_name}"),
            status: other,
            body: resp.into_text(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::super::SetupConfig;
    use super::{ensure_named_network_with_retry, ensure_network, DEFAULT_NETWORK_NAME};

    fn client_for(server: &MockServer) -> GcpClient {
        GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Compute, server.uri())
    }

    #[tokio::test]
    async fn inserts_custom_mode_vpc_then_waits_for_lro() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/global/networks"))
            .and(body_partial_json(json!({
                "name": DEFAULT_NETWORK_NAME,
                "autoCreateSubnetworks": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operation-123",
                "status": "RUNNING"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/compute/v1/projects/p/global/operations/operation-123",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operation-123",
                "status": "DONE"
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        ensure_network(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn inserts_subnet_then_polls_the_regional_operation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/regions/us-west1/subnetworks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operation-456",
                "status": "PENDING"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/compute/v1/projects/p/regions/us-west1/operations/operation-456",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operation-456",
                "status": "DONE"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        super::ensure_named_subnetwork(
            &client,
            "p",
            "us-west1",
            DEFAULT_NETWORK_NAME,
            super::DEFAULT_SUBNETWORK_NAME,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn treats_409_as_already_exists_and_skips_polling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/global/networks"))
            .respond_with(ResponseTemplate::new(409).set_body_string("already exists"))
            .expect(1)
            .mount(&server)
            .await;
        // No GET mock — if we tried to poll, wiremock would 404 the
        // call and fail the test.
        let client = client_for(&server);
        ensure_network(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retries_vpc_insert_while_compute_api_activation_propagates() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/global/networks"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {
                    "status": "PERMISSION_DENIED",
                    "details": [{
                        "reason": "SERVICE_DISABLED",
                        "metadata": {
                            "service": "compute.googleapis.com"
                        }
                    }]
                }
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/global/networks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operation-after-activation",
                "status": "DONE"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        ensure_named_network_with_retry(
            &client,
            "p",
            DEFAULT_NETWORK_NAME,
            std::time::Duration::ZERO,
            2,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn does_not_retry_an_unrelated_compute_permission_denial() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/compute/v1/projects/p/global/networks"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": {
                    "status": "PERMISSION_DENIED",
                    "message": "caller lacks compute.networks.create"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server);
        let err = ensure_named_network_with_retry(
            &client,
            "p",
            DEFAULT_NETWORK_NAME,
            std::time::Duration::ZERO,
            2,
        )
        .await
        .unwrap_err();
        assert!(
            format!("{err}").contains("caller lacks compute.networks.create"),
            "got {err}"
        );
    }

    #[tokio::test]
    async fn dry_run_records_only_the_post() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::Compute, "http://127.0.0.1:1")
            .with_dry_run();
        ensure_network(&client, "p", &SetupConfig::default())
            .await
            .unwrap();
        let calls = client.recorded_calls();
        assert_eq!(
            calls.len(),
            1,
            "dry-run should only record the insert, got {calls:?}"
        );
        assert!(calls[0].url.ends_with("/global/networks"));
    }
}
