//! Google OAuth 2.0 access-token validator for `/mcp`.
//!
//! Why this exists instead of Identity-Aware Proxy: Google IAP
//! requires a JWT-shaped ID token (`eyJ...`) on incoming requests,
//! but Gemini Enterprise's Custom MCP Server data store sends the
//! standard *opaque* OAuth 2.0 access token (`ya29....`) instead.
//! IAP responds `"Invalid IAP credentials: Unable to parse JWT"` and
//! the request never reaches the pod. To accept what Gemini
//! actually sends, we drop IAP at the LB and validate the access
//! token in-process via Google's `tokeninfo` endpoint.
//!
//! Validation rules (env-driven, all required for "enforced"):
//!
//! - `GOOGLE_OAUTH_CLIENT_IDS` — comma-separated allowlist of OAuth
//!   client IDs (with or without the `.apps.googleusercontent.com`
//!   suffix). The token's `aud` / `azp` must match one of them. This
//!   is the equivalent of IAP's `programmaticClients` allowlist.
//! - `GOOGLE_OAUTH_REQUIRED_HD` — Workspace domain (e.g.
//!   `example.com`). The token's `email` suffix must match, and
//!   `email_verified` must be true.
//!
//! When `GOOGLE_OAUTH_CLIENT_IDS` is unset the middleware is a
//! pass-through (KIND / local dev). The Bearer JWT path through
//! `require_auth` continues to work for in-cluster smoke tests.
//!
//! Endpoint reference:
//! <https://oauth2.googleapis.com/tokeninfo?access_token=ACCESS_TOKEN>
//! returns a JSON body with `aud`, `azp`, `sub`, `email`,
//! `email_verified`, `exp`, `scope`. We trust the response on HTTP
//! 200; any other status (including 400 for expired / revoked
//! tokens) is treated as a rejection.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;

use crate::auth::AuthClaims;

/// Google's tokeninfo endpoint. Overridable via
/// `GOOGLE_TOKENINFO_URL` in tests.
pub const DEFAULT_TOKENINFO_URL: &str = "https://oauth2.googleapis.com/tokeninfo";

/// Middleware configuration. `Clone`-cheap (single `Arc`).
#[derive(Clone)]
pub struct GoogleOauthConfig(Arc<GoogleOauthConfigInner>);

struct GoogleOauthConfigInner {
    /// `None` ⇒ middleware is a pass-through. Populated when
    /// `GOOGLE_OAUTH_CLIENT_IDS` env is set.
    allowed_client_ids: Option<HashSet<String>>,
    /// Optional Workspace domain enforcement (`@<this>` email
    /// suffix). `None` ⇒ no domain check.
    required_hd: Option<String>,
    /// Tokeninfo endpoint. Overridable in tests.
    tokeninfo_url: String,
    /// HTTP client; pooled connections to tokeninfo cost ~50ms.
    http: reqwest::Client,
    /// Database handle, wired at mount time via [`GoogleOauthConfig::with_db`].
    /// Used to resolve the verified email to its **real** `persons.role`
    /// rather than stamping every validated token as staff. `None` only
    /// in the pass-through / unit-test configs (which never reach the
    /// role-resolution path).
    db: Option<store::Db>,
}

impl GoogleOauthConfig {
    /// Build from environment. Returns a pass-through config when
    /// `GOOGLE_OAUTH_CLIENT_IDS` is unset (the dev / KIND case).
    #[must_use]
    pub fn from_env() -> Self {
        let allowed_client_ids = std::env::var("GOOGLE_OAUTH_CLIENT_IDS")
            .ok()
            .map(|csv| csv.split(',').map(|s| s.trim().to_string()).collect());
        let required_hd = std::env::var("GOOGLE_OAUTH_REQUIRED_HD").ok();
        let tokeninfo_url =
            std::env::var("GOOGLE_TOKENINFO_URL").unwrap_or_else(|_| DEFAULT_TOKENINFO_URL.into());
        Self(Arc::new(GoogleOauthConfigInner {
            allowed_client_ids,
            required_hd,
            tokeninfo_url,
            http: reqwest::Client::new(),
            db: None,
        }))
    }

    /// Attach the database handle used to resolve the verified email to
    /// its real `persons.role`. Wired in `build_router` / the A2A
    /// router so role resolution is real in production; the
    /// pass-through and unit-test configs leave it `None`.
    #[must_use]
    pub fn with_db(self, db: store::Db) -> Self {
        let inner = &*self.0;
        Self(Arc::new(GoogleOauthConfigInner {
            allowed_client_ids: inner.allowed_client_ids.clone(),
            required_hd: inner.required_hd.clone(),
            tokeninfo_url: inner.tokeninfo_url.clone(),
            http: inner.http.clone(),
            db: Some(db),
        }))
    }

    /// Pass-through (KIND / local-dev) — middleware never blocks.
    #[must_use]
    pub fn passthrough() -> Self {
        Self(Arc::new(GoogleOauthConfigInner {
            allowed_client_ids: None,
            required_hd: None,
            tokeninfo_url: DEFAULT_TOKENINFO_URL.into(),
            http: reqwest::Client::new(),
            db: None,
        }))
    }

    /// Construct for tests with explicit values. The `tokeninfo_url`
    /// should point at a wiremock server.
    #[must_use]
    pub fn for_test(
        allowed_client_ids: impl IntoIterator<Item = impl Into<String>>,
        required_hd: Option<&str>,
        tokeninfo_url: impl Into<String>,
    ) -> Self {
        Self(Arc::new(GoogleOauthConfigInner {
            allowed_client_ids: Some(allowed_client_ids.into_iter().map(Into::into).collect()),
            required_hd: required_hd.map(str::to_string),
            tokeninfo_url: tokeninfo_url.into(),
            http: reqwest::Client::new(),
            db: None,
        }))
    }

    /// True when the middleware will challenge incoming requests.
    #[must_use]
    pub fn is_enforced(&self) -> bool {
        self.0.allowed_client_ids.is_some()
    }

    async fn verify(&self, token: &str) -> Result<TokenInfo, String> {
        let allowed = self
            .0
            .allowed_client_ids
            .as_ref()
            .ok_or("middleware not enforced")?;
        let resp = self
            .0
            .http
            .get(&self.0.tokeninfo_url)
            .query(&[("access_token", token)])
            .send()
            .await
            .map_err(|e| format!("tokeninfo request: {e}"))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("tokeninfo http {status}: {body}"));
        }
        let info: TokenInfo = serde_json::from_str(&body)
            .map_err(|e| format!("tokeninfo parse: {e}; body={body}"))?;
        // aud OR azp must match an allowlisted client; tokeninfo
        // returns both for some flows, only one for others. Normalize
        // both sides (drop the `.apps.googleusercontent.com` suffix)
        // so a bare numeric ID in the token still matches a
        // fully-qualified entry in the allowlist (and vice versa).
        let normalized_allowed: HashSet<&str> = allowed
            .iter()
            .map(|s| strip_oauth_suffix_borrowed(s))
            .collect();
        let matches_allowed = |claim: &str| -> bool {
            normalized_allowed.contains(strip_oauth_suffix_borrowed(claim))
        };
        let aud_match = info.aud.as_deref().is_some_and(matches_allowed);
        let azp_match = info.azp.as_deref().is_some_and(matches_allowed);
        if !aud_match && !azp_match {
            return Err(format!(
                "aud={:?} azp={:?} not in allowlist (size {})",
                info.aud,
                info.azp,
                allowed.len()
            ));
        }
        let verified = matches!(info.email_verified.as_deref(), Some("true" | "True"));
        if !verified {
            return Err(format!(
                "email_verified={:?} (need \"true\")",
                info.email_verified
            ));
        }
        Ok(info)
    }
}

fn strip_oauth_suffix_borrowed(s: &str) -> &str {
    s.trim_end_matches(".apps.googleusercontent.com")
}

/// Subset of Google's tokeninfo JSON. Most fields are strings even
/// when they represent booleans / numbers — that's what the API
/// returns. We keep the schema permissive to survive Google's
/// evolution of optional fields.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenInfo {
    pub aud: Option<String>,
    pub azp: Option<String>,
    pub sub: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Axum middleware. When `GOOGLE_OAUTH_CLIENT_IDS` is unset, passes
/// through — `require_auth` then handles the Bearer-JWT path used by
/// KIND. When configured, every request must carry an
/// `Authorization: Bearer <google-access-token>` header that
/// tokeninfo validates.
pub async fn require_google_oauth(
    State(cfg): State<GoogleOauthConfig>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !cfg.is_enforced() {
        return Ok(next.run(req).await);
    }
    let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        tracing::warn!("google_oauth: missing Authorization: Bearer header; returning 401");
        return Err(StatusCode::UNAUTHORIZED);
    };
    let info = match cfg.verify(token).await {
        Ok(i) => i,
        Err(reason) => {
            tracing::warn!(reason = %reason, "google_oauth: tokeninfo rejected token; returning 401");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };
    if let Some(required) = cfg.0.required_hd.as_deref() {
        let suffix = format!("@{required}");
        let email_ok = info.email.as_deref().is_some_and(|e| e.ends_with(&suffix));
        if !email_ok {
            tracing::warn!(
                required_hd = required,
                got_email = ?info.email,
                "google_oauth: email-domain mismatch; returning 403"
            );
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let email = info
        .email
        .clone()
        .unwrap_or_else(|| info.sub.clone().unwrap_or_default());
    // Resolve the caller's REAL tier from `persons.role`. A valid Google
    // token from the allowlisted client/domain is an *identity*, not an
    // authorization: it does not by itself confer staff access. An email
    // with no Neon Law Navigator account (or a client-tier one) gets `Client`, and
    // the OPA staff-gate on `/mcp` + `/api/aida/rpc` then denies it.
    // Operators must seed legitimate agent identities as staff/admin in
    // `persons`, exactly as for the browser/CLI paths.
    let role = resolve_role(cfg.0.db.as_ref(), &email).await;
    if role == store::entity::person::Role::Client {
        tracing::warn!(
            target: "audit",
            event = "google_oauth.role.client_or_unknown",
            email = %email,
            "google_oauth: validated token resolved to client/unknown tier — staff-gated routes will deny",
        );
    }
    let auth = AuthClaims {
        sub: email,
        // tokeninfo's `exp` would be useful for caching later;
        // for the per-request check the http 200 itself suffices.
        exp: 0,
        role,
    };
    req.extensions_mut().insert(auth);
    Ok(next.run(req).await)
}

/// Resolve `email` to its `persons.role`. Returns `Client` (the
/// least-privileged tier) when the db is absent or no row matches — the
/// secure default, so a missing account never yields staff access.
async fn resolve_role(db: Option<&store::Db>, email: &str) -> store::entity::person::Role {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let Some(db) = db else {
        return store::entity::person::Role::Client;
    };
    store::entity::person::Entity::find()
        .filter(store::entity::person::Column::Email.eq(email))
        .one(db)
        .await
        .ok()
        .flatten()
        .map_or(store::entity::person::Role::Client, |p| p.role)
}

#[cfg(test)]
mod tests {
    use super::{require_google_oauth, GoogleOauthConfig};
    use crate::auth::AuthClaims;
    use axum::body::Body;
    use axum::extract::Extension;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use serde_json::json;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ALLOWED_CLIENT: &str =
        "123456789012-abcdefghijklmnopqrstuvwxyzabcdef.apps.googleusercontent.com";

    async fn handler(Extension(claims): Extension<AuthClaims>) -> String {
        claims.sub
    }

    fn app(cfg: GoogleOauthConfig) -> Router {
        Router::new().route("/protected", get(handler)).route_layer(
            axum::middleware::from_fn_with_state(cfg, require_google_oauth),
        )
    }

    async fn call(app: Router, token: Option<&str>) -> axum::response::Response {
        let mut b = Request::builder().uri("/protected");
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        app.oneshot(b.body(Body::empty()).unwrap()).await.unwrap()
    }

    fn mock_url(server: &MockServer) -> String {
        format!("{}/tokeninfo", server.uri())
    }

    #[tokio::test]
    async fn valid_token_with_allowed_aud_and_verified_email_passes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .and(query_param("access_token", "abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "aud": ALLOWED_CLIENT,
                "azp": ALLOWED_CLIENT,
                "sub": "12345",
                "email": "libra@example.com",
                "email_verified": "true",
                "scope": "openid email"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("abc")).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"libra@example.com");
    }

    #[tokio::test]
    async fn passthrough_when_no_client_ids_configured() {
        let cfg = GoogleOauthConfig::passthrough();
        let resp = call(app(cfg), None).await;
        // Pass-through → handler runs without AuthClaims → 500.
        // The key signal: NOT 401.
        assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_bearer_is_unauthorized() {
        let server = MockServer::start().await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tokeninfo_400_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid_token"))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("bogus")).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn aud_not_in_allowlist_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "aud": "999999999.apps.googleusercontent.com",
                "azp": "999999999.apps.googleusercontent.com",
                "sub": "x",
                "email": "x@example.com",
                "email_verified": "true"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("abc")).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn email_unverified_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "aud": ALLOWED_CLIENT,
                "sub": "x",
                "email": "x@example.com",
                "email_verified": "false"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("abc")).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_email_domain_is_forbidden() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "aud": ALLOWED_CLIENT,
                "sub": "x",
                "email": "intruder@evil.example",
                "email_verified": "true"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("abc")).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn resolve_role_reads_real_tier_and_defaults_unknown_to_client() {
        use super::resolve_role;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        use store::entity::person::{self, Role};

        let db = store::test_support::pg().await;
        for (email, role) in [
            ("staff@example.com", Role::Staff),
            ("cli@example.com", Role::Client),
        ] {
            person::ActiveModel {
                name: Set(email.into()),
                email: Set(email.into()),
                oidc_subject: Set(None),
                role: Set(role),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        assert_eq!(
            resolve_role(Some(&db), "staff@example.com").await,
            Role::Staff
        );
        assert_eq!(
            resolve_role(Some(&db), "cli@example.com").await,
            Role::Client
        );
        // Unknown email and absent db both fall back to the least
        // privilege — never staff.
        assert_eq!(
            resolve_role(Some(&db), "nobody@example.com").await,
            Role::Client
        );
        assert_eq!(resolve_role(None, "anyone@example.com").await, Role::Client);
    }

    #[tokio::test]
    async fn aud_without_apps_googleusercontent_suffix_also_matches() {
        // Some flows return the bare numeric client_id; the
        // allowlist normalization should treat them as equivalent.
        let bare = "123456789012-abcdefghijklmnopqrstuvwxyzabcdef";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "aud": bare,
                "azp": bare,
                "sub": "x",
                "email": "libra@example.com",
                "email_verified": "true"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let cfg =
            GoogleOauthConfig::for_test([ALLOWED_CLIENT], Some("example.com"), mock_url(&server));
        let resp = call(app(cfg), Some("abc")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
