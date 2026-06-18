//! Start a Restate workflow by POSTing to the ingress.
//!
//! The one shared way the application *kicks off* a durable workflow
//! from outside Restate. Two callers converge here:
//!
//! - the `archives` crate's `trigger` binary (the nightly CronJob),
//!   which fires the `Archives` export workflow once per night, and
//! - `web`'s admin "Run nightly export now" button
//!   (`web::archives`), which fires the same `Archives` workflow on
//!   demand for testing / recovery.
//!
//! Both need the same wire shape — `POST {ingress}/{Service}/{key}/{handler}`
//! with an optional `Authorization: Bearer …` header. Restate Cloud
//! authenticates every ingress call with the tenant bearer token
//! (`RESTATE_AUTH_TOKEN`); the in-cluster Restate Operator used in
//! KIND does not. Passing `auth_token = None` (or an empty string)
//! sends no header at all, so the same code path works in both
//! environments — the exact contract the [`crate::RestateRuntime`]
//! adapter already follows for the `notation` service.
//!
//! `one_way = true` targets Restate's `/send` variant: the call
//! returns as soon as the invocation is *accepted* (Restate then runs
//! it to completion on the worker, owning the retry schedule). Use it
//! when the caller must not block on the whole run — e.g. an HTTP
//! handler that would otherwise hold a request open for the duration
//! of a 26-table snapshot.

use serde::Serialize;
use thiserror::Error;

/// Failure starting a workflow invocation through the ingress.
#[derive(Debug, Error)]
pub enum TriggerError {
    /// The HTTP request never produced a response (DNS, connect,
    /// timeout). Carries the URL so logs name the unreachable ingress.
    #[error("transport error calling {url}: {source}")]
    Transport {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    /// The ingress responded with a non-2xx status. A `401` here is
    /// the classic "bearer token missing or wrong" — the bug that
    /// silently stopped the nightly archives email.
    #[error("workflow trigger {url} returned {status}: {body}")]
    Rejected {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
}

/// POST to the Restate ingress to start one invocation of
/// `{service}/{key}/{handler}`.
///
/// - `ingress` — the Restate ingress base URL (Restate Cloud in prod,
///   the in-cluster `restate` Service in KIND). A trailing slash is
///   trimmed.
/// - `auth_token` — `Some(non-empty)` attaches `Authorization: Bearer
///   …`; `None` or `Some("")` sends no header (KIND / dev).
/// - `service` / `key` / `handler` — the Restate virtual-object
///   coordinates, e.g. `("Archives", "2026-06-05", "run")`.
/// - `body` — JSON request body for the handler (`&serde_json::json!({})`
///   for handlers that take an empty struct).
/// - `one_way` — `true` appends `/send` so the call returns on
///   acceptance instead of blocking until the workflow completes.
///
/// On success returns the ingress response body (for `/send` this is
/// the JSON `{"invocationId": "inv_…"}` the caller can log).
///
/// # Errors
///
/// [`TriggerError::Transport`] when the request can't be sent;
/// [`TriggerError::Rejected`] on any non-success status.
#[tracing::instrument(
    level = "info",
    name = "workflow.trigger",
    skip(auth_token, body),
    fields(service = service, key = key, handler = handler, one_way)
)]
pub async fn start_workflow<B: Serialize + ?Sized>(
    ingress: &str,
    auth_token: Option<&str>,
    service: &str,
    key: &str,
    handler: &str,
    body: &B,
    one_way: bool,
) -> Result<String, TriggerError> {
    let suffix = if one_way { "/send" } else { "" };
    let url = format!(
        "{}/{}/{}/{}{}",
        ingress.trim_end_matches('/'),
        service,
        key,
        handler,
        suffix
    );

    // Bound the POST so a hung or unreachable ingress can never leave a
    // trigger pod running indefinitely. A `CronJob` with
    // `concurrencyPolicy: Forbid` treats a still-running job as a reason to
    // skip the next schedule, so an unbounded request turns one transient
    // ingress stall into a permanently wedged schedule (this is one half of
    // how the nightly Archives trigger silently stopped firing). 30s is far
    // longer than a healthy one-way `/send` (milliseconds) yet short enough
    // that the Job's `activeDeadlineSeconds` backstop and the next schedule
    // both still apply.
    let mut req = reqwest::Client::new()
        .post(&url)
        .json(body)
        .timeout(std::time::Duration::from_secs(30));
    // Empty token is treated as absent: a mounted-but-empty secret
    // must not produce `Authorization: Bearer ` (Restate Cloud rejects
    // that as malformed). Mirrors `RestateRuntime::with_auth_token`.
    if let Some(token) = auth_token.filter(|t| !t.is_empty()) {
        req = req.bearer_auth(token);
    }

    // Inject the current span's W3C trace context (`traceparent`) so the
    // workflow handler can continue this trace across the Restate boundary
    // (extracted handler-side from `ctx.headers()`; see telemetry). Empty —
    // and a no-op — when OTLP is unconfigured or no sampled span is active, so
    // dev / KIND / OSS forks attach nothing. Only opaque trace context crosses
    // here, never a client field.
    for (name, value) in telemetry::current_trace_context_headers() {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value),
        ) {
            req = req.header(name, value);
        }
    }

    // Record the outcome as a metric (`navigator.workflow.trigger.fired`) and a
    // structured event on every path — identifiers and counts only, never the
    // request body. This is the single instrumentation point every trigger
    // funnels through, so a service whose scheduled fire silently stops shows
    // up as a flat counter line and an absent "accepted" event.
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(source) => {
            telemetry::record_trigger_fired(service, telemetry::outcome::TRANSPORT_ERROR);
            tracing::error!(service, %url, error = %source, "workflow trigger transport error");
            return Err(TriggerError::Transport { url, source });
        }
    };
    let status = resp.status();
    let resp_body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        telemetry::record_trigger_fired(service, telemetry::outcome::REJECTED);
        tracing::error!(
            service,
            status = status.as_u16(),
            "workflow trigger rejected by ingress"
        );
        return Err(TriggerError::Rejected {
            url,
            status,
            body: resp_body,
        });
    }
    telemetry::record_trigger_fired(service, telemetry::outcome::ACCEPTED);
    tracing::info!(
        service,
        status = status.as_u16(),
        "workflow trigger accepted"
    );
    Ok(resp_body)
}

#[cfg(test)]
mod tests {
    use super::{start_workflow, TriggerError};
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn posts_to_service_key_handler_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/2026-06-05/run"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("{\"invocationId\":\"inv_1\"}"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let body = start_workflow(
            &server.uri(),
            None,
            "Archives",
            "2026-06-05",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap();
        assert!(body.contains("inv_1"));
    }

    #[tokio::test]
    async fn one_way_targets_the_send_variant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/manual-7/run/send"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        start_workflow(
            &server.uri(),
            None,
            "Archives",
            "manual-7",
            "run",
            &json!({}),
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn attaches_bearer_when_token_present() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .and(header("authorization", "Bearer s3cret"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        start_workflow(
            &server.uri(),
            Some("s3cret"),
            "Archives",
            "d",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn sends_no_authorization_header_when_token_absent() {
        let server = MockServer::start().await;
        // Any request carrying an Authorization header must NOT match.
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        start_workflow(
            &server.uri(),
            None,
            "Archives",
            "d",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn empty_token_is_treated_as_absent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        start_workflow(
            &server.uri(),
            Some(""),
            "Archives",
            "d",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn passes_the_json_body_through() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .and(body_partial_json(json!({"run_date": "2026-06-05"})))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        start_workflow(
            &server.uri(),
            None,
            "Archives",
            "d",
            "run",
            &json!({"run_date": "2026-06-05"}),
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn non_success_status_becomes_rejected_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/Archives/d/run"))
            .respond_with(ResponseTemplate::new(401).set_body_string("missing bearer"))
            .mount(&server)
            .await;

        let err = start_workflow(
            &server.uri(),
            None,
            "Archives",
            "d",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap_err();
        match err {
            TriggerError::Rejected { status, body, .. } => {
                assert_eq!(status.as_u16(), 401);
                assert!(body.contains("missing bearer"));
            }
            other @ TriggerError::Transport { .. } => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unreachable_ingress_becomes_transport_error() {
        // Port 0 with a reserved TEST-NET host never connects.
        let err = start_workflow(
            "http://192.0.2.1:1",
            None,
            "Archives",
            "d",
            "run",
            &json!({}),
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, TriggerError::Transport { .. }));
    }
}
