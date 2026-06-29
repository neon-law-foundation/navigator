---
name: grafana-lgtm
description: >
  `navigator start-dev-server` stands up the KIND stack with Grafana LGTM (Loki logs + Tempo traces + Prometheus metrics
  + a bundled OTel Collector) so you can actually SEE the telemetry every Neon Law Navigator binary emits — it wires
  both in-cluster and host-side `web` to export to it and port-forwards Grafana to the host. Trigger when asked to "see
  traces locally", "test OTel output", "check logs/metrics/traces in Grafana", "is my span showing up", to debug a
  cross-service trace (`web` to Restate to a workflow handler), or to verify a new metric/span before it ships to prod.
  This is the LOCAL viewing loop; the emit-side seam (how to ADD a span/metric/log and the no-content rule) is the
  `observability` skill, and the prod landing (Cloud Trace / Cloud Logging / BigQuery) lives there too.
---

# Seeing telemetry locally with Grafana LGTM

Emitting OpenTelemetry through `telemetry::init` is only half the loop. This skill is the other half: a local sink that
**shows** the traces, logs, and metrics so you can confirm they're shaped right before they ever reach prod's Cloud
Trace / Cloud Logging. **All the facts live in the doc** — read
[`docs/observability.md`](../../../docs/observability.md) (§ "Seeing telemetry locally: Grafana LGTM") and keep it, not
this skill, authoritative: the bring-up, the Grafana Explore queries per datasource, the disable-export switch, and the
wiring locations.

## How to use it (the load-bearing rules)

- **`navigator start-dev-server` does all the wiring — don't redo it by hand.** It applies the `lgtm` pod, port-forwards
  Grafana `:3000` + OTLP `:4317`, and writes `OTEL_EXPORTER_OTLP_ENDPOINT` into `.devx/env`; a hand-rolled forward of
  `svc/lgtm` just fights the one it already runs.
- **No endpoint, no export.** Host `web` only emits OTLP when `.devx/env` is sourced (it sets the OTLP endpoint, which
  flips `telemetry::init` out of stdout-only mode). Expecting telemetry without it is the usual "nothing shows up."
- **Verify the cross-service trace, not just the span.** The headline check is that a `web`-initiated workflow and its
  handler steps share **one** trace id in Tempo across the Restate boundary; a *new* trace id on the handler span means
  W3C propagation broke.
- **Identifiers and counts, never content** — even locally. The standing no-content rule is owned by the
  [[observability]] skill and doc; don't put a client name/answer/email in a span to "make it findable" in Grafana. Log
  the id, look the rest up.

## Boundaries

- The emit-side seam — how to ADD a span/metric/log field and the no-content rule — is [[observability]]; the prod
  landing (Cloud Trace / Cloud Logging / BigQuery) lives there too. LGTM is KIND-only.
- Standing up / tearing down the KIND cluster itself: [[kind-local-dev]]. Booting host `web` against the deps:
  [[web-preview]].
