---
name: rust-best-practices
description: >
  Workspace-wide Rust conventions — error handling (anyhow vs thiserror), modules, types, clippy, testing, and what NOT
  to do. Trigger when introducing a new public API, adding a new error type, deciding between `Result`/`Option`, naming
  a module or trait, writing a test, or before silencing a clippy lint. Also trigger on any PR that adds `unsafe`,
  `unwrap`, `expect`, or `panic!` outside of `main()`/tests.
---

# Rust conventions in the navigator workspace

Pinned toolchain: **Rust 1.95.0**, edition 2021, `rustfmt` + `clippy` components, `unsafe_code = "forbid"` at workspace lint level. Workspace clippy: `pedantic` at warn, with `module_name_repetitions`, `missing_errors_doc`, `missing_panics_doc` allowed.

## Error handling

- **Libraries** (`rules`, `views`, `workflows`): typed errors with `thiserror`. One enum per logical operation; variants carry source errors with `#[from]`.
- **Binaries** (`cli`, `web`, `compass`): `anyhow::Result<T>` at the boundary. Convert library `thiserror` errors into `anyhow` with `?` and add context with `.context("…")` / `.with_context(|| …)`.
- The HTTP error story (`AppError → IntoResponse`) lives in [[rust-axum]]; everything else funnels through `anyhow` in binaries.
- **Never** `unwrap()` or `expect()` outside tests and `main()`. The acceptable forms in production code are:
  - `let Some(x) = … else { return Err(…) };`
  - `?` on `Result` / `Option`.
  - `expect("invariant: <single line explanation>")` *only* when the invariant is genuinely provable from the surrounding code and a future reader needs the proof to stay valid.

## Modules and naming

- One concept per file. Files over ~400 lines almost always merge two concepts that want to be split.
- `mod foo;` + `pub use foo::Bar;` in `lib.rs` is the right re-export pattern; avoid `pub mod` for internal modules.
- Trait names are nouns or `-able` adjectives (`Storage`, `Cacheable`). Verb-named traits (`Fetch`, `Build`) usually want to be functions.
- Test modules: `#[cfg(test)] mod tests { use super::*; … }` inside the file under test for unit tests; `tests/<name>.rs` for integration tests.

## Types

- Newtypes (`pub struct PersonId(pub Uuid);`) for any ID that crosses a module boundary. Prevents `template_id` and `person_id` from being silently interchangeable at a call site.
- Derive `Clone, Debug, Eq, PartialEq, Hash` by default for value-like structs; add `Serialize, Deserialize` only when actually serialized.
- Use the language's actual ownership primitives — `Copy`, `Clone`, ownership, and lifetimes. Don't reach for traits or sigils borrowed from other languages.
- Prefer enums over booleans for state. `enum Visibility { Public, Internal }` beats `is_public: bool` at every call site.
- `Option<T>` over sentinel values. `Result<T, E>` over `Result<Option<T>, E>` when "not found" is an error in the caller's frame.

## Formatting

`rustfmt` is authoritative. Run `cargo fmt` before committing. Don't tweak `.rustfmt.toml` to silence a single case — restructure the code.

## Clippy

- Run `cargo clippy --workspace --all-targets -- -D warnings` before every commit.
- Silencing lints: prefer `#[allow(clippy::lint_name)]` at the smallest scope (item, not module). Add a one-line comment explaining why.
- Don't disable workspace-wide lints to ship a feature. If a clippy lint is wrong for our codebase across the board, move it to the workspace `allow` list with a comment in `Cargo.toml`.

## Testing

- TDD is the rule — tests in the same commit as the implementation they cover ([[feedback-commits-tdd]]).
- Unit tests live next to the code (`#[cfg(test)] mod tests`); integration tests live in `<crate>/tests/`.
- Async tests use `#[tokio::test]` (see [[rust-concurrency]]).
- One assertion per logical behavior; multiple `assert_eq!` lines per test are fine as long as they all probe the same behavior.
- `assert_cmd` + `predicates` for CLI smoke tests (the pattern in `cli/tests/`).
- Snapshot tests (`insta`) for HTML/JSON output where the shape matters more than a single field.

## Concurrency

See [[rust-concurrency]]. Short version: Tokio is the runtime; structured concurrency is the default; no `unbounded_channel` outside intra-process control planes; no holding a `Mutex` across `.await`.

## What NOT to do

- No `unsafe` (forbidden at workspace level).
- No `#![allow(warnings)]`, no blanket `#[allow(dead_code)]` on a module — earn the silence on individual items.
- No `lazy_static!` — use `std::sync::OnceLock` or `OnceCell`.
- No re-exports for "backwards compatibility" with code you wrote yesterday. Rename, fix call sites, move on.
- No `Box<dyn Error>` in public signatures — use a real `thiserror` enum.
- No `String` parameters where `&str` works; no `Vec<T>` parameters where `&[T]` works.
- No `.clone()` to silence a borrow-checker error. Restructure or take by value intentionally.
- No `.unwrap_or_default()` in a place where the default has a different meaning from the value (e.g., `Vec::new()` vs "user has no addresses" — make the empty-vs-missing distinction explicit).

## Commit / TDD discipline

**Branch → PR → auto-merge — never commit on `main`.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, every
change lands the same way: do the work on a topic branch (`git switch -c <topic>`), push and open a PR
(`git push -u origin <topic>` → `gh pr create`), then enable auto-merge (`gh pr merge --auto --squash`). `ci.yml` runs
on the PR and GitHub squash-merges it once every required check is green — you never commit to `main` and never merge by
hand. You don't have to invent branch ceremony; this flow is global.

Tests and implementation ship in the same commit. Before every commit:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Canonical sources

- The Rust Programming Language Book: <https://doc.rust-lang.org/book/>
- Rust by Example: <https://doc.rust-lang.org/rust-by-example/>
- Rust API Guidelines (naming, error, futureproofing): <https://rust-lang.github.io/api-guidelines/>
- `rustfmt` configuration reference: <https://rust-lang.github.io/rustfmt/>
- `clippy` lint index: <https://rust-lang.github.io/rust-clippy/master/>
- `thiserror`: <https://docs.rs/thiserror>
- `anyhow`: <https://docs.rs/anyhow>
- Rust edition guide: <https://doc.rust-lang.org/edition-guide/>
- Rust release notes (track 1.95+): <https://github.com/rust-lang/rust/blob/master/RELEASES.md>
