//! Policy decisions delegated to Open Policy Agent.
//!
//! The server posts a JSON `input` document to an OPA Data API URL
//! and reads back `result.allow` (a boolean). Rego policies live
//! in-cluster as a ConfigMap and OPA runs as a sidecar to every web
//! pod, so a decision call is a localhost round-trip.
//!
//! Configuration is read from `NAVIGATOR_OPA_URL` (default
//! `http://localhost:8181`). The `package` segment of the policy
//! determines the Data API path: this module hard-codes the
//! `navigator/authz/allow` endpoint that ships in
//! `k8s/base/opa/opa.yaml`.
//!
//! The `input.session.role` field — which the default policy
//! checks against the system-wide tier (`client`, `staff`, `admin`)
//! — is always sourced from the `persons.role` column on the local
//! database, never from the IdP token. See
//! [`docs/access-model.md`](../../../docs/access-model.md) and
//! [`docs/oidc.md`](../../../docs/oidc.md).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What [`PolicyClient::evaluate`] returns. `allow=false` is a deny;
/// the wrapper also surfaces the raw JSON from OPA so callers can
/// log it for audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecision {
    pub allow: bool,
    pub raw: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("HTTP error talking to OPA: {0}")]
    Http(#[from] reqwest::Error),
    #[error("OPA returned non-2xx: status={status} body={body}")]
    Status { status: u16, body: String },
    #[error("OPA response was not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Wire shape OPA expects on the Data API.
#[derive(Debug, Serialize)]
struct OpaRequest<'a> {
    input: &'a serde_json::Value,
}

/// Wire shape OPA returns from the Data API.
#[derive(Debug, Deserialize)]
struct OpaResponse {
    /// `result` is whatever the policy evaluates to. For an `allow`
    /// rule that's a boolean; we tolerate it being missing (deny)
    /// or a richer object (look up `allow` inside).
    #[serde(default)]
    result: serde_json::Value,
}

/// Thin reqwest-based client. Cheap to clone (Arc-shared internally).
///
/// A `PolicyClient` can be in one of two modes:
///
/// - **Enforced** — built with [`PolicyClient::new`] or
///   [`PolicyClient::from_env`] when `NAVIGATOR_OPA_URL` is set. The
///   `require_policy` middleware posts every request to OPA and
///   honors the decision.
/// - **Passthrough** — built with [`PolicyClient::passthrough`] (or
///   `from_env` when the env var is unset). The middleware logs a
///   one-line warning at boot and lets every request through. Used
///   by `cargo test` paths that don't exercise the policy layer
///   and by local development where standing up OPA would be
///   over-engineering.
#[derive(Debug, Clone)]
pub struct PolicyClient {
    inner: ClientInner,
}

#[derive(Debug, Clone)]
enum ClientInner {
    Enforced {
        http: reqwest::Client,
        decision_url: String,
    },
    Passthrough,
}

impl PolicyClient {
    /// Build an enforced client that posts to
    /// `<base_url>/v1/data/navigator/authz/allow`. `base_url` should
    /// be just scheme+host+port (e.g. `http://localhost:8181`), no
    /// trailing slash.
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        let base = base.trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .expect("build reqwest client");
        Self {
            inner: ClientInner::Enforced {
                http,
                decision_url: format!("{base}/v1/data/navigator/authz/allow"),
            },
        }
    }

    /// Build a passthrough client — `evaluate` always returns
    /// `allow=true` without touching the network. Use for tests
    /// that don't care about policy, or local dev that hasn't
    /// stood up an OPA sidecar.
    #[must_use]
    pub fn passthrough() -> Self {
        Self {
            inner: ClientInner::Passthrough,
        }
    }

    /// `true` when this client makes real HTTP calls.
    #[must_use]
    pub fn is_enforced(&self) -> bool {
        matches!(self.inner, ClientInner::Enforced { .. })
    }

    /// The decision URL this client posts to, when enforced. Used by
    /// the `/readyz` probe to ping OPA at boot/health-check time.
    #[must_use]
    pub fn decision_url(&self) -> Option<&str> {
        match &self.inner {
            ClientInner::Enforced { decision_url, .. } => Some(decision_url),
            ClientInner::Passthrough => None,
        }
    }

    /// Cheap reachability check for OPA. Sends a GET against the OPA
    /// `/health` endpoint (independent of the decision URL) and
    /// returns `Ok(())` on a 2xx. Always returns `Ok(())` for the
    /// passthrough client so dev environments never fail readiness
    /// because OPA isn't running.
    pub async fn probe_health(&self) -> Result<(), String> {
        let ClientInner::Enforced { http, decision_url } = &self.inner else {
            return Ok(());
        };
        // `decision_url` is `<base>/v1/data/navigator/authz/allow`; we
        // want `<base>/health` for the OPA health endpoint.
        let base = decision_url
            .split("/v1/data/")
            .next()
            .unwrap_or(decision_url);
        let health = format!("{base}/health");
        match http.get(&health).send().await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(format!("opa /health returned {}", resp.status())),
            Err(e) => Err(format!("opa /health unreachable: {e}")),
        }
    }

    /// Read the OPA base URL from `NAVIGATOR_OPA_URL`. When that
    /// env var is unset, returns a passthrough client (policy is
    /// effectively disabled). Set the URL to enable enforcement.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("NAVIGATOR_OPA_URL") {
            Ok(base) if !base.trim().is_empty() => Self::new(base),
            _ => Self::passthrough(),
        }
    }

    /// Evaluate the policy against `input`. Any error talking to
    /// OPA returns `Err`; the caller decides whether to fail-closed
    /// (treat the error as a deny) or surface the error verbatim.
    /// Passthrough clients always return `allow=true`.
    pub async fn evaluate(&self, input: &serde_json::Value) -> Result<PolicyDecision, PolicyError> {
        let (http, decision_url) = match &self.inner {
            ClientInner::Enforced { http, decision_url } => (http, decision_url),
            ClientInner::Passthrough => {
                return Ok(PolicyDecision {
                    allow: true,
                    raw: serde_json::Value::Bool(true),
                });
            }
        };
        Self::do_evaluate(http, decision_url, input).await
    }

    async fn do_evaluate(
        http: &reqwest::Client,
        decision_url: &str,
        input: &serde_json::Value,
    ) -> Result<PolicyDecision, PolicyError> {
        let resp = http
            .post(decision_url)
            .json(&OpaRequest { input })
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PolicyError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let parsed: OpaResponse = resp.json().await?;
        let allow = parsed
            .result
            .as_bool()
            .or_else(|| {
                parsed
                    .result
                    .as_object()
                    .and_then(|m| m.get("allow"))
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or(false);
        Ok(PolicyDecision {
            allow,
            raw: parsed.result,
        })
    }
}

/// Axum middleware that requires OPA `allow=true` for the request.
///
/// Reads the session cookie (if any), builds an `input` JSON
/// containing `path`, `method`, and `session`, and POSTs it to OPA.
/// On `allow=true` the next handler runs; on `allow=false` or any
/// transport error the request is rejected with `403 Forbidden`.
/// Errors are logged but never leaked to the client.
///
/// Designed to live underneath an existing session/auth middleware
/// so callers can rely on the session being populated before this
/// middleware fires. When no session cookie is present, the input
/// `session` field is `null` and the policy decides whether
/// unauthenticated access is allowed.
pub async fn require_policy(
    axum::extract::State((sessions, client)): axum::extract::State<(
        crate::session::SessionStore,
        PolicyClient,
    )>,
    cookies: tower_cookies::Cookies,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    // Primary: browser SSO sets a session cookie.
    let mut session = cookies
        .get(crate::session::SESSION_COOKIE_NAME)
        .and_then(|c| sessions.decode(c.value()));
    // Fallback: a bearer-token / Google-OAuth middleware upstream
    // has already authenticated the caller and inserted AuthClaims.
    // Synthesize a session-shaped value so the OPA rule that checks
    // `input.session.role` works for both flows uniformly.
    if session.is_none() {
        if let Some(claims) = req.extensions().get::<crate::auth::AuthClaims>() {
            session = Some(crate::session::SessionData {
                sub: claims.sub.clone(),
                email: Some(claims.sub.clone()),
                person_id: None,
                exp: claims.exp.max(0),
                role: claims.role,
                csrf_token: String::new(),
                source: crate::session::SessionSource::Browser,
            });
        }
    }
    let swagger_ui_request = req.headers().contains_key("x-navigator-swagger-ui");
    let path = req.uri().path().to_string();
    let path_segments: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    let input = serde_json::json!({
        "path": path_segments,
        "method": req.method().as_str(),
        "session": session,
    });
    match client.evaluate(&input).await {
        Ok(decision) if decision.allow => Ok(next.run(req).await),
        Ok(_) => Ok(deny_response(&path, session.is_some(), swagger_ui_request)),
        Err(e) => {
            tracing::warn!(error = %e, "policy evaluation failed; failing closed");
            Ok(deny_response(&path, session.is_some(), swagger_ui_request))
        }
    }
}

/// Render the denial. Anonymous visitors get a 303 to the OIDC
/// start endpoint so the experience is "click protected link →
/// land at Keycloak", not "click protected link → blank 403 page".
/// Authenticated visitors who lack the role stay at 403 — the IdP
/// flow won't help them; they need a role grant in the DB. For
/// browser surfaces that 403 carries the styled HTML page; for
/// `/api/*` and `/mcp` it stays a tiny JSON body so JSON-RPC clients
/// see a parseable error.
fn deny_response(
    path: &str,
    has_session: bool,
    swagger_ui_request: bool,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if has_session {
        tracing::info!(path, "policy denied request (authenticated; 403)");
        if crate::wants_json(path) {
            (
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": "forbidden" })),
            )
                .into_response()
        } else {
            (
                axum::http::StatusCode::FORBIDDEN,
                views::forbidden_page_with_auth(views::AuthState::Authenticated),
            )
                .into_response()
        }
    } else if swagger_ui_request && crate::wants_json(path) {
        let login = format!("/auth/login?return_to={}", percent_encode_path("/api-docs"));
        tracing::info!(path, login = %login, "policy denied Swagger UI request (anonymous; 401)");
        (
            axum::http::StatusCode::UNAUTHORIZED,
            [(
                axum::http::header::WWW_AUTHENTICATE,
                "NavigatorSession realm=\"Neon Law Navigator API\"",
            )],
            axum::Json(serde_json::json!({
                "error": "unauthenticated",
                "message": "Sign in before using Swagger UI's Try it out.",
                "login": login,
            })),
        )
            .into_response()
    } else {
        let target = format!("/auth/login?return_to={}", percent_encode_path(path));
        tracing::info!(path, target = %target, "policy denied request (anonymous; redirecting to /auth/login)");
        axum::response::Redirect::to(&target).into_response()
    }
}

/// Percent-encode a path so it survives being a `?return_to=` query
/// value. Only the small set of characters that materially break a
/// query string (`?`, `&`, `#`, `%`, `+`, space) is encoded — `/`
/// stays raw so the resulting URL is readable in the Location header.
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'?' => out.push_str("%3F"),
            b'&' => out.push_str("%26"),
            b'#' => out.push_str("%23"),
            b'%' => out.push_str("%25"),
            b'+' => out.push_str("%2B"),
            b' ' => out.push_str("%20"),
            _ => out.push(b as char),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{PolicyClient, PolicyError};
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn evaluate_returns_allow_true_when_opa_says_true() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/data/navigator/authz/allow"))
            .and(body_json(json!({ "input": { "user": "libra" } })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
            .mount(&mock)
            .await;

        let client = PolicyClient::new(mock.uri());
        let decision = client.evaluate(&json!({ "user": "libra" })).await.unwrap();
        assert!(decision.allow);
    }

    #[tokio::test]
    async fn evaluate_returns_allow_false_when_opa_omits_result() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/data/navigator/authz/allow"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&mock)
            .await;

        let client = PolicyClient::new(mock.uri());
        let decision = client.evaluate(&json!({})).await.unwrap();
        assert!(!decision.allow);
    }

    #[tokio::test]
    async fn evaluate_surfaces_non_2xx_responses_as_error() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/data/navigator/authz/allow"))
            .respond_with(ResponseTemplate::new(500).set_body_string("upstream broke"))
            .mount(&mock)
            .await;

        let client = PolicyClient::new(mock.uri());
        let err = client.evaluate(&json!({})).await.unwrap_err();
        match err {
            PolicyError::Status { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("upstream broke"));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }
}
