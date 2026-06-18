---
name: rust-service-lifecycle
description: >
  Service startup, shutdown, health, and readiness for long-running Rust binaries in the workspace (primarily `web`).
  Trigger when wiring `main()` for a binary, adding `/health` or `/readyz`, handling `SIGTERM`/`SIGINT`, draining
  in-flight requests, or sequencing dependency initialization. Also trigger before adding ad-hoc `ctrl_c` handlers —
  there's one canonical shutdown signal helper.
---

# Service lifecycle for navigator binaries

A Rust binary in this workspace is a Kubernetes citizen: it gets a `SIGTERM` from the kubelet on rollout, has a deadline (`terminationGracePeriodSeconds`, default 30s) to drain, and its readiness probe gates traffic during and after that drain. The lifecycle code below is what makes the binary survive rollouts cleanly.

## `main()` shape

```text
1. Init tracing (fmt + OTel if OTEL_EXPORTER_OTLP_ENDPOINT is set)
2. Load config from env (one function: Config::from_env() → Result<Config, ConfigError>)
3. Open the database; run migrations; abort startup if either fails
4. Construct AppState (db, opa client, oauth client, …)
5. Build axum Router; attach middleware
6. Bind TcpListener on PORT
7. axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await
8. Flush OTel exporter; drop tracing guards; return Ok(())
```

Each step is one function in `web::boot`. Tests can drive steps 1–5 without binding a port.

## Shutdown signal

One shared helper, listens for both SIGTERM (kubelet, systemd) and SIGINT (Ctrl-C in dev):

```rust
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("ctrl_c handler");
    };
    let term = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
```

`with_graceful_shutdown` stops accepting new connections and waits for in-flight handlers to finish. Hold per-request resources (DB transactions, OPA calls) under `tokio::time::timeout` so a slow upstream can't outrun the grace period.

## Health vs readiness

| Endpoint | Purpose | Should fail when |
|---|---|---|
| `GET /health` (liveness) | "Process is alive" — kubelet restarts the pod on repeated failure. | Process is wedged. Almost always `200 OK`. **Do not** check downstreams here — a DB blip will restart-loop the pod. |
| `GET /readyz` (readiness) | "Ready to take traffic" — kubelet removes the pod from Service endpoints on failure. | DB round-trip fails; OIDC discovery doc unfetched; mandatory dependency unreachable. |

`/health` returns `200` until the shutdown signal fires; after that, return `503` so the readiness probe drains pods before SIGTERM lands.

## Init order

Initialize in dependency order, fail fast:

1. **Tracing first** — every later log line wants it.
2. **Config** — abort with a clear error if a required env var is missing.
3. **Database** — open, then `Migrator::up(&db, None).await?`. A failed migration is a fatal startup error, not a runtime warning.
4. **External clients** — `reqwest::Client` for OPA + OIDC, `oauth2::BasicClient`, `google-cloud-storage::Client`. Construct once, share via `Arc`.
5. **OIDC discovery** — fetch the discovery doc with a bounded retry. If the IdP is down at startup, log + continue with a deferred refresh; don't crash-loop.
6. **Server bind** — last. By the time the listener is bound, every dependency is ready.

## Background tasks

- Spawn with a `JoinSet` owned by `main`, so shutdown cancels them in one `set.shutdown().await`.
- Long-lived loops listen on a `tokio::sync::watch::Receiver<bool>` for the shutdown signal and break cleanly. Aborting mid-iteration is a last resort.
- Periodic work (token refresh, OPA bundle reload) lives in its own task with `tokio::time::interval`, never `loop { sleep(); }` (which drifts).

## OpenTelemetry

- Construct the tracer in `main`, hold the guard until after `axum::serve` returns. Dropping the guard inside a `tokio::spawn` cancels in-flight span exports.
- `OTEL_EXPORTER_OTLP_ENDPOINT` unset → fmt-only logging, zero OTel cost.
- Don't propagate panics through `tracing` filters — `RUST_LOG=…` always wins over hardcoded defaults.

## Kubernetes probe wiring

```yaml
livenessProbe:
  httpGet: { path: /health, port: http }
  initialDelaySeconds: 5
  periodSeconds: 10
  failureThreshold: 3
readinessProbe:
  httpGet: { path: /readyz, port: http }
  initialDelaySeconds: 2
  periodSeconds: 5
  failureThreshold: 2
terminationGracePeriodSeconds: 30
```

The 30-second grace window is the upper bound for `with_graceful_shutdown`. If a handler takes longer than that, it gets killed by SIGKILL — wrap external calls in `timeout(Duration::from_secs(20), …)` to leave 10 seconds of slack.

## Anti-patterns

- `ctrl_c().await.unwrap()` inline in `main` — write the canonical helper once and call it.
- Reading config inside handlers — config belongs in `AppState`, decoded once at boot.
- A readiness probe that always returns `200` — defeats Kubernetes' rolling-update safety.
- `tokio::spawn` without an owner; on shutdown those tasks get orphaned and miss cancellation.

## Canonical sources

- Tokio `signal` module: <https://docs.rs/tokio/latest/tokio/signal/>
- Axum graceful shutdown example: <https://github.com/tokio-rs/axum/tree/main/examples/graceful-shutdown>
- Kubernetes pod lifecycle: <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/>
- Kubernetes liveness/readiness/startup probes: <https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/>
- CNCF — graceful shutdown reference (production readiness): <https://www.cncf.io/blog/>
- OpenTelemetry Rust SDK: <https://github.com/open-telemetry/opentelemetry-rust>
- OpenTelemetry semantic conventions (HTTP): <https://opentelemetry.io/docs/specs/semconv/http/>
