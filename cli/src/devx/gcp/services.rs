//! Enable the GCP APIs `setup` calls during the rest of the
//! pipeline. We use `serviceusage.batchEnable` so the whole list
//! goes through a single long-running operation.
//!
//! Enabling an already-enabled service is a no-op on Google's side
//! — the LRO just completes successfully. So we don't need to
//! special-case 409 here.

use serde_json::json;

use super::client::{GcpClient, GcpService};
use super::error::{SetupError, SetupResult};
use super::lro;

/// The APIs every `setup` run needs. Order is cosmetic.
pub const REQUIRED_SERVICES: &[&str] = &[
    "compute.googleapis.com",
    "sqladmin.googleapis.com",
    "servicenetworking.googleapis.com",
    "storage.googleapis.com",
    "iam.googleapis.com",
    "container.googleapis.com",
    "gkebackup.googleapis.com",
    "configconnector.googleapis.com",
    "anthosconfigmanagement.googleapis.com",
    "logging.googleapis.com",
    "secretmanager.googleapis.com",
    "certificatemanager.googleapis.com",
    "speech.googleapis.com",
];

pub async fn enable_services(client: &GcpClient, project_id: &str) -> SetupResult<()> {
    enable(client, project_id, REQUIRED_SERVICES).await
}

/// Enable an arbitrary list of GCP APIs on `project_id` via
/// `serviceusage.batchEnable`. Used by focused subcommands that want
/// only one API turned on rather than the full `REQUIRED_SERVICES` set.
pub async fn enable(client: &GcpClient, project_id: &str, service_ids: &[&str]) -> SetupResult<()> {
    let body = json!({ "serviceIds": service_ids });
    let resp = client
        .post_json(
            GcpService::ServiceUsage,
            &format!("/v1/projects/{project_id}/services:batchEnable"),
            &body,
        )
        .await?;
    let status = resp.status_u16();
    if !(200..=299).contains(&status) {
        return Err(SetupError::BadStatus {
            operation: "batchEnable".into(),
            status,
            body: resp.into_text(),
        });
    }
    let body: serde_json::Value =
        serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
            what: "batchEnable response",
            source,
        })?;
    lro::wait(client, GcpService::ServiceUsage, &body, "/v1/{name}").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::{enable_services, REQUIRED_SERVICES};

    #[tokio::test]
    async fn posts_batch_enable_with_full_service_list() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/services:batchEnable"))
            .and(body_partial_json(
                json!({ "serviceIds": REQUIRED_SERVICES }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/abc",
                "done": true
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        enable_services(&client, "proj").await.unwrap();
    }

    #[tokio::test]
    async fn waits_for_lro_when_initial_response_is_not_done() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/services:batchEnable"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/op1",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/operations/op1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/op1",
                "done": true
            })))
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        enable_services(&client, "proj").await.unwrap();
    }

    #[tokio::test]
    async fn bails_on_non_2xx_from_batch_enable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/proj/services:batchEnable"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        let err = enable_services(&client, "proj").await.unwrap_err();
        assert!(format!("{err}").contains("403"), "got {err}");
    }

    #[tokio::test]
    async fn dry_run_records_one_post_and_no_polling() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, "http://127.0.0.1:1")
            .with_dry_run();
        enable_services(&client, "proj").await.unwrap();
        let calls = client.recorded_calls();
        assert_eq!(
            calls.len(),
            1,
            "dry-run should record one POST, got {calls:?}"
        );
        assert_eq!(calls[0].method, "POST");
        assert!(calls[0].url.ends_with("/services:batchEnable"));
    }
}
