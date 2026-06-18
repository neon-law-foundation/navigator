---
name: rust-axum
description: >
  Axum 0.7 router / handler / extractor / middleware patterns used by `web`. Trigger when adding or modifying an HTTP
  route, an extractor, a middleware layer, a `State<…>` field, or a response type; or when wiring an `axum::Router` in
  tests. Also trigger when reaching for a different web framework — we standardize on Axum.
---

# Axum 0.7 patterns for `web`

The web crate is `axum = "0.7"` + `tower` + `tower-http`. Maud renders HTML; SeaORM speaks to the database; tower
middleware handles cookies, tracing, OPA.

## Router shape

- One `pub fn router(state: AppState) -> Router` per logical area (auth, admin, marketing). Compose with
  `.merge(other_router)` or `.nest("/path", other_router)`.
- The top-level binary builds the final `Router` by merging area routers and applying global middleware *once*.
  Per-area middleware is added inside that area's `router()` function with `.layer(...)`.
- `Router::with_state(AppState)` for typed state. Avoid `Extension<T>` for app-level state — extractors are
  stronger-typed and refactor more cleanly.

## Handlers

- Async fn returning `impl IntoResponse`. Concrete return types are fine for handlers with one possible response shape;
  use `Result<impl IntoResponse, AppError>` when failure is possible.
- One extractor per parameter, in the canonical Axum order: path → query → state → headers/cookies → body. Body
  extractors (`Json`, `Form`, multipart) come **last** — Axum can only consume the body once.
- Return a struct that implements `IntoResponse` (maud `Markup` already does). For JSON: `axum::Json(value)`; for
  redirects: `axum::response::Redirect::to`.

## Extractors

| Built-in | Use |
| --- | --- |
| `Path<T>` | URL segments (typed). |
| `Query<T>` | Query string into a `Deserialize` struct. |
| `Json<T>` / `Form<T>` | Request body. |
| `State<AppState>` | App-wide handles (DB pool, OPA client, config). |
| `tower_cookies::Cookies` | Cookies — required by our session middleware. |

Write a custom `FromRequestParts` impl for cross-cutting auth (`AuthedPerson`, `StaffOnly`) so handlers receive an
already-validated identity instead of re-decoding cookies in every handler.

## Error handling

- One `AppError` enum per crate with `#[derive(thiserror::Error)]`. Implement `IntoResponse` once.
- `?` propagates `Result<_, AppError>`. Map non-`AppError` errors at the call site, not inside `IntoResponse`.
- `5xx` responses must log via `tracing` before returning. `4xx` responses don't.

## Middleware (tower layers)

- `tower_http::trace::TraceLayer` — request/response tracing, applied globally.
- `tower_http::services::ServeDir` for static assets.
- `tower_cookies::CookieManagerLayer` for our HMAC-signed sessions.
- Custom OPA middleware: `axum::middleware::from_fn_with_state(state, opa_guard)`. Skip when `NAVIGATOR_OPA_URL` is
  unset (dev pass-through).
- `from_fn_with_state` over `from_fn` whenever the middleware needs anything from `AppState` — it threads typed state
  cleanly.

## Testing handlers

- Build the router under test (`router(test_state())`), call it via `tower::ServiceExt::oneshot(Request::builder()…)`.
  No real socket, deterministic.
- `axum::body::Body::to_bytes` (via `http_body_util::BodyExt::collect`) to assert response bodies.
- Use `store::test_support::pg()` for handler tests that touch the DB — a per-test Postgres schema inside a per-binary
  container. Never mock SeaORM.

## Graceful shutdown

`axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await` — see the [[rust-service-lifecycle]] skill
for the signal handler shape and drain semantics.

## Anti-patterns

- Async trait objects for routing (`Box<dyn Service<…>>`) — write handlers as plain functions and let the type system
  carry them.
- Extension-based config when typed state would compile.
- Reading the body twice — extractors consume it; use `Bytes` or buffer it once.
- `unwrap()` in handlers; that's a `500` waiting to happen with no log line. Use `?` and a real `AppError`.

## Canonical sources

- Axum docs: <https://docs.rs/axum>
- Axum repository + examples: <https://github.com/tokio-rs/axum>
- `axum/examples/` directory (router composition, middleware, custom extractors, testing):
  <https://github.com/tokio-rs/axum/tree/main/examples>
- Tower service trait: <https://docs.rs/tower>
- `tower-http`: <https://docs.rs/tower-http>
- `tower-cookies`: <https://docs.rs/tower-cookies>
- Maud (HTML templating): <https://maud.lambda.xyz/> · <https://github.com/lambda-fairy/maud>
