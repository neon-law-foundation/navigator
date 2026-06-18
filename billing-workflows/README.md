# billing-workflows

Worker-side billing workflows — the Restate-durable orchestration that *uses* the `billing` provider seam. Hosted by the
`workflows-service` Restate worker, which binds `BillingCanaryService` alongside the `Notation`, `Archives`, and
`DriveSync` services: one endpoint, no separate billing pod.

Where `billing` is the *what* (the provider abstraction), this crate is the *when/durably* (the worker flows that call
it). It depends on **both** `billing` and `restate-sdk`, which is exactly why it is separate from the Restate-free
`billing` crate — see [`billing/README.md`](../billing/README.md#why-a-separate-crate).

## What it provides

- `BillingCanary` — a nightly health check that proves the Xero integration is live end-to-end. It find-or-creates a
  single stable canary contact and asserts the resolve is idempotent. The `billing-canary-trigger` `CronJob` starts one
  invocation per day; Restate owns the retry schedule.
- `BillingCanaryService` — the Restate service implementation `workflows-service` binds.
- `run_canary` / `build_confirmation` — the pure phase logic and the diagnostic email body, testable without a worker.

The future matter-close contact + invoice workflow lands here too, reusing the same `billing` provider seam.

## Layout

- `src/lib.rs` — the library hosted inside `workflows-service`.
- `src/bin/trigger.rs` — the thin `trigger` binary the `billing-canary-trigger` `CronJob` runs to start one invocation.
  Shipped as the `navigator-billing-canary-trigger` image (see the [`power-push`](../.claude/skills/power-push/SKILL.md)
  skill's trigger-image note).

## Getting started

```bash
# Canary phase logic + confirmation-email rendering. No Restate runtime or Xero account needed.
cargo test -p billing-workflows
```
