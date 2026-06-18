//! Provision the Navigator VPC.
//!
//! ## Scope
//!
//! For the first cut we create one custom-mode VPC and stop there.
//! Cloud Run reaches Cloud SQL through the Cloud SQL Auth Proxy
//! (Unix socket, via `INSTANCE_CONNECTION_NAME`), not through a
//! private IP — so we don't need a subnet, a VPC connector, or a
//! private-services-access peering at this stage.
//!
//! When we want private IP for Cloud SQL later, the additions go
//! here: subnet → global address (`PURPOSE=VPC_PEERING`) →
//! `servicenetworking.connections.create`. All three follow the
//! same insert-then-poll-LRO pattern the other steps use.
//!
//! ## Idempotency
//!
//! `compute.networks.insert` returns HTTP **409 Conflict** when a
//! network with the same name already exists — same trick as
//! buckets. The LRO poll is skipped on 409.

use serde_json::json;

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::{lro, SetupConfig};

/// Default VPC network name. Overridable via `NAVIGATOR_VPC_NAME`.
pub const DEFAULT_NETWORK_NAME: &str = "navigator-vpc";

pub async fn ensure_network(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<()> {
    let body = json!({
        "name": config.vpc_name,
        "autoCreateSubnetworks": false,
        "routingConfig": { "routingMode": "REGIONAL" }
    });
    let resp = client
        .post_json(
            GcpService::Compute,
            &format!("/compute/v1/projects/{project_id}/global/networks"),
            &body,
        )
        .await?;
    let status = resp.status_u16();
    match status {
        409 => return Ok(()),
        200..=299 => {}
        other => {
            return Err(SetupError::BadStatus {
                operation: format!("create VPC {}", config.vpc_name),
                status: other,
                body: resp.into_text(),
            });
        }
    }
    let op: serde_json::Value =
        serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
            what: "network insert response",
            source,
        })?;
    lro::wait(client, GcpService::Compute, &op, "/compute/v1/{name}").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::super::SetupConfig;
    use super::{ensure_network, DEFAULT_NETWORK_NAME};

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
                "name": "projects/p/global/operations/op1",
                "selfLink": "x",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/compute/v1/projects/p/global/operations/op1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "projects/p/global/operations/op1",
                "done": true
            })))
            .mount(&server)
            .await;

        let client = client_for(&server);
        ensure_network(&client, "p", &SetupConfig::default())
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
