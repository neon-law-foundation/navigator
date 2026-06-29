---
name: rust
description: >
  Workspace Rust guardrails. Trigger on the sharpest moments: a change that adds `unsafe`, `unwrap`, `expect`, or
  `panic!` outside `main()`/tests; introducing a new public API, error type, or module; reaching for a different web
  framework, ORM, or async runtime (we standardize on Axum + SeaORM + Tokio); or wiring a new binary's `main()`. Read
  [`docs/rust-programming.md`](../../../docs/rust-programming.md) before acting — it is the authoritative reference.
---

# Rust guardrails

The doc owns the conventions; this skill is the short list of guards that are easy to violate. Read
[`docs/rust-programming.md`](../../../docs/rust-programming.md) and keep it, not this skill, authoritative.

- **No `unwrap`/`expect`/`panic!` outside `main()` and tests.** Use `?` with `anyhow` (binaries) or `thiserror`
  (libraries); `expect("invariant: …")` only when the invariant is provable in one line for a future reader.
- **`unsafe_code = "forbid"`** at the workspace level — never reach for `unsafe`.
- **Standardize on Axum + SeaORM + Tokio.** Don't add a second web framework, ORM, or async runtime; extend the existing
  router, entity, and runtime instead.
- **One canonical shutdown-signal helper** for service lifecycle (SIGTERM + SIGINT). No ad-hoc `ctrl_c().await.unwrap()`
  inline in `main`.
- **Axum body/consuming extractors go LAST** in handler argument order — the body can only be consumed once.

Everything else — conventions, async, Axum, SeaORM, service lifecycle, testing — is in
[`docs/rust-programming.md`](../../../docs/rust-programming.md).
