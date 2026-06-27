//! JSON API surface.
//!
//! Read-only listings for the core domain tables. Every handler is a
//! thin SeaORM query + `Json(...)` response — no extra serializer
//! between the model and the wire because the entity `Model`s
//! derive `Serialize` directly.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use sea_orm::EntityTrait;

use store::entity::{entity, entity_type, jurisdiction, person};
use store::Db;

/// Mount every `/api/*` route onto a router that already has `Db` as
/// state. Returns the same router so callers can chain.
pub fn routes() -> Router<Db> {
    Router::new()
        .route("/api/people", axum::routing::get(list_people))
        .route("/api/people/{id}", axum::routing::get(get_person))
        .route("/api/entities", axum::routing::get(list_entities))
        .route("/api/entities/{id}", axum::routing::get(get_entity))
        .route("/api/jurisdictions", axum::routing::get(list_jurisdictions))
        .route("/api/entity-types", axum::routing::get(list_entity_types))
        .route(
            "/api/notations/validate",
            axum::routing::post(validate_notation),
        )
        .route("/openapi.json", axum::routing::get(openapi_json))
        .route("/api/docs", axum::routing::get(api_docs))
}

/// The `/api/*` routes that should appear in the OpenAPI document, in
/// OpenAPI path-template form (`{id}`, not axum's `:id`). Kept in
/// lockstep with [`routes`] by `web/tests/openapi_drift.rs`. Excludes
/// `/openapi.json` and `/api/docs` because those are meta-endpoints
/// (the doc itself and the Swagger UI shell), not part of the public
/// API surface the document describes.
#[must_use]
pub fn documented_api_paths() -> Vec<&'static str> {
    vec![
        "/api/people",
        "/api/people/{id}",
        "/api/entities",
        "/api/entities/{id}",
        "/api/jurisdictions",
        "/api/entity-types",
        "/api/notations/validate",
    ]
}

/// Static Swagger UI shell. Loads the vendored `swagger-ui-dist`
/// assets from `/public/swagger-ui/` and points the renderer at
/// `/openapi.json`. Public via its own OPA exemption, alongside
/// `/openapi.json` — the documentation describes the API but is not
/// the API, so the OIDC gate guards the data endpoints it documents,
/// not the docs themselves. The shell renders the public
/// `/openapi.json` and calls no protected route, so a session would
/// add nothing. The per-response `Content-Security-Policy` header
/// keeps script execution on the same origin — the whole point of
/// vendoring rather than CDN-loading the dist is so this header can
/// stay strict.
async fn api_docs() -> impl IntoResponse {
    const HTML: &str = include_str!("../public/swagger-ui/index.html");
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                axum::http::header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; \
                 script-src 'self'; \
                 style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; \
                 connect-src 'self'; \
                 frame-ancestors 'none'",
            ),
        ],
        axum::response::Html(HTML),
    )
}

async fn openapi_json(headers: axum::http::HeaderMap) -> Json<serde_json::Value> {
    let authority = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let base = crate::openapi::base_url_for(authority);
    Json(crate::openapi::document_with_base(&base))
}

async fn list_people(State(db): State<Db>) -> Result<Json<Vec<person::Model>>, ApiError> {
    let rows = person::Entity::find().all(&db).await?;
    Ok(Json(rows))
}

async fn get_person(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
) -> Result<Json<person::Model>, ApiError> {
    person::Entity::find_by_id(id)
        .one(&db)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn list_entities(State(db): State<Db>) -> Result<Json<Vec<entity::Model>>, ApiError> {
    let rows = entity::Entity::find().all(&db).await?;
    Ok(Json(rows))
}

async fn get_entity(
    State(db): State<Db>,
    Path(id): Path<Uuid>,
) -> Result<Json<entity::Model>, ApiError> {
    entity::Entity::find_by_id(id)
        .one(&db)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn list_jurisdictions(
    State(db): State<Db>,
) -> Result<Json<Vec<jurisdiction::Model>>, ApiError> {
    let rows = jurisdiction::Entity::find().all(&db).await?;
    Ok(Json(rows))
}

async fn list_entity_types(
    State(db): State<Db>,
) -> Result<Json<Vec<entity_type::Model>>, ApiError> {
    let rows = entity_type::Entity::find().all(&db).await?;
    Ok(Json(rows))
}

/// Request body for `POST /api/notations/validate`. The caller hands
/// over markdown they're drafting; we lint it and return violations
/// without touching the database.
#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    /// Raw markdown body, including any YAML frontmatter.
    pub contents: String,
    /// Optional pretend filename so rules that key off the path
    /// (`N103` snake_case) and the response have something meaningful
    /// to report. Defaults to `notation.md` — a snake_case placeholder
    /// so the default doesn't pollute the response with a filename
    /// complaint the caller never intended.
    #[serde(default)]
    pub path: Option<String>,
    /// When true, lint with the Markdown-only rule set (drops the
    /// N-family, adds `S102` line packing) — same as
    /// `cli validate --markdown-only`. Defaults to false: the full
    /// Neon Law Navigator notation rule set runs.
    #[serde(default)]
    pub markdown_only: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub path: String,
    pub clean: bool,
    pub violations: Vec<ValidationViolation>,
}

#[derive(Debug, Serialize)]
pub struct ValidationViolation {
    pub code: &'static str,
    pub line: usize,
    pub message: String,
}

/// Lint markdown without persisting it. Mirrors the `cli validate`
/// rule-set selection so a notation that passes the CLI passes here
/// and vice versa. Requires an authenticated session — same posture
/// as the rest of `/api/*`.
async fn validate_notation(Json(req): Json<ValidateRequest>) -> Json<ValidateResponse> {
    let path = req.path.unwrap_or_else(|| "notation.md".to_string());
    let rule_set = if req.markdown_only {
        rules::navigator_markdown_only_rules()
    } else {
        rules::navigator_default_rules()
    };
    let file = rules::SourceFile {
        path: std::path::PathBuf::from(&path),
        contents: req.contents,
    };
    let mut violations = Vec::new();
    for rule in &rule_set {
        for v in rule.lint(&file) {
            violations.push(ValidationViolation {
                code: v.code,
                line: v.line,
                message: v.message,
            });
        }
    }
    // `clean` means no *blocking* (Error-severity) violations. Yellow
    // advisories like N112 ("step allowed but not built yet") are still
    // returned in `violations` so the caller sees them, but they don't
    // flip `clean` to false — mirroring `navigator validate`.
    let clean = !violations
        .iter()
        .any(|v| rules::severity_for_code(v.code) == rules::Severity::Error);
    Json(ValidateResponse {
        path,
        clean,
        violations,
    })
}

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    Db(sea_orm::DbErr),
}

impl From<sea_orm::DbErr> for ApiError {
    fn from(e: sea_orm::DbErr) -> Self {
        Self::Db(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "not_found" })),
            )
                .into_response(),
            Self::Db(e) => {
                tracing::error!(error = %e, "api: db error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "internal" })),
                )
                    .into_response()
            }
        }
    }
}
