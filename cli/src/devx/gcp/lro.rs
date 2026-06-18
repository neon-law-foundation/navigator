//! Long-running operation polling.
//!
//! GCP control-plane writes (`services.batchEnable`,
//! `compute.networks.insert`, `sql.instances.insert`,
//! `run.projects.locations.services.create`, …) return an
//! `Operation` resource that becomes `done: true` minutes later.
//! Every step in `setup` follows the same recipe: parse the
//! operation `name`, poll its status endpoint until done, and bail
//! if the operation reports an `error`.
//!
//! In [`Mode::DryRun`](super::client::Mode::DryRun), the synthetic
//! `{}` body that [`super::client::GcpClient`] returns is treated
//! as an already-complete operation — we skip the polling entirely.

use std::time::Duration;

use serde_json::Value;

use super::client::{GcpClient, GcpService, Mode};
use super::error::{SetupError, SetupResult};

/// How long to sleep between polls of an LRO. Tests override the
/// real timer via `with_poll_interval` on a `LroWaiter`; production
/// uses a few seconds.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Hard cap on total polling time. Cloud SQL inserts routinely take
/// 5–10 minutes; the others finish in seconds.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_mins(15);

/// Wait for the operation `op` (the JSON body returned from the
/// initial create call) to complete.
///
/// `status_path` is a format string with one `{name}` slot — e.g.
/// `"/v1/{name}"` for `ServiceUsage`, `"/compute/v1/{name}"` for
/// `Compute`. The operation's `name` field is interpolated into it.
pub async fn wait(
    client: &GcpClient,
    service: GcpService,
    op: &Value,
    status_path_template: &str,
) -> SetupResult<Value> {
    wait_with_interval(
        client,
        service,
        op,
        status_path_template,
        DEFAULT_POLL_INTERVAL,
    )
    .await
}

/// Same as [`wait`] but with a configurable poll interval (tests
/// pass `Duration::ZERO` to make polling effectively synchronous).
pub async fn wait_with_interval(
    client: &GcpClient,
    service: GcpService,
    op: &Value,
    status_path_template: &str,
    interval: Duration,
) -> SetupResult<Value> {
    // Dry-run short-circuits: the synthetic `{}` returned by
    // `record_and_synthesize` is "done" by definition.
    if client.mode() == Mode::DryRun {
        return Ok(op.clone());
    }
    if op.get("done").and_then(Value::as_bool) == Some(true) {
        return check_op_error(op);
    }
    let name = op
        .get("name")
        .and_then(Value::as_str)
        .ok_or(SetupError::Malformed("operation has no `name` field"))?
        .to_string();
    let path = status_path_template.replace("{name}", &name);

    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > DEFAULT_TIMEOUT {
            return Err(SetupError::OperationTimeout {
                name,
                timeout: DEFAULT_TIMEOUT,
            });
        }
        let resp = client.get(service, &path).await?;
        let status = resp.status_u16();
        if !(200..=299).contains(&status) {
            return Err(SetupError::BadStatus {
                operation: format!("polling {name}"),
                status,
                body: resp.into_text(),
            });
        }
        let body: Value =
            serde_json::from_str(&resp.into_text()).map_err(|source| SetupError::Json {
                what: "operation poll response",
                source,
            })?;
        if body.get("done").and_then(Value::as_bool) == Some(true) {
            return check_op_error(&body);
        }
        if interval.is_zero() {
            // Yield to the runtime so wiremock matchers can see the
            // queued response in tests.
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(interval).await;
        }
    }
}

fn check_op_error(op: &Value) -> SetupResult<Value> {
    if let Some(err) = op.get("error") {
        return Err(SetupError::OperationFailed(err.to_string()));
    }
    Ok(op.clone())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::client::{GcpClient, GcpService, StaticToken};
    use super::wait_with_interval;

    #[tokio::test]
    async fn returns_immediately_when_op_is_already_done() {
        let server = MockServer::start().await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        let op = json!({"name": "operations/x", "done": true});
        // No mocks registered — if we polled, wiremock would 404.
        let out = wait_with_interval(
            &client,
            GcpService::ServiceUsage,
            &op,
            "/v1/{name}",
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(out["done"], json!(true));
    }

    #[tokio::test]
    async fn polls_status_endpoint_until_done() {
        let server = MockServer::start().await;
        // First poll: not done. Second: done.
        Mock::given(method("GET"))
            .and(path("/v1/operations/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/abc",
                "done": false
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/operations/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/abc",
                "done": true
            })))
            .mount(&server)
            .await;

        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        let op = json!({"name": "operations/abc"});
        let out = wait_with_interval(
            &client,
            GcpService::ServiceUsage,
            &op,
            "/v1/{name}",
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(out["done"], json!(true));
    }

    #[tokio::test]
    async fn bails_when_op_completes_with_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/operations/bad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "operations/bad",
                "done": true,
                "error": { "code": 7, "message": "permission denied" }
            })))
            .mount(&server)
            .await;
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, server.uri());
        let op = json!({"name": "operations/bad"});
        let err = wait_with_interval(
            &client,
            GcpService::ServiceUsage,
            &op,
            "/v1/{name}",
            std::time::Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(format!("{err}").contains("permission denied"), "got {err}");
    }

    #[tokio::test]
    async fn dry_run_skips_polling_entirely() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, "http://127.0.0.1:1")
            .with_dry_run();
        let op = json!({"name": "operations/x"});
        // No `done` field, dry-run still returns immediately.
        let out = wait_with_interval(
            &client,
            GcpService::ServiceUsage,
            &op,
            "/v1/{name}",
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(out["name"], json!("operations/x"));
    }
}
