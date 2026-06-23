---
name: restate-rust
description: >
  How to AUTHOR a Restate handler in Rust (`restate-sdk` 0.10) without breaking durability — the determinism and
  error-handling contract that makes `workflows-service` replay-safe. Distilled from Restate's own
  `building-restate-services` skill (https://github.com/restatedev/skills), retargeted from its TS/Python/Java/Go
  examples to this workspace's Rust SDK and grounded in `workflows-service/src` (the only `restate-sdk` consumer). THE
  ONE RULE, load-bearing: every non-deterministic act — clock (`Utc::now`), randomness, UUIDs, a DB write, object
  storage, any network/IO call — must happen INSIDE `ctx.run(...)`, which journals the outcome so a replay reuses the
  recorded value instead of re-executing. Bare non-determinism in handler body = a replay that diverges from the journal
  = corrupted durable state. Trigger when adding or editing a handler in `workflows-service`, reaching for
  `#[restate_sdk::object]` / `#[restate_sdk::workflow]` / `#[restate_sdk::service]`, calling `ctx.run` / `ctx.get` /
  `ctx.set` / `ctx.sleep` / an awakeable, choosing between `TerminalError` and a retryable error, wiring a new service
  into `main.rs`'s `Endpoint::builder().bind(...)`, or porting orchestration logic into a durable handler. To ADD a
  whole legal flow (template + questionnaire + workflow YAML) use create-legal-workflow; to OPERATE/diagnose the
  already-running engine use durable-execution; this skill is the Rust SDK authoring rules that sit under both. Skip for
  non-Restate Rust (use rust-best-practices / rust-concurrency) and for the submit side (`workflows` lib, which only
  POSTs to the ingress and never binds the SDK).
---

# restate-rust

Restate is a durable execution runtime: it **journals every step** a handler takes, and on crash or retry it **replays
the handler from the start**, returning each already-journaled step's recorded result instead of re-executing it, until
it reaches the first step that never completed. That replay is the whole point — it is how a filing or a signature flow
survives a pod restart mid-flight. It is also the source of the one way to corrupt everything: if a replayed step
produces a *different* value than the journal recorded, durable state diverges.

This skill is the Rust (`restate-sdk` 0.10) authoring contract that keeps replay sound. The operational side (keeping
the running engine alive, diagnosing a workflow that "didn't fire") is [[durable-execution]]; adding a whole legal
matter type is [[create-legal-workflow]]. Deep architecture:
[`docs/durable-workflows.md`](../../../docs/durable-workflows.md).

## THE ONE RULE — wrap every non-deterministic act in `ctx.run`

A handler body re-runs verbatim on every replay. So anything whose value could differ between the first run and a replay
must be journaled. Read the clock, generate a UUID or random number, call the database, write object storage, hit a
third-party API — all of it goes **inside** `ctx.run(...)`, never in the bare handler body. `ctx.run` executes the
closure once, journals the returned value, and on replay hands back the journaled value without re-running the closure.

`workflows-service/src/heartbeat.rs` is the canonical illustration — even reading the wall clock is journaled:

```rust
// CORRECT — the instant is captured inside ctx.run, so a replay reuses
// the recorded time instead of re-reading the clock.
let report: HeartbeatReport = ctx
    .run(|| async {
        Ok(Json(HeartbeatReport {
            invocation_id: invocation_id.clone(),
            beat_at: Utc::now(), // non-deterministic — MUST be journaled
        }))
    })
    .name("beat")
    .await?
    .into_inner();
```

```rust
// WRONG — Utc::now() in the bare body yields a new instant on every
// replay; the journal and the live value diverge.
let beat_at = Utc::now();
ctx.set(BEAT_KEY, beat_at.to_rfc3339());
```

Corollaries:

- **One side effect per `ctx.run`, each `.name("…")`-tagged.** Split distinct effects (e.g. a DB write and an email)
  into separate journaled steps so a retry of one never re-runs the other — see heartbeat's `"beat"` then `"notify"`.
- **No `ctx` operations inside a `ctx.run` closure.** The closure is opaque to the journal; `ctx.get` / `ctx.set` /
  another `ctx.run` belong in the handler body, between the journaled steps.
- **`ctx.set` already journaled — don't double-wrap.** State writes go straight in the body; only the *computation of
  the value* (if non-deterministic) needs a preceding `ctx.run`.

## The handler shapes

Three macros on a trait, implemented by a service struct, bound in `main.rs`. Pick by concurrency need:

- `#[restate_sdk::service]` + `Context<'_>` — stateless, no key. Plain durable RPC.
- `#[restate_sdk::object]` + `ObjectContext<'_>` (exclusive write) / `SharedObjectContext<'_>` (concurrent read-only) —
  a **virtual object**: keyed, single-writer state. Restate serializes writes per key, so per-key handlers never race.
  `notation_service.rs` keys on the notation id.
- `#[restate_sdk::workflow]` + `WorkflowContext<'_>` — a run-once workflow with a result. `heartbeat.rs`.

```rust
#[restate_sdk::object]
#[name = "notation"]            // the registered service name (PascalCase for services/workflows; objects lowercase)
pub trait Notation {
    async fn questionnaire_signal(body: Json<SignalBody>) -> Result<Json<SignalResponse>, HandlerError>;
}
```

State on a keyed context: `ctx.get::<String>(KEY).await?` (returns `Option`), `ctx.set(KEY, value)`, plus `ctx.key()`,
`ctx.headers()`, `ctx.invocation_id()`. Every handler returns `Result<…, HandlerError>` and almost always wraps its
payload in `Json<T>`.

Wire a new service into the one worker endpoint in `main.rs` and record its name in `registry.rs` (a test guards the two
against drift):

```rust
Endpoint::builder()
    .bind(NotationService::new(db.clone(), email.clone(), storage).serve())
    .bind(HeartbeatService::new(ops_delivery.clone()).serve())
    .build()
```

## Errors — terminal vs retryable

The error type decides whether Restate **retries**. This is the second contract after determinism:

- **Retryable (default):** any `HandlerError` that is *not* terminal — a transient DB blip, a 503 from a peer. Restate
  retries the invocation with backoff, forever, until it succeeds. Use this for anything that might succeed later.
- **Terminal:** `TerminalError::new("…")` (wrap as `HandlerError::from(TerminalError::new(...))` where the signature
  needs `HandlerError`). Restate stops retrying and fails the invocation. Use for un-fixable input — a malformed key, a
  validation failure, a precondition that can never become true:

```rust
let notation_id = uuid::Uuid::parse_str(ctx.key())
    .map_err(|e| TerminalError::new(format!("invalid notation key: {e}")))?;
```

Getting this backwards is a real outage: a validation bug raised as a *retryable* error wedges the engine retrying a
doomed invocation forever. When in doubt about whether a failure is recoverable, prefer terminal for bad input and
retryable for infrastructure.

## Concurrency — Restate combinators only

Inside a handler, **do not** reach for `tokio::spawn`, `tokio::join!`, `futures::join_all`, or channels to run journaled
steps concurrently — native concurrency is invisible to the journal and races replay. If you need to await multiple
durable operations, use the SDK's own combinators / select, or sequence the `ctx.run` steps. (The workspace standardizes
on Tokio everywhere else — that's [[rust-concurrency]]; this restriction is *only* about journaled steps inside a
Restate handler.) Durable timers use `ctx.sleep(duration).await?`, never `tokio::time::sleep`; cross-handler rendezvous
uses awakeables, never an in-process `oneshot`.

## Observability inside a handler

Span/log identifiers and counts, **never client content** — same rule as everywhere ([[observability]]). Handlers here
already do this: `traced_handler_span(name, ctx.headers(), ctx.key())` carries the handler name and the key, not the
questionnaire answers. A `notation_id`, service name, state name, or invocation id is fine; an answer body, email
address, or document body must never enter a span, log, or `ctx.set` debug line.

## Verify by replay, not by "it compiled"

A handler that compiles can still be non-deterministic — the bug only shows on replay. Per the workspace rule (no
assumptions; always test what you changed), any change to handler business logic needs a covering test that exercises
the **replay** path, not just a single happy run. The Rust SDK drives this with testcontainers against a real Restate
server (the side-effecting steps need a real Postgres too — see `workflows/tests/onchain_dispatch.rs`). A green compile
plus a green non-replay test is *not* evidence the step is deterministic; force the replay.

## Attribution

The determinism / terminal-error / combinator / replay-test rules are Restate's own, from the upstream
`building-restate-services` skill at <https://github.com/restatedev/skills> (TS/Python/Java/Go). This file retargets
them to `restate-sdk` 0.10 Rust and this workspace; the upstream repo also ships a `restate-docs` MCP server worth
adding if you want live access to the conceptual guides.
