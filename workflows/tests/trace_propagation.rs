//! The outbound side of cross-service tracing: `start_workflow` must inject a
//! well-formed W3C `traceparent` into the POST to the Restate ingress when a
//! sampled span is active, so the workflow handler can continue the caller's
//! trace (it extracts the header from `ctx.headers()` — see telemetry).
//!
//! This is its own test binary so it can install a tracing subscriber wired to
//! a real OpenTelemetry tracer without contending with other tests for the
//! process-global subscriber.

use opentelemetry::trace::TracerProvider as _;
use serde_json::json;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn start_workflow_injects_w3c_traceparent_when_a_span_is_active() {
    // W3C propagator + a real tracer (no exporter needed — the default
    // AlwaysOn sampler still mints valid, sampled span contexts).
    opentelemetry::global::set_text_map_propagator(
        opentelemetry_sdk::propagation::TraceContextPropagator::new(),
    );
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
    let tracer = provider.tracer("trace-propagation-test");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    // Thread-local default: `#[tokio::test]` runs on the current thread, so the
    // instrumented future below is polled under this subscriber.
    let _guard = tracing::subscriber::set_default(subscriber);

    let server = MockServer::start().await;
    // The mock only matches when `traceparent` is present — so a missing header
    // yields a 404, `start_workflow` returns `Rejected`, and the `unwrap` below
    // fails the test. `expect(1)` is verified on server drop.
    Mock::given(method("POST"))
        .and(path("/Archives/2026-06-14/run"))
        .and(header_exists("traceparent"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .expect(1)
        .mount(&server)
        .await;

    async {
        workflows::start_workflow(
            &server.uri(),
            None,
            "Archives",
            "2026-06-14",
            "run",
            &json!({}),
            false,
        )
        .await
        .expect("trigger POST should be accepted (and carry traceparent)");
    }
    .instrument(tracing::info_span!("test.caller"))
    .await;
}
