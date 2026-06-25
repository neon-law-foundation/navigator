# Observability

How Neon Law Navigator emits telemetry, where it lands for analysis, and how an operator debugs a durable-execution
failure fast. Born from an incident: a trigger Job sat in `ImagePullBackOff` for days while a `CronJob`'s
`concurrencyPolicy: Forbid` silently skipped every run, and *nothing emitted a queryable signal* — the only telemetry
was the nightly email, which was the thing that broke. This page exists so that never repeats.

> **The one rule for anyone adding a span, metric, or log field — identifiers and counts, never content.** A
  `notation_id`, a `service` name, an `outcome`, a duration, a status code: yes. A client name, an answer body, an email
  address, a document body: never. Telemetry crosses the firm's trust boundary; client content does not. This is a
  standing engineering- and legal-council order, not a style preference.

## One seam: `telemetry::init`

Every binary calls [`telemetry::init`](../telemetry/src/lib.rs) once in `main` and holds the returned guard until exit.
There is no per-binary subscriber wiring anymore — web, the `workflows-service` worker, and all six `*-trigger` jobs
share the one crate. Two modes, chosen by whether `OTEL_EXPORTER_OTLP_ENDPOINT` is set:

| | Unset (dev / CI / OSS fork) | Set (prod) |
| --- | --- | --- |
| stdout | human-readable `fmt` | **structured JSON** (Cloud Logging parses every field) |
| traces | — | OTLP → collector |
| metrics | — | OTLP → collector |
| cost | zero — no network | one batch span + periodic metric push |

Standard `OTEL_*` env vars (`OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`) drive everything; there is no
product-specific telemetry config. The guard's drop flushes batched spans/metrics — important for the short-lived
trigger jobs, which would otherwise exit before the periodic exporter fires.

## What is instrumented

Every workflow trigger funnels through `workflows::start_workflow`, instrumented once there so every trigger inherits
it:

- a span `workflow.trigger` with `service` / `key` / `handler` — never the request body; the metric
  **`navigator.workflow.trigger.fired`**, dimensioned by `service` and `outcome` ∈ {`accepted`, `rejected`,
  `transport_error`}. A flat line for a service that should fire on a schedule is the signal a trigger has silently
  stopped — the exact failure that hid for days;
- a structured event on each outcome (`status`, `service`) so a 401 / 404 / timeout is one log line, not a guess.

The worker and web emit their own spans through the same subscriber, so new handlers inherit tracing for free.

## Where it lands: BigQuery + the GCP consoles

Two doors, one of them is BigQuery on day one:

- **Logs → BigQuery (dual-path).** Logs now export over OTLP to the Collector *and* still write structured JSON to
  stdout. Pod stdout is collected by **Cloud Logging**; a **Logging sink to a BigQuery dataset**
  (`examples/deploy/k8s/observability/README.md`) lands every structured log — including the trigger-outcome events with
  their `service` / `status` / trace ids — in a queryable table. The stdout leg is the failure-isolation guarantee: it
  survives a collector outage (stdout always reaches Cloud Logging), so an outage is "no live traces / no lake
  telemetry," never "lost a log line."
- **Traces + metrics + logs → Cloud Trace / Cloud Monitoring / Cloud Logging.** All three signals speak OTLP to an
  **OpenTelemetry Collector** (`examples/deploy/k8s/observability/otel-collector.yaml`) that fans out to Google Cloud.
  Swapping backends never touches Rust — only the Collector config.
- **The Collector is the privilege choke point.** A **fail-closed redaction processor** (`allow_all_keys: false`)
  deletes every attribute whose key is not on an explicit operational allow-list, so the "identifiers and counts, never
  content" rule is enforced structurally, not by convention. See the overlay README for the allow-list and what is
  dropped by construction (resolved URL paths, SQL, exception payloads, any free-text).

This complements the existing Iceberg archive ([iceberg-archive guide](iceberg-archive.md)): the nightly `Archives`
workflow snapshots Postgres *tables* to Parquet on GCS for BigQuery external-table analysis. Telemetry (logs, traces,
metrics) is the *operational* half; the Iceberg archive is the *data* half. Both query from BigQuery.

## Debugging "the workflow didn't run"

Work down the chain (full version in the [durable-workflows guide](durable-workflows.md)); each rung now has telemetry:

1. **Did the trigger fire — and is a job wedged?** Run **`navigator doctor`**. It reads the cluster and names, in plain
   language, any trigger Job stuck in `ImagePullBackOff` / `CrashLoopBackOff` or Active too long (which, under `Forbid`,
   skips every subsequent run) and any unready workload — each with the exact `kubectl` command that fixes it. First
   stop for a missing nightly/periodic job.
2. **Did the ingress accept it?** Query BigQuery for `navigator.workflow.trigger.fired` by `service` and `outcome`, or
   read the trigger-outcome log events: `rejected` with `status=401` is a stale `RESTATE_AUTH_TOKEN`; `status=404` is
   the registration gotcha; `transport_error` is an unreachable/hung ingress (now capped by a 30s client timeout +
   `activeDeadlineSeconds`).
3. **Did the worker run it?** The Restate Cloud console → Invocations shows the journal; a failing step retries and
   surfaces there. The `Heartbeat` and `Archives` emails both deep-link it (`RESTATE_CLOUD_CONSOLE_URL`).
4. **Is durable execution alive at all?** The six-hourly `Heartbeat` email is the liveness signal; its *absence* is the
   alert.

## Tracing across the Restate boundary

A workflow kicked off from `web` continues the caller's trace into the durable handler, so a single trace spans "button
click → ingress POST → snapshot/dispatch steps." `workflows::trigger` injects the current span's W3C `traceparent` into
the outbound ingress POST (`telemetry::current_trace_context_headers`); each handler extracts it from `ctx.headers()`
and parents its span on the result (`telemetry::set_span_parent`, used by `Archives::run` and every `Notation` handler).
Only opaque trace context crosses — never a client field (LEGAL #2). When OTLP is unconfigured the inject/extract pair
is a no-op, so dev / KIND / OSS forks stay zero-cost.

The Rust contract — inject produces a well-formed `traceparent`, extract recovers the same trace id — is covered by
`telemetry`'s round-trip test and `workflows`' `trace_propagation` integration test. The **one** thing only a live
cluster confirms is that Restate forwards the ingress `traceparent` onto the handler invocation headers; verify once in
KIND/prod by checking a `web`-initiated workflow and its steps share a trace id in Cloud Trace. If a future Restate
version stops forwarding it, the fallback is to carry a `trace_id` in the request body and link (rather than parent) the
handler span — no other code changes.

## The hardening that came with this

- **HTTP timeout** in `start_workflow` (30s) so a hung ingress can't keep a trigger pod running forever.
  **`activeDeadlineSeconds: 120` + `startingDeadlineSeconds`** on the trigger `CronJob`s, so a stuck trigger
  self-terminates instead of holding the `Forbid` lock and skipping every run — the precise failure mode that stopped
  the nightly archives email for days.
- **`navigator doctor`** so the next operator sees the wedge in one command instead of `kubectl` archaeology.

## See also

- The [durable-workflows guide](durable-workflows.md) — the durable-execution model and the registration gotcha. The
  [Iceberg archive guide](iceberg-archive.md) — the nightly Postgres → Parquet → BigQuery table archive.
  `examples/deploy/k8s/observability/` — the OTel Collector + the Cloud Logging → BigQuery sink.
  [`cloud-operations.md`](cloud-operations.md) — the author-facing recipe (leads with the no-content rule).
