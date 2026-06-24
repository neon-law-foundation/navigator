# Rust programming

Navigator is a Rust workspace. This page is the common Rust reference for agents and humans; use the official Rust
sources below when language behavior matters.

Canonical language references:

- [The Rust Programming Language](https://doc.rust-lang.org/book/)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust async book](https://rust-lang.github.io/async-book/)
- [Rust edition guide](https://doc.rust-lang.org/edition-guide/)
- [rustfmt reference](https://rust-lang.github.io/rustfmt/)
- [Clippy lint index](https://rust-lang.github.io/rust-clippy/master/)

## Workspace defaults

- Toolchain: Rust 1.96.0, edition 2021.
- `unsafe_code = "forbid"` at the workspace level.
- `rustfmt` is authoritative.
- Clippy pedantic warnings are enabled; run with `-D warnings` before committing.
- Tokio is the async runtime.
- Axum is the web framework.
- SeaORM is the ORM.
- Restate SDK belongs only in `workflows-service`; the rest of the workspace submits through the `workflows` crate.

## Error handling

- Libraries use typed `thiserror` enums.
- Binaries use `anyhow::Result<T>` at the boundary and add context with `.context(...)` or `.with_context(...)`.
- HTTP handlers convert through the workspace `AppError` / `IntoResponse` pattern.
- Do not use `Box<dyn Error>` in public signatures.
- Do not `unwrap()` or `expect()` outside tests and `main()` unless the invariant is truly local and the message proves
  it in one line.

## Types and modules

- Prefer newtypes for ids that cross module boundaries.
- Prefer enums over booleans for meaningful state.
- Prefer `Option<T>` over sentinel values.
- Accept `&str` instead of `String`, and `&[T]` instead of `Vec<T>`, when ownership is not needed.
- Keep one concept per file; split large files when two concepts are hiding inside one module.
- Use `mod foo;` plus `pub use foo::Type;` for public re-exports.

## Async and concurrency

- Use `async fn` over manual future types except at trait/object boundaries.
- Use structured concurrency first: `tokio::join!`, `tokio::try_join!`, `JoinSet`, and `select!`.
- A bare `tokio::spawn` needs an owner and a cancellation story.
- Use bounded channels. `unbounded_channel` is allowed only for tightly controlled intra-process control planes.
- Do not hold a `Mutex` guard across `.await` unless the call sites are audited.
- Use `spawn_blocking` for CPU-bound or sync-only blocking work.
- Use `tokio::time::timeout` around external calls.

Inside Restate handlers, the rules are stricter: do not use native concurrency for journaled work. See
[`agent-workflows.md`](agent-workflows.md#author-a-restate-handler) and [`durable-workflows.md`](durable-workflows.md).

## Axum

- Add routes in the existing router shape; do not introduce another web framework.
- Prefer typed extractors and explicit state over ad-hoc request parsing.
- Keep auth and visibility checks close to existing middleware/access helpers.
- New `/portal/...` routes must respect [`access-model.md`](access-model.md) and OPA policy.
- Return `404` where the existing surface intentionally hides staff-only management routes from clients.

## SeaORM and Postgres

- Postgres is the only database. No SQLite fallback.
- Migrations, entities, and seed changes must land together when they depend on each other.
- Use transactions for multi-row invariants.
- Raw SQL skips SeaORM-managed timestamp behavior; set `updated_at = now()` yourself when doing approved production SQL.
- Re-seeding is idempotent and inserts missing rows; it does not update live production rows.

## Service lifecycle

- Long-running binaries initialize config, database, telemetry, and external clients before serving traffic.
- Hold the telemetry guard until the end of `main`.
- Use the workspace shutdown helper instead of ad-hoc signal handling.
- Health and readiness endpoints should reflect the dependency contract the service actually needs.

## Testing

- Tests ship in the same commit as the implementation.
- Unit tests live beside code in `#[cfg(test)] mod tests`.
- Integration tests live under `<crate>/tests/`.
- Async tests use `#[tokio::test]`.
- CLI smoke tests use `assert_cmd` and `predicates`.
- Snapshot tests are appropriate for HTML and JSON shapes.
- Restate handler changes need replay-aware coverage, not only a happy-path compile.

## Dependencies and assets

- Routine Rust dependency refresh uses `cargo update` for semver-compatible lockfile updates.
- `cargo upgrade` changes version requirements and needs explicit review.
- Keep crate updates separate from vendored frontend asset refreshes.
- Vendored web assets are served same-origin from `web/public/`; do not link runtime CDNs.

## Before committing Rust

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run narrower tests while iterating, but report the exact gate you actually ran.
