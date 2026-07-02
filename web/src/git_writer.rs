//! Repo-ensure endpoint of the **single mounted writer** — the `web`
//! deployment that mounts the repo volume (`git-serving` in prod).
//!
//! Matter creation is a hard-blocked step on surfaces that do *not* mount
//! the volume (the stateless `web` tier); they provision through
//! [`store::projects::RepoEnsurer::Remote`], which POSTs here
//! ([`store::projects::GIT_WRITER_ENSURE_PATH`]) with a shared bearer token.
//! This handler is the filesystem half only — an idempotent
//! [`repos::RepoStore::ensure`]; the `git_initialized_at` stamp stays on the
//! caller's open transaction (docs/git-project-repos.md §6).
//!
//! ## Exposure + auth
//!
//! The route mounts only when this process is the writer (repo root + token
//! both configured), so the stateless tier serves a plain `404` here. The
//! prod ingress routes only `/projects/*` to `navigator-git`, so the
//! endpoint is reachable solely through the in-cluster Service — and even
//! there it requires the bearer from [`store::projects::GIT_WRITER_TOKEN_ENV`]
//! (both sides read it from `navigator-web-secrets`). The wire carries a
//! project id and a repo path, never client content.

use std::sync::Arc;

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use store::projects::{EnsureRepoRequest, EnsureRepoResponse};

use crate::AppState;

/// What the writer role needs: the volume-backed repo store and the shared
/// bearer token callers must present.
pub struct WriterConfig {
    pub token: String,
    pub store: repos::RepoStore,
}

impl WriterConfig {
    /// Resolve the writer role from the process environment: `Some` only
    /// when both the repo root and the writer token are set.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// [`WriterConfig::from_env`] against any lookup — the test seam.
    #[must_use]
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Option<Self> {
        let root = get(repos::REPO_ROOT_ENV).filter(|s| !s.is_empty())?;
        let token = get(store::projects::GIT_WRITER_TOKEN_ENV).filter(|s| !s.is_empty())?;
        Some(Self {
            token,
            store: repos::RepoStore::new(root),
        })
    }
}

/// Mount the ensure route when this process is the writer; otherwise an
/// empty router, so the path falls through to the app's `404`.
pub fn routes() -> Router<AppState> {
    routes_with(WriterConfig::from_env())
}

/// [`routes`] with an explicit config — the seam tests use to build a
/// standalone writer without touching process env vars. Generic over the
/// router state because the handler carries its config in the closure.
pub fn routes_with<S>(config: Option<WriterConfig>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let Some(config) = config else {
        return Router::new();
    };
    let config = Arc::new(config);
    Router::new().route(
        store::projects::GIT_WRITER_ENSURE_PATH,
        post(move |headers: HeaderMap, body: Json<EnsureRepoRequest>| {
            ensure(config.clone(), headers, body)
        }),
    )
}

async fn ensure(
    config: Arc<WriterConfig>,
    headers: HeaderMap,
    Json(req): Json<EnsureRepoRequest>,
) -> Response {
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|presented| presented == config.token);
    if !authorized {
        return (StatusCode::UNAUTHORIZED, "invalid writer token").into_response();
    }

    let store = config.store.clone();
    let project_id = req.project_id;
    // `ensure` shells to `git` (blocking); keep it off the async runtime.
    match tokio::task::spawn_blocking(move || store.ensure(project_id)).await {
        Ok(Ok(path)) => Json(EnsureRepoResponse {
            path: path.to_string_lossy().into_owned(),
        })
        .into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, %project_id, "git_writer: ensure failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "repo ensure failed").into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %project_id, "git_writer: ensure task failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "repo ensure failed").into_response()
        }
    }
}
