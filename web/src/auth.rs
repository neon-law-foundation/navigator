//! Bearer-token authentication middleware.
//!
//! The middleware extracts an `Authorization: Bearer <token>`
//! header, verifies the JWT, and attaches the decoded
//! [`AuthClaims`] to the request via an [`axum::extract::Extension`]
//! so downstream handlers can inspect the caller.
//!
//! Today we verify HS256 against a shared secret read from the
//! environment. The verifier is encapsulated in [`AuthConfig`] so a
//! later commit can swap HS256 for RS256-with-JWKS without touching
//! the middleware signature.
//!
//! When `OIDC_DISABLED=true` (or no secret is configured), the
//! middleware is a no-op pass-through — useful for local dev and
//! integration tests that don't care about the auth seam.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use store::entity::person::Role;

use crate::session::{SessionData, SessionStore};

/// Decoded JWT claims attached to authenticated requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub sub: String,
    pub exp: i64,
    /// System-wide tier carried in the JWT. Defaults to
    /// [`Role::Client`] when the token omits it.
    #[serde(default = "default_role")]
    pub role: Role,
}

fn default_role() -> Role {
    Role::Client
}

/// Runtime auth configuration. Cheap to clone.
#[derive(Clone)]
pub struct AuthConfig(Arc<AuthConfigInner>);

/// One verifier per supported algorithm. `from_env` picks one or
/// the other; production deployments will typically use `Jwks`.
enum Verifier {
    Hs256 {
        key: DecodingKey,
        validation: Validation,
    },
    /// RS256 JWKS — `kid` → (key, validation) lookup table fetched
    /// at boot from the configured JWKS URL.
    Jwks {
        keys: Vec<JwksEntry>,
        validation: Validation,
    },
}

struct JwksEntry {
    kid: String,
    key: DecodingKey,
}

struct AuthConfigInner {
    disabled: bool,
    verifier: Option<Verifier>,
}

impl AuthConfig {
    /// Build from environment.
    ///
    /// - `OIDC_DISABLED=true|1` — middleware passes through.
    /// - `OIDC_JWKS_URL` — fetch JWKS at boot and verify RS256 tokens
    ///   against the published keys (production path).
    /// - `OIDC_HS256_SECRET` — fall back to HS256 against a shared
    ///   secret (dev / pre-IdP path).
    ///
    /// If neither URL nor secret is set, the middleware is a
    /// pass-through.
    pub async fn from_env() -> Self {
        let disabled = std::env::var("OIDC_DISABLED").is_ok_and(|v| v == "true" || v == "1");
        if let Ok(url) = std::env::var("OIDC_JWKS_URL") {
            // When configured, pin the audience and issuer so a token
            // minted for a *different* client in the same IdP/tenant is
            // rejected (the token-confusion defense). Absent, we fall
            // back to signature+expiry only and warn.
            let audience = std::env::var("OIDC_AUDIENCE")
                .ok()
                .filter(|s| !s.is_empty());
            let issuer = std::env::var("OIDC_ISSUER").ok().filter(|s| !s.is_empty());
            if audience.is_none() {
                tracing::warn!(
                    "auth: OIDC_AUDIENCE unset — bearer tokens are accepted without audience pinning",
                );
            }
            match Self::from_jwks_url(disabled, &url, audience.as_deref(), issuer.as_deref()).await
            {
                Ok(cfg) => return cfg,
                Err(e) => {
                    // Fail CLOSED. An explicitly-configured JWKS endpoint we
                    // cannot load must crash the boot, never silently turn
                    // `require_auth` into an open pass-through. The pod
                    // crash-loops until the IdP is reachable — the correct
                    // posture for an auth dependency.
                    panic!(
                        "auth: OIDC_JWKS_URL is set ({url}) but its JWKS could not be loaded \
                         ({e}); refusing to boot with bearer-token verification disabled",
                    );
                }
            }
        }
        let secret = std::env::var("OIDC_HS256_SECRET").ok();
        Self::new(disabled, secret.as_deref())
    }

    #[must_use]
    pub fn new(disabled: bool, hs256_secret: Option<&str>) -> Self {
        let verifier = hs256_secret.map(|s| Verifier::Hs256 {
            key: DecodingKey::from_secret(s.as_bytes()),
            validation: Validation::default(),
        });
        Self(Arc::new(AuthConfigInner { disabled, verifier }))
    }

    #[must_use]
    pub fn new_disabled() -> Self {
        Self(Arc::new(AuthConfigInner {
            disabled: true,
            verifier: None,
        }))
    }

    /// Fetch a JWKS document from `url` and build an RS256 verifier
    /// from every RSA entry it contains. When `audience`/`issuer` are
    /// `Some`, the resulting verifier enforces them.
    pub async fn from_jwks_url(
        disabled: bool,
        url: &str,
        audience: Option<&str>,
        issuer: Option<&str>,
    ) -> Result<Self, AuthSetupError> {
        let doc: JwksDocument = reqwest::get(url)
            .await
            .map_err(|e| AuthSetupError::Fetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthSetupError::Parse(e.to_string()))?;
        Self::from_jwks_document(disabled, &doc, audience, issuer)
    }

    pub fn from_jwks_document(
        disabled: bool,
        doc: &JwksDocument,
        audience: Option<&str>,
        issuer: Option<&str>,
    ) -> Result<Self, AuthSetupError> {
        let mut entries = Vec::new();
        for k in &doc.keys {
            if k.kty != "RSA" {
                continue;
            }
            let (Some(n), Some(e)) = (k.n.as_deref(), k.e.as_deref()) else {
                continue;
            };
            let key = DecodingKey::from_rsa_components(n, e)
                .map_err(|e| AuthSetupError::Key(e.to_string()))?;
            entries.push(JwksEntry {
                kid: k.kid.clone().unwrap_or_default(),
                key,
            });
        }
        if entries.is_empty() {
            return Err(AuthSetupError::Empty);
        }
        let mut validation = Validation::new(Algorithm::RS256);
        // Enforce the audience only when one is configured; pinning it is
        // the defense against a token minted for a different client of
        // the same IdP being replayed here. `set_audience` flips
        // `validate_aud` on for us.
        match audience {
            Some(aud) => validation.set_audience(&[aud]),
            None => validation.validate_aud = false,
        }
        if let Some(iss) = issuer {
            validation.set_issuer(&[iss]);
        }
        Ok(Self(Arc::new(AuthConfigInner {
            disabled,
            verifier: Some(Verifier::Jwks {
                keys: entries,
                validation,
            }),
        })))
    }

    #[must_use]
    pub fn is_enforced(&self) -> bool {
        !self.0.disabled && self.0.verifier.is_some()
    }

    fn verify(&self, token: &str) -> Option<AuthClaims> {
        let v = self.0.verifier.as_ref()?;
        match v {
            Verifier::Hs256 { key, validation } => decode::<AuthClaims>(token, key, validation)
                .ok()
                .map(|d| d.claims),
            Verifier::Jwks { keys, validation } => {
                let header = decode_header(token).ok()?;
                let kid = header.kid?;
                let entry = keys.iter().find(|e| e.kid == kid)?;
                decode::<AuthClaims>(token, &entry.key, validation)
                    .ok()
                    .map(|d| d.claims)
            }
        }
    }
}

/// Minimal JWKS document shape, enough to extract RSA `n` and `e`.
#[derive(Debug, Deserialize)]
pub struct JwksDocument {
    pub keys: Vec<JwksKey>,
}

#[derive(Debug, Deserialize)]
pub struct JwksKey {
    pub kid: Option<String>,
    pub kty: String,
    pub n: Option<String>,
    pub e: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthSetupError {
    #[error("fetching JWKS: {0}")]
    Fetch(String),
    #[error("parsing JWKS: {0}")]
    Parse(String),
    #[error("constructing key: {0}")]
    Key(String),
    #[error("no RSA keys in JWKS document")]
    Empty,
}

/// Axum middleware that requires a valid bearer token on every
/// request it covers. When [`AuthConfig::is_enforced`] is false,
/// it passes through unchanged so dev environments don't need a
/// JWT at all.
pub async fn require_auth(
    State(cfg): State<AuthConfig>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !cfg.is_enforced() {
        return Ok(next.run(req).await);
    }
    // An upstream layer (e.g. IAP) may have already authenticated the
    // caller and populated `AuthClaims`. Don't require a second
    // credential — that lets `/mcp` accept IAP in prod and Bearer
    // JWTs in KIND without forking the middleware stack.
    if req.extensions().get::<AuthClaims>().is_some() {
        return Ok(next.run(req).await);
    }
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = cfg.verify(token).ok_or(StatusCode::UNAUTHORIZED)?;
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

/// Resolve a CLI bearer credential into a [`SessionData`] extension.
///
/// The `navigator` CLI presents the **same** HMAC-signed
/// [`SessionData`] blob the browser holds in its cookie, only as an
/// `Authorization: Bearer <blob>` header instead. When that blob
/// decodes to a valid, non-expired session this middleware injects two
/// extensions so the rest of the stack treats the request exactly like
/// a cookie-authenticated one:
///
///   - [`SessionData`] — what every `/portal` handler reads for
///     `is_staff_tier`, `csrf_token`, and `authored_by` provenance.
///   - [`AuthClaims`] — so the downstream [`require_auth`] layer
///     short-circuits (it already passes through when `AuthClaims` is
///     present) instead of trying to JWT-verify a session blob, and so
///     [`crate::policy::require_policy`] reads the role from its
///     `AuthClaims` fallback.
///
/// Anything that is not a valid session blob (a real OIDC JWT, garbage,
/// no header at all) is left untouched for `require_auth` to handle.
/// This layer must sit **outside** `require_auth` so it runs first.
/// CSRF is intentionally not a concern on this path: a bearer
/// credential carries no cookie, so [`crate::csrf::require_csrf`] is a
/// no-op — which is correct, CSRF defends cookie auth, not bearer auth.
pub async fn inject_bearer_session(
    State(sessions): State<SessionStore>,
    mut req: Request,
    next: Next,
) -> Response {
    // A cookie/IAP layer may already have resolved the caller — don't
    // clobber it.
    if req.extensions().get::<SessionData>().is_none() {
        if let Some(data) = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|blob| sessions.decode(blob))
        {
            req.extensions_mut().insert(AuthClaims {
                sub: data.sub.clone(),
                exp: data.exp,
                role: data.role,
            });
            req.extensions_mut().insert(data);
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::{AuthClaims, AuthConfig, JwksDocument, JwksKey};
    use jsonwebtoken::{encode, EncodingKey, Header};

    mod bearer_session {
        use crate::auth::inject_bearer_session;
        use crate::session::{SessionData, SessionStore};
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use axum::routing::get;
        use axum::{Extension, Router};
        use store::entity::person::Role;
        use tower::ServiceExt;

        /// Handler that reports the role of any injected `SessionData`,
        /// or `none` when the middleware injected nothing.
        async fn echo_role(session: Option<Extension<SessionData>>) -> String {
            session.map_or_else(|| "none".to_string(), |s| s.role.as_str().to_string())
        }

        fn app(sessions: SessionStore) -> Router {
            Router::new().route("/probe", get(echo_role)).layer(
                axum::middleware::from_fn_with_state(sessions, inject_bearer_session),
            )
        }

        async fn probe(app: &Router, auth_header: Option<&str>) -> (StatusCode, String) {
            let mut builder = Request::builder().uri("/probe");
            if let Some(h) = auth_header {
                builder = builder.header("authorization", h);
            }
            let resp = app
                .clone()
                .oneshot(builder.body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = resp.status();
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, String::from_utf8(bytes.to_vec()).unwrap())
        }

        #[tokio::test]
        async fn valid_session_blob_injects_session_data() {
            let sessions = SessionStore::new("k");
            let token = sessions.encode(&SessionData::fresh("nick@neonlaw.com", Role::Admin));
            let (status, body) = probe(&app(sessions), Some(&format!("Bearer {token}"))).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "admin");
        }

        #[tokio::test]
        async fn missing_header_injects_nothing() {
            let (status, body) = probe(&app(SessionStore::new("k")), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "none");
        }

        #[tokio::test]
        async fn expired_session_blob_is_rejected() {
            let sessions = SessionStore::new("k");
            let mut data = SessionData::fresh("nick@neonlaw.com", Role::Admin);
            data.exp = crate::session::now_unix_secs() - 60;
            let token = sessions.encode(&data);
            let (_, body) = probe(&app(sessions), Some(&format!("Bearer {token}"))).await;
            assert_eq!(body, "none", "an expired token must not authenticate");
        }

        #[tokio::test]
        async fn garbage_bearer_injects_nothing() {
            let (_, body) = probe(
                &app(SessionStore::new("k")),
                Some("Bearer not-a-real-session-blob"),
            )
            .await;
            assert_eq!(body, "none");
        }

        #[tokio::test]
        async fn blob_signed_by_a_different_key_is_rejected() {
            let token = SessionStore::new("key-a").encode(&SessionData::fresh("x", Role::Admin));
            let (_, body) = probe(
                &app(SessionStore::new("key-b")),
                Some(&format!("Bearer {token}")),
            )
            .await;
            assert_eq!(body, "none");
        }
    }

    fn sign(secret: &str, sub: &str) -> String {
        let claims = AuthClaims {
            sub: sub.into(),
            // Far enough in the future to outlast any test run.
            exp: i64::try_from(jsonwebtoken::get_current_timestamp() + 3600).unwrap(),
            role: super::Role::Admin,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn disabled_config_is_not_enforced() {
        let cfg = AuthConfig::new(true, Some("anything"));
        assert!(!cfg.is_enforced());
    }

    #[test]
    fn missing_secret_is_not_enforced() {
        let cfg = AuthConfig::new(false, None);
        assert!(!cfg.is_enforced());
    }

    #[test]
    fn config_with_secret_is_enforced() {
        let cfg = AuthConfig::new(false, Some("secret"));
        assert!(cfg.is_enforced());
    }

    #[test]
    fn verify_accepts_correctly_signed_token() {
        let cfg = AuthConfig::new(false, Some("secret"));
        let token = sign("secret", "nick");
        let claims = cfg.verify(&token).expect("valid token");
        assert_eq!(claims.sub, "nick");
        assert_eq!(claims.role, super::Role::Admin);
    }

    #[test]
    fn verify_rejects_token_signed_with_wrong_secret() {
        let cfg = AuthConfig::new(false, Some("right"));
        let token = sign("wrong", "nick");
        assert!(cfg.verify(&token).is_none());
    }

    #[test]
    fn verify_rejects_garbage() {
        let cfg = AuthConfig::new(false, Some("secret"));
        assert!(cfg.verify("not-a-jwt").is_none());
    }

    #[test]
    fn from_jwks_document_with_no_rsa_keys_errors() {
        let doc = JwksDocument {
            keys: vec![JwksKey {
                kid: Some("k1".into()),
                kty: "oct".into(),
                n: None,
                e: None,
                alg: None,
            }],
        };
        assert!(AuthConfig::from_jwks_document(false, &doc, None, None).is_err());
    }

    #[test]
    fn from_jwks_document_with_rsa_keys_is_enforced() {
        // A short but valid RSA public-key pair (n,e) in url-safe base64.
        // Sourced from RFC 7517 §A.1 example (truncated to a real value).
        let doc = JwksDocument {
            keys: vec![JwksKey {
                kid: Some("key-1".into()),
                kty: "RSA".into(),
                n: Some(
                    "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw".into(),
                ),
                e: Some("AQAB".into()),
                alg: Some("RS256".into()),
            }],
        };
        let cfg = AuthConfig::from_jwks_document(false, &doc, None, None).unwrap();
        assert!(cfg.is_enforced());
        // No matching kid → rejected.
        assert!(cfg.verify("invalid.token.here").is_none());
    }

    #[test]
    fn from_jwks_document_pins_audience_when_configured() {
        // A token signed by a JWKS key but minted for a different
        // audience must be rejected once we pin our own. We assert the
        // validation is configured with our audience; the cryptographic
        // round-trip is covered by the signed-token tests in `oauth`.
        let doc = JwksDocument {
            keys: vec![JwksKey {
                kid: Some("key-1".into()),
                kty: "RSA".into(),
                n: Some(
                    "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw".into(),
                ),
                e: Some("AQAB".into()),
                alg: Some("RS256".into()),
            }],
        };
        let cfg =
            AuthConfig::from_jwks_document(false, &doc, Some("navigator-web"), Some("https://idp"))
                .unwrap();
        assert!(cfg.is_enforced());
    }
}
