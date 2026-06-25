---
name: grafana-lgtm
description: >
  The local OpenTelemetry loop — run the KIND stack with Grafana LGTM (Loki logs + Tempo traces + Prometheus metrics + a
  bundled OTel Collector) and actually SEE the telemetry every Neon Law Navigator binary emits. `navigator start-dev-server`
  stands LGTM up, wires both in-cluster and host-side `web` to export to it, and port-forwards Grafana to the host.
  Trigger when asked to "see
  traces locally", "test OTel output", "check logs/metrics/traces in Grafana", "is my span showing up", debug a
  cross-service trace (`web` → Restate → a workflow handler), or verify a new metric/span before it ships to prod. This
  is the LOCAL viewing loop; the emit-side seam (how to ADD a span/metric/log and the no-content rule) is the
  `observability` skill, and the prod landing (Cloud Trace / Cloud Logging / BigQuery) lives there too.
---

# Seeing telemetry locally with Grafana LGTM

Neon Law Navigator emits OpenTelemetry through one seam — `telemetry::init` in every service binary — but emitting is
only half the loop. This skill is the other half: a local sink that **shows** the traces, logs, and metrics so you can
confirm they're shaped right before they ever reach prod's Cloud Trace / Cloud Logging.

The sink is **Grafana LGTM** (the `grafana/otel-lgtm` one-process image): Grafana + **L**oki (logs) + **G**rafana +
**T**empo (traces) + Prometheus (**M**etrics, historically Mimir) + a bundled OTel Collector that receives OTLP and fans
it into the three stores. One pod, datasources auto-provisioned, no dashboards to import.

## The load-bearing rule still applies

> **Identifiers and counts, never content.** Even locally, a span attribute / metric label / log field carries a
  `notation_id`, a `service`, an `outcome`, a duration, a status code — never a client name, answer body, email address,
  or document body. LGTM is a debugging convenience, not a license to log content. See the `observability` skill — this
  is a standing engineering- and legal-council order.

## The loop

### 1. Bring it up (it's automatic)

```bash
cargo run --release -p cli -- start-dev-server
```

`navigator start-dev-server` does all the wiring — there is no manual port-forward or env export:

- Applies the `lgtm` Deployment + Service (`k8s/overlays/kind/deps/lgtm.yaml`, included from the `kind-deps`
  kustomization) and waits for its rollout.
- Port-forwards **Grafana 3000 → host `:3000`** and the **OTLP gRPC 4317 → host `:4317`**. Writes
  `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` into `.devx/env`.

Override the host ports with `NAVIGATOR_KIND_GRAFANA_PORT` / `NAVIGATOR_KIND_OTLP_PORT` if 3000/4317 clash.

### 2. Generate telemetry

- **Host `web`** (`cargo run -p web` after sourcing `.devx/env`, under Doppler — see [[web-preview]]): sourcing
  `.devx/env` sets `OTEL_EXPORTER_OTLP_ENDPOINT`, which is exactly what flips `telemetry::init` from human-readable
  stdout logs to **JSON logs + OTLP export**. Then drive a request (`http://localhost:3001/...`).
- **In-cluster `web` + `workflows-service`** already carry `OTEL_EXPORTER_OTLP_ENDPOINT` pointing at
  `http://lgtm.navigator.svc.cluster.local:4317` (baked into their manifests), so a cross-service trace stays intact:
  `web` injects `traceparent` on the POST to the Restate ingress, the workflow handler extracts it (see
  `telemetry::set_span_parent`), and both spans share one trace id in Tempo.

To exercise the durable-execution path, trigger a workflow (open a matter, run a `*-trigger` job) — the
`workflow.trigger` span + `navigator.workflow.trigger.fired` counter flow through.

### 3. Look at it in Grafana

Open **`http://localhost:3000`** (anonymous Admin, login form disabled — `navigator start-dev-server` prints the URL).
Then **Explore** (compass icon) and pick a datasource:

- **Traces → Tempo**: Search → `Service Name = navigator-web` (or `navigator-workflows-service`), then click a trace to
  see the span tree spanning the Restate boundary.
- **Logs → Loki**: `{service_name="navigator-web"}`, then filter further by a field (e.g. add a `status` selector for
  the HTTP code).
- **Metrics → Prometheus**: query `navigator_workflow_trigger_fired_total` or `navigator_mcp_tool_called_total` — OTLP
  dots become `_` and counters pick up a `_total` suffix.

The two counters worth knowing (from `telemetry/src/lib.rs`): `navigator.workflow.trigger.fired{service,outcome}` (a
flat line for a service that should fire on a schedule = a silently-stopped trigger) and
`navigator.mcp.tool.called{tool,outcome}` (the `/mcp` surface). A trace begun in `web` that continues into a workflow
handler is the headline thing to verify here — if the workflow span shows a *new* trace id instead of joining `web`'s,
W3C propagation is broken.

### 4. Tear down

```bash
cargo run --release -p cli -- down
```

Storage is `emptyDir` — tearing down (or restarting the pod) wipes all telemetry. That's the right default for KIND.

## Disabling export

To run host `web` with plain human-readable stdout logs and no OTLP (the pre-LGTM behavior), set
`OTEL_EXPORTER_OTLP_ENDPOINT=` (empty) in `.env` — it loads before `.devx/env` and wins, and `telemetry::init` treats an
empty/blank endpoint as "do not export."

## Where the wiring lives

- `k8s/overlays/kind/deps/lgtm.yaml` — the LGTM Deployment + Service (OTLP gRPC 4317, OTLP HTTP 4318, Grafana 3000).
  `k8s/overlays/kind/deps/kustomization.yaml` — includes `lgtm.yaml`, so both the `kind` and `kind-deps` overlays apply
  it.
- `cli/src/devx/mod.rs` — `up()` port-forwards `svc/lgtm`, waits for its rollout, and `render_env` writes the OTLP
  endpoint into `.devx/env`; `NAVIGATOR_KIND_GRAFANA_PORT` / `NAVIGATOR_KIND_OTLP_PORT` are the host-port knobs.
- `k8s/base/web/web.yaml` + `k8s/overlays/kind/workflows-service/workflows-service.yaml` — the in-cluster
  `OTEL_EXPORTER_OTLP_ENDPOINT` pointing at the cluster-internal `lgtm` Service.

## Anti-patterns

- Hand-rolling a `kubectl port-forward deploy/lgtm 4317:4317` — `navigator start-dev-server` already does it; a second
  forward fights it.
- Expecting telemetry without sourcing `.devx/env` — no endpoint means `telemetry::init` stays stdout-only by design.
  Putting client content in a span/log to "make it easier to find" in Grafana — never. Log the id, look the rest up.
  Treating prod the same way — prod exports to Cloud Trace / Cloud Logging via the real OTel Collector (`observability`
  skill); LGTM is KIND-only.
