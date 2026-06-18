//! Manual trigger for the nightly `Archives` export workflow.
//!
//! `POST /portal/admin/archives/run` fires the same `Archives` Restate
//! workflow the `archives-trigger` CronJob runs nightly — the durable
//! snapshot → GCP-cost → diagnostic-email pipeline in
//! [`archives::workflow`]. It exists so an operator can (a) test the
//! pipeline end-to-end after a deploy and (b) re-run a missed night on
//! demand, without waiting for the 02:00 PST schedule.
//!
//! Unlike the nightly fire (keyed by UTC date so a double-fire is a
//! no-op), the manual run uses a unique `manual-<uuid>` key — Restate
//! admits at most one invocation per key, so a date key would make the
//! button a silent no-op once the night already ran. The unique key
//! guarantees every click actually executes and emails.
//!
//! Reuses the exact web→Restate-with-bearer path the other workflow
//! triggers use: `RESTATE_BROKER_URL` is the ingress,
//! `RESTATE_AUTH_TOKEN` the optional Restate Cloud bearer. The shared
//! [`workflows::start_workflow`] helper attaches the bearer only when
//! present, so this works against both Restate Cloud and the KIND
//! Operator. When no broker is configured the handler returns 503
//! rather than guessing an endpoint.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;
use views::pages::admin::archives as archives_views;

/// Default diagnostic-email recipient, mirrored from
/// `archives::workflow::DEFAULT_NOTIFY_EMAIL` so the confirmation page
/// names the same address the workflow will actually email.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// `POST /portal/admin/archives/run`. Gated by the admin router's
/// `require_auth` + policy + CSRF layers, so reaching this handler
/// already means an authenticated staff/admin session.
pub async fn run() -> Response {
    let Some(broker) = restate_broker_url() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            archives_views::failed(
                "Restate broker not configured",
                "RESTATE_BROKER_URL is unset on this deploy, so there is no ingress to start the Archives workflow.",
            ),
        )
            .into_response();
    };
    let token = std::env::var("RESTATE_AUTH_TOKEN").ok();
    let run_key = manual_run_key();

    match trigger_manual(&broker, token.as_deref(), &run_key).await {
        Ok(response) => {
            tracing::info!(run_key = %run_key, response = %response, "manual archives trigger accepted");
            archives_views::triggered(&run_key, &notify_email()).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, run_key = %run_key, "manual archives trigger failed");
            (
                StatusCode::BAD_GATEWAY,
                archives_views::failed("Restate ingress rejected the call", &e.to_string()),
            )
                .into_response()
        }
    }
}

/// Fire the `Archives` workflow one-way under `run_key`. Split out so
/// the wire shape (one-way `/send`, `manual-` key path, optional
/// bearer) is unit-testable against a mock ingress without
/// process-env plumbing.
async fn trigger_manual(
    broker: &str,
    token: Option<&str>,
    run_key: &str,
) -> Result<String, workflows::TriggerError> {
    workflows::start_workflow(
        broker,
        token,
        "Archives",
        run_key,
        "run",
        &serde_json::json!({}),
        true, // one-way: don't hold the HTTP request open for the whole snapshot.
    )
    .await
}

/// `manual-<uuid>` — unique per click so Restate never dedupes a
/// manual run against the date-keyed nightly fire.
fn manual_run_key() -> String {
    format!("manual-{}", Uuid::new_v4())
}

/// Diagnostic-email recipient shown on the confirmation page:
/// `ARCHIVES_NOTIFY_EMAIL` or the default.
fn notify_email() -> String {
    std::env::var("ARCHIVES_NOTIFY_EMAIL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string())
}

/// `RESTATE_BROKER_URL`, trimmed and treated as absent when empty.
/// Same selection `web::main` uses.
fn restate_broker_url() -> Option<String> {
    std::env::var("RESTATE_BROKER_URL")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{manual_run_key, trigger_manual};
    use workflows::TriggerError;

    use wiremock::matchers::{body_partial_json, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn manual_run_key_is_unique_and_prefixed() {
        let a = manual_run_key();
        let b = manual_run_key();
        assert!(
            a.starts_with("manual-"),
            "key should carry the manual- prefix"
        );
        assert_ne!(a, b, "each click must produce a distinct key");
    }

    #[tokio::test]
    async fn trigger_manual_fires_one_way_against_a_manual_keyed_archives_run() {
        let server = MockServer::start().await;
        // One-way `/send` to a `manual-<uuid>` key under the Archives
        // service, carrying the empty RunRequest body.
        Mock::given(method("POST"))
            .and(path_regex(r"^/Archives/manual-[0-9a-fA-F-]+/run/send$"))
            .and(body_partial_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("{\"invocationId\":\"inv_x\"}"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let key = manual_run_key();
        let body = trigger_manual(&server.uri(), None, &key)
            .await
            .expect("ingress accepted the invocation");
        assert!(body.contains("inv_x"));
    }

    #[tokio::test]
    async fn trigger_manual_attaches_bearer_when_token_present() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/Archives/manual-[0-9a-fA-F-]+/run/send$"))
            .and(wiremock::matchers::header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let key = manual_run_key();
        trigger_manual(&server.uri(), Some("tok"), &key)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn trigger_manual_surfaces_ingress_rejection() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/Archives/manual-[0-9a-fA-F-]+/run/send$"))
            .respond_with(ResponseTemplate::new(401).set_body_string("missing bearer"))
            .mount(&server)
            .await;

        let key = manual_run_key();
        let err = trigger_manual(&server.uri(), None, &key).await.unwrap_err();
        assert!(matches!(err, TriggerError::Rejected { .. }));
    }
}
