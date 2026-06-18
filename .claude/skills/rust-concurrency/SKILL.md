---
name: rust-concurrency
description: >
  Async + concurrency patterns for the navigator workspace — Tokio runtime, structured concurrency, channels, shared
  state, cancellation. Trigger when writing code that uses `async fn`, `tokio::`, `tokio::spawn`, `JoinSet`, `select!`,
  `Arc<Mutex<_>>`, channels, or anywhere a task can outlive its caller. Also trigger before adding a new dependency that
  pulls in a different async runtime — we standardize on Tokio.
---

# Rust concurrency in the navigator workspace

Runtime is Tokio, multi-threaded scheduler, `flavor = "current_thread"` reserved for short tests. Workspace lint
`unsafe_code = "forbid"` is non-negotiable.

## The shape we use

- **Async fn over manually `impl Future`.** Boxed futures (`Pin<Box<dyn Future…>>`) only appear at trait boundaries
  (`async_trait`) or at FFI/dyn edges.
- **Structured concurrency first.** `tokio::join!`, `tokio::try_join!`, `JoinSet`, and `select!` cover the cases where
  two or more futures share a lifetime. A bare `tokio::spawn` needs a documented owner and a cancellation story (drop
  handle, `CancellationToken`, or `JoinHandle::abort`).
- **`Send + 'static` future bounds** are the default. If you find yourself reaching for `LocalSet`, the work probably
  belongs on a blocking thread (`spawn_blocking`) or on a single-threaded utility runtime.

## Shared state

- `Arc<T>` where `T: Sync` — immutable shared config, handles to `DatabaseConnection`, `reqwest::Client`.
- `Arc<tokio::sync::Mutex<T>>` — async-aware critical sections that may await while held. Avoid holding across `.await`
  boundaries if you can — measure first.
- `Arc<tokio::sync::RwLock<T>>` — read-mostly state.
- `Arc<std::sync::Mutex<T>>` — cheap, short, non-awaiting critical sections. Default to this for plain field updates.
- `tokio::sync::watch` — "latest value" broadcast — config reload, leadership changes.
- `tokio::sync::Notify` — one-shot wakeups for async event-driven code.
- `OnceCell` / `OnceLock` — lazy singletons. Prefer over `lazy_static!`.

`Mutex<T>` may not be held across `.await` unless every callsite is audited; prefer to clone-out the value, drop the
guard, then await.

## Channels — pick the smallest one that fits

- `tokio::sync::oneshot` — single producer, single consumer, fire-and-forget result.
- `tokio::sync::mpsc` — bounded by default. Pick a real bound; `unbounded_channel` is a memory leak waiting for a slow
  consumer.
- `tokio::sync::broadcast` — fan-out, lossy on slow receivers.
- `tokio::sync::watch` — single most-recent value; great for "stop" signals and config.

## Cancellation + timeouts

- Wrap any external call in `tokio::time::timeout(dur, fut)`. Network/database calls never run unbounded.
- Use `tokio_util::sync::CancellationToken` for tree-shaped cancellation (parent cancels, every child child-token
  cancels).
- Drop-to-cancel: if you `tokio::spawn` and never store the `JoinHandle`, you've created a leak; structured concurrency
  primitives propagate panics, bare spawns swallow them.

## Region-based isolation and `Send`

If the compiler complains a future isn't `Send`, the usual culprit is a `Rc`, `RefCell`, or a guard held across
`.await`. Replace with `Arc`/`Mutex` or drop the guard before awaiting. `#[tokio::main(flavor = "current_thread")]` does
**not** loosen this constraint when spawning into a multi-threaded runtime later.

## Blocking work

- CPU-bound or sync-only IO (file system, sync `rusqlite`, sync DNS): `tokio::task::spawn_blocking`.
- Long-running blocking loops: dedicated `std::thread` plus a `mpsc` for async ↔ sync handoff. Do not occupy a tokio
  worker for more than a few microseconds without yielding.

## Testing

- Use `#[tokio::test]` for async tests; `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` when reproducing
  race-condition concerns.
- `tokio::time::pause()` + `advance()` for deterministic timeouts.
- `tokio::task::yield_now().await` to deterministically interleave futures in tests.

## Anti-patterns

- `tokio::spawn` inside library code without an owner.
- `unbounded_channel` outside of intra-process control planes.
- `block_on` inside async context (deadlocks the runtime).
- `tokio::main` on a library crate; runtime selection belongs to the binary.
- `Arc<Mutex<HashMap<…>>>` as a load-bearing cache — reach for `dashmap`, `moka`, or an actor instead.

## Canonical sources

- Tokio docs: <https://docs.rs/tokio>
- Tokio repository: <https://github.com/tokio-rs/tokio>
- Tokio tutorial (async + channels + select): <https://tokio.rs/tokio/tutorial>
- Async Book (Rust async fundamentals): <https://rust-lang.github.io/async-book/>
- `tokio-util` (CancellationToken, codec): <https://docs.rs/tokio-util>
- The Rust Programming Language Book — Fearless Concurrency: <https://doc.rust-lang.org/book/ch16-00-concurrency.html>
- Edition guide — async traits: <https://doc.rust-lang.org/edition-guide/rust-next/async-traits.html>
