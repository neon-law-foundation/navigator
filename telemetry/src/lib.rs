#![allow(clippy::doc_markdown)]
//! The one observability seam for every Navigator binary.
//!
//! [`init`] wires `tracing` once and returns a [`TelemetryGuard`] whose drop
//! flushes any pending OpenTelemetry export. Every `main` calls it with its
//! service name; nothing else hand-rolls a subscriber.
//!
//! Two modes, chosen by whether `OTEL_EXPORTER_OTLP_ENDPOINT` is set:
//!
//! - **Unset (dev / CI / OSS fork)** — a human-readable `fmt` layer to stdout
//!   and nothing else. Zero OTel cost, no network.
//! - **Set (prod)** — stdout switches to **structured JSON** (so the pod's
//!   logs parse cleanly through Cloud Logging into the BigQuery sink), and the
//!   process additionally exports **traces, metrics, and logs** over OTLP to
//!   the configured collector. The stdout JSON layer stays on in this mode
//!   too: logs **dual-emit** to stdout *and* OTLP, so a collector outage
//!   degrades to "no live traces / no lake telemetry," never "lost a log
//!   line." Standard `OTEL_*` env vars drive everything; there is no
//!   Navigator-specific telemetry config.
//!
//! **The one rule for anyone adding a span, metric, or log field (legal- and
//! engineering-council standing order): identifiers and counts, never
//! content.** A `notation_id`, a `service` name, an `outcome`, a duration, a
//! status code — yes. A client name, an answer body, an email address, a
//! document body — never. This matters most for **logs**, where a free-text
//! message is the easy place for a client identifier to slip in: log a
//! `person_id`, never a person's name. Telemetry leaves the firm's trust
//! boundary; client content does not.

use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::logs::LoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// The instrumentation scope name for durable-execution metrics.
const TRIGGER_METER: &str = "navigator.workflow.trigger";

/// Counter: how many times a workflow trigger POSTed to the Restate ingress,
/// dimensioned by `service` and `outcome`. A flat line for a `service` that
/// should fire on a schedule is the signal that a trigger has silently stopped
/// — the exact failure that hid for days before this existed.
pub const TRIGGER_FIRED: &str = "navigator.workflow.trigger.fired";

/// Outcome label values for [`TRIGGER_FIRED`]. Status only — never content.
pub mod outcome {
    /// The ingress accepted the invocation (2xx).
    pub const ACCEPTED: &str = "accepted";
    /// The ingress answered but rejected it (e.g. 401 stale token, 404 service
    /// not registered).
    pub const REJECTED: &str = "rejected";
    /// The POST never got an answer (DNS, connect, or the 30s timeout).
    pub const TRANSPORT_ERROR: &str = "transport_error";
}

/// Flush-on-drop guard for the OTLP providers. Hold it for the lifetime of
/// `main`; dropping it (or calling [`TelemetryGuard::shutdown`]) exports any
/// batched spans/metrics/logs before the process exits.
#[must_use = "dropping the guard immediately flushes and tears down telemetry"]
pub struct TelemetryGuard {
    tracer: Option<TracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<LoggerProvider>,
}

impl TelemetryGuard {
    /// Explicitly flush and tear down. Equivalent to dropping the guard; offered
    /// so a `main` can shut telemetry down ahead of other cleanup and read as
    /// intentional.
    pub fn shutdown(self) {}
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(p) = self.tracer.take() {
            let _ = p.shutdown();
        }
        if let Some(m) = self.meter.take() {
            let _ = m.shutdown();
        }
        if let Some(l) = self.logger.take() {
            let _ = l.shutdown();
        }
    }
}

/// The three OTLP providers built for the export (prod) path, sharing one
/// [`Resource`]. Kept as a struct so [`init`] and the unit tests construct them
/// the same way — the tests exercise this without touching the process-global
/// subscriber, which can only be installed once.
struct ExportProviders {
    tracer: TracerProvider,
    meter: SdkMeterProvider,
    logger: LoggerProvider,
}

/// Normalize the raw `OTEL_EXPORTER_OTLP_ENDPOINT` value: an unset, empty, or
/// whitespace-only endpoint means "do not export" and yields `None`. Factored
/// out so the dev/prod branch decision is unit-testable without mutating
/// process env.
fn normalize_endpoint(raw: Option<String>) -> Option<String> {
    raw.filter(|v| !v.trim().is_empty())
}

/// Build the trace / metric / log OTLP providers for `endpoint`, all sharing a
/// single [`Resource`] (DRY: one resource, three providers — never three
/// resources that can drift). Building an exporter does **not** open a
/// connection — tonic connects lazily on first export — so this is safe to call
/// offline (and the unit tests do exactly that).
fn build_export_providers(endpoint: &str, service_name: &str) -> ExportProviders {
    let resource = Resource::new(vec![KeyValue::new(
        "service.name",
        service_name.to_string(),
    )]);

    // Traces — one batch span exporter.
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP span exporter");
    let tracer = TracerProvider::builder()
        .with_batch_exporter(span_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource.clone())
        .build();

    // Metrics — periodic OTLP push.
    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP metric exporter");
    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(
        metric_exporter,
        opentelemetry_sdk::runtime::Tokio,
    )
    .build();
    let meter = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource.clone())
        .build();

    // Logs — batch OTLP push, bridged from `tracing` (see [`init`]). The same
    // resource binds all three signals to one `service.name`.
    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP log exporter");
    let logger = LoggerProvider::builder()
        .with_batch_exporter(log_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build();

    ExportProviders {
        tracer,
        meter,
        logger,
    }
}

/// Initialize the global `tracing` subscriber and, when configured, OTLP
/// export. Call exactly once per process, early in `main`.
pub fn init(default_service_name: &str) -> TelemetryGuard {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let endpoint = normalize_endpoint(std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok());

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_service_name.to_string());

    // JSON to stdout when exporting (prod) so Cloud Logging -> BigQuery parses
    // each field; human-readable otherwise. Boxed so both arms share one type.
    let fmt_layer = if endpoint.is_some() {
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    let Some(endpoint) = endpoint else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
        return TelemetryGuard {
            tracer: None,
            meter: None,
            logger: None,
        };
    };

    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let ExportProviders {
        tracer,
        meter,
        logger,
    } = build_export_providers(&endpoint, &service_name);

    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(tracer.tracer(service_name));

    // Register the meter provider globally so `record_trigger_fired` (and any
    // future instrument) reaches it.
    opentelemetry::global::set_meter_provider(meter.clone());

    // Bridge `tracing` log records to the OTLP logger. This is the third layer
    // alongside the stdout fmt layer — logs **dual-emit** (stdout JSON *and*
    // OTLP), so a collector outage never drops a log line.
    let otel_log_layer = OpenTelemetryTracingBridge::new(&logger);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();

    TelemetryGuard {
        tracer: Some(tracer),
        meter: Some(meter),
        logger: Some(logger),
    }
}

/// Record one workflow-trigger fire. Safe to call unconditionally: when OTLP is
/// not configured the global meter is a no-op, so this costs nothing in dev.
/// `service` is the Restate service name (e.g. `Archives`); `outcome` is one of
/// the [`outcome`] constants. Identifiers and counts only — never content.
pub fn record_trigger_fired(service: &str, outcome: &str) {
    let counter = opentelemetry::global::meter(TRIGGER_METER)
        .u64_counter(TRIGGER_FIRED)
        .build();
    counter.add(
        1,
        &[
            KeyValue::new("service", service.to_string()),
            KeyValue::new("outcome", outcome.to_string()),
        ],
    );
}

/// The instrumentation scope name for the `/mcp` tool-call metric.
const MCP_METER: &str = "navigator.mcp";

/// Counter: how many times a tool was invoked over the `/mcp` JSON-RPC surface,
/// dimensioned by `tool` and `outcome`. The A2A surface already audits its tool
/// calls; this is the matching signal for the *direct* `/mcp` callers (Claude.ai
/// Connectors, Claude Code, Cursor, LibreChat) so neither protocol surface that
/// shares the one tool catalog is blind in prod.
pub const MCP_TOOL_CALLED: &str = "navigator.mcp.tool.called";

/// Outcome label values for [`MCP_TOOL_CALLED`]. Status only — never the
/// arguments a client passed nor the tool's result body.
pub mod mcp_outcome {
    /// The tool ran and returned a result.
    pub const OK: &str = "ok";
    /// The tool returned a `ToolError` (rendered to the caller as an `isError`
    /// result per MCP convention).
    pub const ERROR: &str = "error";
}

/// Record one `/mcp` tool invocation. Safe to call unconditionally: when OTLP is
/// not configured the global meter is a no-op, so this costs nothing in dev.
/// `tool` is the namespaced tool name (e.g. `aida_create_person`); `outcome` is
/// one of the [`mcp_outcome`] constants. Identifiers and counts only — the tool
/// name and the outcome enum, never the arguments or the result.
pub fn record_mcp_tool_called(tool: &str, outcome: &str) {
    let counter = opentelemetry::global::meter(MCP_METER)
        .u64_counter(MCP_TOOL_CALLED)
        .build();
    counter.add(
        1,
        &[
            KeyValue::new("tool", tool.to_string()),
            KeyValue::new("outcome", outcome.to_string()),
        ],
    );
}

// ---------------------------------------------------------------------------
// Cross-service trace propagation (W3C `traceparent`).
//
// The one place the inject/extract pair lives, so every boundary crossing
// speaks the same wire format: `workflows::trigger` injects on the outbound
// POST to the Restate ingress; the `Archives` / `Notation` handlers extract
// from `ctx.headers()` and parent their spans on the result, so a trace begun
// in `web` continues through the durable workflow. The helpers take a plain
// `opentelemetry::Context` and `&str` header values — never reqwest's
// `HeaderMap<HeaderValue>` nor the Restate SDK's `HeaderMap<String>` — so both
// sides reuse them without type coupling.
//
// LEGAL (#2): only trace context crosses here — `traceparent` is
// `version-traceid-spanid-flags`, all opaque. Never put a client field in
// baggage or a propagated header.
// ---------------------------------------------------------------------------

/// Collects the propagator's injected headers into name/value pairs for a
/// caller to attach to its outbound request.
struct HeaderCollector(Vec<(String, String)>);

impl Injector for HeaderCollector {
    fn set(&mut self, key: &str, value: String) {
        self.0.push((key.to_string(), value));
    }
}

/// Extracts trace context from a fixed `traceparent` / `tracestate` pair — the
/// only two headers `TraceContextPropagator` reads.
struct PairExtractor<'a> {
    traceparent: Option<&'a str>,
    tracestate: Option<&'a str>,
}

impl Extractor for PairExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.traceparent,
            "tracestate" => self.tracestate,
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        ["traceparent", "tracestate"]
            .into_iter()
            .filter(|k| self.get(k).is_some())
            .collect()
    }
}

/// Inject the W3C trace context of `cx` into HTTP header name/value pairs (the
/// outbound side of cross-service tracing). Returns the propagation headers —
/// typically `traceparent`, plus `tracestate` when present — for the caller to
/// attach to its request. Empty when no sampled span is active or OTLP is
/// unconfigured (the global propagator is then a no-op), so tracing degrades
/// gracefully: the caller simply attaches nothing.
#[must_use]
pub fn trace_context_headers(cx: &opentelemetry::Context) -> Vec<(String, String)> {
    let mut collector = HeaderCollector(Vec::new());
    opentelemetry::global::get_text_map_propagator(|p| p.inject_context(cx, &mut collector));
    collector.0
}

/// Inject the *current* tracing span's trace context — the common call site
/// (the caller is inside an instrumented span). Convenience wrapper over
/// [`trace_context_headers`].
#[must_use]
pub fn current_trace_context_headers() -> Vec<(String, String)> {
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    trace_context_headers(&tracing::Span::current().context())
}

/// Rebuild the parent [`opentelemetry::Context`] from the W3C trace headers a
/// handler received (the receiving side). Pass the incoming `traceparent` and
/// `tracestate` header values. Attach the result to a span with
/// `tracing_opentelemetry::OpenTelemetrySpanExt::set_parent` so the handler's
/// spans join the caller's trace. Returns an empty context (a fresh root) when
/// no `traceparent` is present.
#[must_use]
pub fn parent_context_from(
    traceparent: Option<&str>,
    tracestate: Option<&str>,
) -> opentelemetry::Context {
    let extractor = PairExtractor {
        traceparent,
        tracestate,
    };
    opentelemetry::global::get_text_map_propagator(|p| p.extract(&extractor))
}

/// Parent `span` on the trace context carried by a handler's incoming
/// `traceparent` / `tracestate` headers, so the span and its children join the
/// caller's trace across the Restate boundary. The receiving-side convenience
/// over [`parent_context_from`] — it keeps the `tracing-opentelemetry`
/// dependency in this one crate instead of every workflow handler. A no-op
/// (fresh root) when no `traceparent` is present.
pub fn set_span_parent(span: &tracing::Span, traceparent: Option<&str>, tracestate: Option<&str>) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    span.set_parent(parent_context_from(traceparent, tracestate));
}

#[cfg(test)]
mod tests {
    use super::{
        build_export_providers, current_trace_context_headers, normalize_endpoint,
        parent_context_from, trace_context_headers,
    };

    #[test]
    fn normalize_endpoint_treats_unset_empty_and_blank_as_no_export() {
        assert_eq!(normalize_endpoint(None), None);
        assert_eq!(normalize_endpoint(Some(String::new())), None);
        assert_eq!(normalize_endpoint(Some("   ".to_string())), None);
    }

    #[test]
    fn normalize_endpoint_keeps_a_real_endpoint() {
        assert_eq!(
            normalize_endpoint(Some("http://otel-collector:4317".to_string())),
            Some("http://otel-collector:4317".to_string())
        );
    }

    /// Building the three providers must not open a connection (tonic connects
    /// lazily), so this constructs them against an unreachable endpoint and
    /// shuts them down — proving the export path wires logs alongside traces +
    /// metrics with no network.
    ///
    /// **Must run on a multi-thread runtime.** The batch span/log processors
    /// and the periodic metric reader each own a background flush task on the
    /// Tokio runtime; `shutdown()` blocks until that task acknowledges. On the
    /// default current-thread `#[tokio::test]` runtime the blocking shutdown
    /// starves the very task it waits on — a deadlock. Two worker threads let
    /// the flush task make progress while shutdown blocks.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn export_providers_build_all_three_signals_offline() {
        let providers = build_export_providers("http://127.0.0.1:4317", "telemetry-test");
        // All three signals are present; shutting down flushes (no-op here,
        // nothing batched) without panicking or requiring a live collector.
        let _ = providers.tracer.shutdown();
        let _ = providers.meter.shutdown();
        let _ = providers.logger.shutdown();
    }

    /// The cross-service propagation contract, fully offline: a known span
    /// context injects to a well-formed W3C `traceparent`, and extracting that
    /// header back yields a parent context with the SAME trace id. This is the
    /// invariant `workflows::trigger` (inject) and the `Archives` / `Notation`
    /// handlers (extract) depend on across the Restate boundary.
    #[test]
    fn trace_context_round_trips_through_w3c_headers() {
        use opentelemetry::trace::{
            SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
        };

        // Without an explicit propagator the global default is a no-op; set the
        // W3C propagator so inject/extract actually run.
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        let trace_id = TraceId::from_bytes([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ]);
        let span_id = SpanId::from_bytes([0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]);
        let sc = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        );
        let cx = opentelemetry::Context::new().with_remote_span_context(sc);

        let headers = trace_context_headers(&cx);
        let traceparent = headers
            .iter()
            .find(|(k, _)| k == "traceparent")
            .map(|(_, v)| v.as_str());
        assert!(traceparent.is_some(), "traceparent must be injected");
        let tp = traceparent.unwrap();
        // W3C shape: version-traceid-spanid-flags, and it carries our ids.
        assert!(tp.starts_with("00-"), "W3C version prefix: {tp}");
        assert!(
            tp.contains("0102030405060708090a0b0c0d0e0f10"),
            "carries the trace id: {tp}"
        );

        let parent = parent_context_from(traceparent, None);
        assert_eq!(
            parent.span().span_context().trace_id(),
            trace_id,
            "extracted parent must share the injected trace id"
        );
        assert!(
            parent.span().span_context().is_remote(),
            "extracted context is a remote parent"
        );
    }

    /// With no active span (and the no-op default propagator path), the current
    /// helper returns no headers — the graceful-degradation property that keeps
    /// dev/CI/OSS forks zero-cost and never attaches a malformed header.
    #[test]
    fn current_headers_empty_without_an_active_span() {
        assert!(current_trace_context_headers().is_empty());
    }
}
