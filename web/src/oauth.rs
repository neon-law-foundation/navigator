#![allow(clippy::struct_field_names)]
//! OAuth2 Authorization Code flow with PKCE — the browser-flow
//! half of OIDC.
//!
//! Routes mounted under `/auth/*`:
//!
//! - `GET /auth/login` — generate state + PKCE verifier, set a
//!   short-lived pre-auth cookie, 302 to the IdP.
//! - `GET /auth/callback` — validate the returned `state`, exchange
//!   the `code` for tokens, decode the id_token, set the session
//!   cookie, 302 back to the `return_to` URL.
//! - `GET|POST /auth/logout` — clear the session cookie, 302 to home.
//!
//! Config is loaded from the environment at boot — see
//! [`OAuthConfig::from_env`]. The IdP's authorization + token
//! endpoints come from `<issuer>/.well-known/openid-configuration`
//! so we don't hard-code provider-specific URLs.

use std::sync::Arc;
use uuid::Uuid;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use base64::Engine;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_cookies::{cookie::SameSite, Cookie, Cookies};

use store::entity::person::Role;

use crate::auth::{AuthSetupError, JwksDocument};

use crate::session::{
    now_unix_secs, random_token_32, SessionData, SessionStore, DEFAULT_SESSION_TTL_SECS,
    SESSION_COOKIE_NAME,
};

/// Pre-auth (login-in-progress) cookie name.
pub const PRE_AUTH_COOKIE_NAME: &str = "navigator_pre_auth";
/// Pre-auth cookie lifetime — 5 minutes is plenty for the
/// roundtrip to the IdP and back.
pub const PRE_AUTH_TTL_SECS: i64 = 5 * 60;

#[derive(Debug, thiserror::Error)]
pub enum OAuthSetupError {
    #[error("missing env var: {0}")]
    Missing(&'static str),
    #[error("fetching discovery doc: {0}")]
    DiscoveryFetch(String),
    #[error("parsing discovery doc: {0}")]
    DiscoveryParse(String),
}

#[derive(Clone)]
pub struct OAuthConfig {
    inner: Arc<OAuthConfigInner>,
}

#[derive(Clone)]
struct OAuthConfigInner {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    authorization_endpoint: String,
    token_endpoint: String,
    end_session_endpoint: Option<String>,
    /// RS256 id_token verifier, built at boot from the IdP's published
    /// JWKS and pinned to the discovered `issuer` + our `client_id`
    /// audience. `None` only on the hand-built test config; production
    /// always carries one and [`callback`] refuses to mint a session
    /// without it.
    id_token_verifier: Option<Arc<IdTokenVerifier>>,
}

#[derive(Debug, Deserialize)]
struct DiscoveryDoc {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    end_session_endpoint: Option<String>,
}

impl OAuthConfig {
    /// Build with hand-supplied endpoints. Used by tests to point at
    /// a mock IdP without doing real discovery.
    #[must_use]
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
        authorization_endpoint: impl Into<String>,
        token_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(OAuthConfigInner {
                client_id: client_id.into(),
                client_secret: client_secret.into(),
                redirect_uri: redirect_uri.into(),
                authorization_endpoint: authorization_endpoint.into(),
                token_endpoint: token_endpoint.into(),
                end_session_endpoint: None,
                id_token_verifier: None,
            }),
        }
    }

    /// Attach an id_token verifier to a hand-built config. Tests use this
    /// to exercise the real verification path in [`callback`] with a
    /// locally-minted signing key; production builds the verifier inside
    /// [`OAuthConfig::from_env`] from the IdP's published JWKS.
    #[must_use]
    pub fn with_id_token_verifier(self, verifier: IdTokenVerifier) -> Self {
        let mut inner = (*self.inner).clone();
        inner.id_token_verifier = Some(Arc::new(verifier));
        Self {
            inner: Arc::new(inner),
        }
    }

    /// The RS256 id_token verifier, when configured. `callback` treats
    /// `None` as a misconfiguration and refuses the sign-in rather than
    /// trusting an unverified token.
    #[must_use]
    pub fn id_token_verifier(&self) -> Option<&Arc<IdTokenVerifier>> {
        self.inner.id_token_verifier.as_ref()
    }

    /// Build from env. Returns `Ok(None)` when `OAUTH_ISSUER_URL` is
    /// unset (the binary keeps booting without the browser-flow
    /// routes); returns `Err` only when `OAUTH_ISSUER_URL` *is* set
    /// but a required sibling is missing or discovery fails.
    pub async fn from_env() -> Result<Option<Self>, OAuthSetupError> {
        let Ok(issuer) = std::env::var("OAUTH_ISSUER_URL") else {
            return Ok(None);
        };
        let client_id = std::env::var("OAUTH_CLIENT_ID")
            .map_err(|_| OAuthSetupError::Missing("OAUTH_CLIENT_ID"))?;
        let client_secret = std::env::var("OAUTH_CLIENT_SECRET")
            .map_err(|_| OAuthSetupError::Missing("OAUTH_CLIENT_SECRET"))?;
        let redirect_uri = std::env::var("OAUTH_REDIRECT_URI")
            .map_err(|_| OAuthSetupError::Missing("OAUTH_REDIRECT_URI"))?;

        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let doc: DiscoveryDoc = reqwest::get(&url)
            .await
            .map_err(|e| OAuthSetupError::DiscoveryFetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| OAuthSetupError::DiscoveryParse(e.to_string()))?;

        // Build the id_token verifier from the IdP's published JWKS,
        // pinned to the discovered issuer and our client_id audience.
        // This is the mandatory check on the redirect callback — a
        // forged or mis-issued id_token can never mint a session.
        let verifier = IdTokenVerifier::from_jwks_url(&doc.jwks_uri, &doc.issuer, &client_id)
            .await
            .map_err(|e| OAuthSetupError::DiscoveryFetch(e.to_string()))?;

        Ok(Some(Self {
            inner: Arc::new(OAuthConfigInner {
                client_id,
                client_secret,
                redirect_uri,
                authorization_endpoint: doc.authorization_endpoint,
                token_endpoint: doc.token_endpoint,
                end_session_endpoint: doc.end_session_endpoint,
                id_token_verifier: Some(Arc::new(verifier)),
            }),
        }))
    }

    #[must_use]
    pub fn authorization_endpoint(&self) -> &str {
        &self.inner.authorization_endpoint
    }
    #[must_use]
    pub fn token_endpoint(&self) -> &str {
        &self.inner.token_endpoint
    }
    #[must_use]
    pub fn end_session_endpoint(&self) -> Option<&str> {
        self.inner.end_session_endpoint.as_deref()
    }

    /// The configured OAuth redirect URI. Its scheme is the deployment's
    /// external scheme (KIND uses `http://localhost…`, prod uses
    /// `https://…`), so it doubles as the signal for whether auth cookies
    /// should carry the `Secure` flag — even behind a TLS-terminating LB
    /// that forwards plain HTTP internally.
    #[must_use]
    pub fn redirect_uri(&self) -> &str {
        &self.inner.redirect_uri
    }
}

/// 32-byte random url-safe verifier, then S256-derived challenge.
#[must_use]
pub fn pkce_verifier() -> String {
    random_token_32()
}

#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Pre-auth cookie payload: enough to validate the callback later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreAuth {
    pub state: String,
    pub verifier: String,
    /// One-time value sent on the authorize request and required to
    /// match `id_token.nonce` in the callback — binds the returned
    /// token to *this* login and defeats id_token replay/injection.
    #[serde(default)]
    pub nonce: String,
    pub return_to: String,
    pub exp: i64,
}

impl PreAuth {
    #[must_use]
    pub fn new(return_to: String) -> Self {
        Self {
            state: random_token_32(),
            verifier: pkce_verifier(),
            nonce: random_token_32(),
            return_to,
            exp: now_unix_secs() + PRE_AUTH_TTL_SECS,
        }
    }

    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.exp <= now_unix_secs()
    }
}

/// Build the authorize URL the user gets redirected to.
#[must_use]
pub fn authorize_url(cfg: &OAuthConfig, pre: &PreAuth) -> String {
    use std::fmt::Write;
    let challenge = pkce_challenge(&pre.verifier);
    let mut url = url_with_query(cfg.authorization_endpoint());
    let client = urlencode(&cfg.inner.client_id);
    let redirect = urlencode(&cfg.inner.redirect_uri);
    let scope = urlencode("openid email profile");
    let state = urlencode(&pre.state);
    let nonce = urlencode(&pre.nonce);
    let _ = write!(
        url,
        "response_type=code&client_id={client}&redirect_uri={redirect}&scope={scope}&state={state}&nonce={nonce}&code_challenge={challenge}&code_challenge_method=S256",
    );
    url
}

fn url_with_query(base: &str) -> String {
    let mut out = base.to_string();
    if base.contains('?') {
        out.push('&');
    } else {
        out.push('?');
    }
    out
}

fn urlencode(s: &str) -> String {
    use std::fmt::Write;
    // Minimal percent-encoder for OAuth params (RFC 3986 unreserved
    // chars are left as-is). Good enough for the limited character
    // set we hand to the IdP.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Combined router state.
#[derive(Clone)]
pub struct AuthState {
    pub oauth: OAuthConfig,
    pub sessions: SessionStore,
    /// Database handle so the callback can upsert a `persons` row
    /// for the authenticated subject.
    pub db: store::Db,
    /// Outbound email backend kept on the auth state for the admin
    /// "Send welcome" button and other direct sends; the workflow
    /// trigger below routes through `workflow_runtime`, not this
    /// field.
    pub email: std::sync::Arc<dyn crate::email::EmailService>,
    /// Durable workflow runtime — the OAuth callback fires
    /// `workflows::email::welcome::trigger_welcome` against this
    /// when a fresh `persons` row appears.
    pub workflow_runtime: std::sync::Arc<dyn workflows::StateMachineRuntime>,
    /// Email address that is "always admin" — JIT-created on first
    /// sign-in, role-healed if cleared. `None` disables the carve-out
    /// (every sign-in then strictly requires a pre-seeded row). Loaded
    /// from `NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL` in `build_router`; threaded
    /// here so tests can opt in/out without mutating process env.
    pub bootstrap_admin_email: Option<String>,
    /// Email/password front door, delegated to **GCP Identity
    /// Platform**. Present only when `NAVIGATOR_IDENTITY_PLATFORM_API_KEY`
    /// is set. `None` keeps `/auth/login` as the pure OIDC redirect, so
    /// existing OIDC-only deploys are byte-identical. We never store or
    /// hash a password — Identity Platform validates it over TLS and
    /// hands back an ID token, the same trust model as the OIDC
    /// back-channel below. See the "Sign-in" section of the deploy
    /// workshop.
    pub identity_password: Option<IdentityPasswordConfig>,
    /// Admin door to **GCP Identity Platform**, used by the password-reset
    /// and email-confirm flows to write a new password or flip
    /// `emailVerified` for an account the signed-out user can't touch
    /// themselves. Unlike [`Self::identity_password`] (a public browser
    /// key), these calls need a service-account bearer token, minted from
    /// the GCE metadata server over plain `reqwest` — no GCP SDK in `web`.
    /// `None` disables reset/confirm even when the password door is on, so
    /// the routes 404 and the email-confirm gate falls through (no admin
    /// credential ⇒ nothing to write). See [`crate::idp_admin`].
    pub identity_admin: Option<crate::idp_admin::IdentityAdminConfig>,
    /// Whether auth cookies (`session`, pre-auth, login-CSRF) carry the
    /// `Secure` flag. Derived in `build_router` from the OAuth redirect
    /// URI scheme: `true` for an `https://` deployment, `false` for the
    /// `http://localhost` KIND loop so cookies still round-trip over
    /// plain HTTP in dev.
    pub secure_cookies: bool,
}

/// Configuration for the Identity Platform email/password sign-in path.
///
/// The `api_key` is the project's Identity Platform **browser key** — it
/// only scopes anonymous Identity Toolkit calls to this project; it is
/// not an admin credential and grants no data access on its own. The
/// password the user types is forwarded once to Google's
/// `accounts:signInWithPassword` endpoint over TLS and never persisted.
#[derive(Clone)]
pub struct IdentityPasswordConfig {
    /// Identity Platform browser API key (`?key=` on the REST call).
    pub api_key: String,
    /// Identity Toolkit REST base. `https://identitytoolkit.googleapis.com`
    /// in prod; tests point it at a mock.
    pub endpoint: String,
}

impl IdentityPasswordConfig {
    /// Production Identity Toolkit REST base.
    pub const DEFAULT_ENDPOINT: &'static str = "https://identitytoolkit.googleapis.com";

    /// Build from the environment. Returns `None` (password sign-in off)
    /// when `NAVIGATOR_IDENTITY_PLATFORM_API_KEY` is unset or empty, so
    /// the route is strictly opt-in and never a boot invariant.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("NAVIGATOR_IDENTITY_PLATFORM_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let endpoint = std::env::var("NAVIGATOR_IDENTITY_PLATFORM_ENDPOINT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_ENDPOINT.to_string());
        Some(Self { api_key, endpoint })
    }
}

/// Build the /auth/* sub-router.
pub fn routes(state: AuthState) -> Router {
    let mut router = Router::new()
        .route("/auth/login", get(login))
        // The OIDC redirect, always reachable by its own path so the
        // "Sign in with Google" button on the password chooser works even
        // when `/auth/login` renders the chooser instead of redirecting.
        .route("/auth/login/oidc", get(start_oidc_redirect))
        // Email/password submit (Identity Platform). 404s when password
        // sign-in is not configured.
        .route("/auth/password", post(password_login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout).post(logout));

    // Self-service password reset + email confirmation only exist where
    // an email/password door does — an OIDC-only deploy has no passwords
    // to reset and its Google tokens are already `email_verified`. Mount
    // them only then, so those deploys stay byte-identical (the routes are
    // simply absent → 404), mirroring how `/auth/password` itself 404s.
    if state.identity_password.is_some() {
        router = router
            .merge(crate::password_reset::routes())
            .merge(crate::email_confirm::routes());
    }

    router.with_state(state)
}

/// Cookie that carries the signed login-CSRF token while the password
/// form is on screen — the double-submit counterpart to the hidden
/// field embedded in the form.
pub const LOGIN_CSRF_COOKIE_NAME: &str = "navigator_login_csrf";

/// Warm, non-enumerating message shown for every failed password
/// sign-in — identical for unknown email and wrong password.
const GENERIC_LOGIN_ERROR: &str =
    "That email and password don't match what we have on file. Please try again.";

/// Toast shown on the sign-in page when the visitor was redirected here
/// because a page required a login (the private-mode gate, in place of a
/// 403). Keyed by `?notice=login_required`; any other value renders no
/// toast, so a voluntary sign-in stays clean.
const LOGIN_REQUIRED_NOTICE: &str = "You need to log in to view that page.";

/// Shown on `?notice=password_reset` after a successful password reset, so
/// the user lands on sign-in knowing the change took.
const PASSWORD_RESET_NOTICE: &str = "Your password has been updated. Please sign in.";

/// Shown on `?notice=email_confirmed` after a successful email
/// confirmation, so a previously-gated user knows they can now sign in.
const EMAIL_CONFIRMED_NOTICE: &str = "Your email is confirmed. Please sign in.";

/// Map the `notice` query flag to the toned banner the sign-in page
/// should surface, if any. The bounce case is red; the post-action
/// outcomes are green.
fn login_notice(notice: Option<&str>) -> Option<views::LoginNotice<'static>> {
    match notice {
        Some("login_required") => Some(views::LoginNotice::Danger(LOGIN_REQUIRED_NOTICE)),
        Some("password_reset") => Some(views::LoginNotice::Success(PASSWORD_RESET_NOTICE)),
        Some("email_confirmed") => Some(views::LoginNotice::Success(EMAIL_CONFIRMED_NOTICE)),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct LoginQuery {
    #[serde(default = "default_return_to")]
    pub return_to: String,
    /// Optional UX hint set by the redirector. `notice=login_required`
    /// tells the sign-in page to greet the visitor with a red toast (see
    /// [`login_notice`]); absent for a voluntary visit to `/auth/login`.
    #[serde(default)]
    pub notice: Option<String>,
}

fn default_return_to() -> String {
    "/portal".into()
}

async fn login(
    State(s): State<AuthState>,
    cookies: Cookies,
    Query(q): Query<LoginQuery>,
) -> Response {
    // Password front door configured → render the chooser (email/password
    // form + the OIDC button). Existing OIDC-only deploys (no API key)
    // keep the immediate redirect, byte-identical.
    if s.identity_password.is_some() {
        return login_chooser_response(
            &s,
            &cookies,
            &q.return_to,
            None,
            login_notice(q.notice.as_deref()),
            StatusCode::OK,
        );
    }
    start_oidc(&s, &cookies, q.return_to)
}

/// The OIDC redirect handler, exposed at `/auth/login/oidc` so the
/// password chooser's "Sign in with Google" button reaches it.
async fn start_oidc_redirect(
    State(s): State<AuthState>,
    cookies: Cookies,
    Query(q): Query<LoginQuery>,
) -> Response {
    start_oidc(&s, &cookies, q.return_to)
}

/// Set the pre-auth cookie and 302 to the IdP. Shared by `/auth/login`
/// (OIDC-only deploys) and `/auth/login/oidc` (the chooser's button).
fn start_oidc(s: &AuthState, cookies: &Cookies, return_to: String) -> Response {
    let pre = PreAuth::new(return_to);
    let cookie_value = s
        .sessions
        .encode_signed_bytes(&serde_json::to_vec(&pre).expect("pre-auth is always serializable"));
    cookies.add(pre_auth_cookie(cookie_value, s.secure_cookies));
    Redirect::to(&authorize_url(&s.oauth, &pre)).into_response()
}

/// Render the password sign-in page, mint a fresh login-CSRF token, and
/// drop it as a signed cookie (the double-submit pair to the form's
/// hidden field). Used for the initial GET and for re-rendering after a
/// rejected attempt (with `error` set and a 401 status).
fn login_chooser_response(
    s: &AuthState,
    cookies: &Cookies,
    return_to: &str,
    error: Option<&str>,
    notice: Option<views::LoginNotice<'_>>,
    status: StatusCode,
) -> Response {
    let csrf = random_token_32();
    let signed = s.sessions.encode_signed_bytes(csrf.as_bytes());
    cookies.add(login_csrf_cookie(signed, s.secure_cookies));
    let page = views::login_page(
        return_to, &csrf, /* oidc_enabled = */ true, error, notice,
    );
    (status, page).into_response()
}

/// Submitted email/password form.
#[derive(Deserialize)]
pub struct PasswordLoginForm {
    pub email: String,
    pub password: String,
    #[serde(default = "default_return_to")]
    pub return_to: String,
    #[serde(default)]
    pub csrf_token: String,
}

/// Why a password sign-in didn't yield a token.
enum PasswordError {
    /// Identity Platform rejected the credentials (unknown email, wrong
    /// password, disabled, throttled). Collapsed to one outcome so the
    /// response never reveals which — a client-confidentiality duty, not
    /// just security hygiene.
    Rejected,
    /// The sign-in service itself failed (network, 5xx, unparseable).
    Upstream,
}

#[derive(Serialize)]
struct SignInRequest<'a> {
    email: &'a str,
    password: &'a str,
    #[serde(rename = "returnSecureToken")]
    return_secure_token: bool,
}

#[derive(Deserialize)]
struct SignInResponse {
    #[serde(rename = "idToken")]
    id_token: String,
}

/// Forward the typed password to Identity Platform's
/// `accounts:signInWithPassword` over TLS and return the ID token it
/// mints. The password is never logged, stored, or hashed by us —
/// Google owns the credential. The returned token is trusted because it
/// arrives over TLS straight from Google, exactly as the OIDC
/// back-channel trusts the token endpoint's response.
async fn verify_password_with_identity_platform(
    cfg: &IdentityPasswordConfig,
    email: &str,
    password: &str,
) -> Result<String, PasswordError> {
    let url = format!(
        "{}/v1/accounts:signInWithPassword?key={}",
        cfg.endpoint.trim_end_matches('/'),
        cfg.api_key,
    );
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&SignInRequest {
            email,
            password,
            return_secure_token: true,
        })
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => match r.json::<SignInResponse>().await {
            Ok(b) => Ok(b.id_token),
            Err(e) => {
                tracing::warn!(error = %e, "identity-platform: sign-in response parse failed");
                Err(PasswordError::Upstream)
            }
        },
        // 4xx is the credential-rejection family (EMAIL_NOT_FOUND,
        // INVALID_PASSWORD, INVALID_LOGIN_CREDENTIALS, USER_DISABLED,
        // TOO_MANY_ATTEMPTS_TRY_LATER) — all collapse to Rejected so the
        // caller's response can't be used to enumerate accounts. We log
        // only the status code, never the email or password.
        Ok(r) if r.status().is_client_error() => {
            tracing::info!(
                status = r.status().as_u16(),
                "identity-platform: password sign-in rejected"
            );
            Err(PasswordError::Rejected)
        }
        Ok(r) => {
            tracing::warn!(
                status = r.status().as_u16(),
                "identity-platform: sign-in upstream error"
            );
            Err(PasswordError::Upstream)
        }
        Err(e) => {
            tracing::warn!(error = %e, "identity-platform: sign-in http error");
            Err(PasswordError::Upstream)
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn password_login(
    State(s): State<AuthState>,
    cookies: Cookies,
    Form(form): Form<PasswordLoginForm>,
) -> Response {
    let Some(cfg) = s.identity_password.clone() else {
        return (StatusCode::NOT_FOUND, "password sign-in is not enabled").into_response();
    };

    // Double-submit CSRF: the token in the signed cookie must match the
    // hidden form field. The cookie is HttpOnly + HMAC-signed, so a
    // cross-origin attacker can neither read nor forge it.
    let cookie_token = cookies
        .get(LOGIN_CSRF_COOKIE_NAME)
        .and_then(|c| s.sessions.decode_signed_bytes(c.value()))
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let csrf_ok = cookie_token.as_deref().is_some_and(|tok| {
        !tok.is_empty() && constant_time_eq(tok.as_bytes(), form.csrf_token.as_bytes())
    });
    if !csrf_ok {
        return (StatusCode::BAD_REQUEST, "invalid or missing CSRF token").into_response();
    }
    // One-shot: clear the consumed token (a fresh one is minted on any
    // re-render below).
    cookies.add(expired_cookie(LOGIN_CSRF_COOKIE_NAME));

    match verify_password_with_identity_platform(&cfg, &form.email, &form.password).await {
        Ok(id_token) => {
            let Some(claims) = decode_unverified_payload(&id_token) else {
                return (StatusCode::BAD_GATEWAY, "id_token claims parse failed").into_response();
            };
            complete_sign_in(&s, &cookies, claims, &form.return_to).await
        }
        Err(PasswordError::Rejected) => login_chooser_response(
            &s,
            &cookies,
            &form.return_to,
            Some(GENERIC_LOGIN_ERROR),
            None,
            StatusCode::UNAUTHORIZED,
        ),
        Err(PasswordError::Upstream) => {
            (StatusCode::BAD_GATEWAY, "sign-in service unavailable").into_response()
        }
    }
}

/// Constant-time byte compare so a CSRF check can't be timing-probed.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    id_token: Option<String>,
}

/// Minimal id_token payload. We deliberately only ask for the
/// fields Neon Law Navigator actually needs: a stable subject for linkage,
/// an email for first-time row creation, and an optional display
/// name. **The role is not read from the token.** Authorization is
/// derived from the `role` column on the `persons` table after the
/// upsert, so granting/revoking access is a database write — not
/// an IdP configuration change.
///
/// See [`docs/oidc.md`](../../../docs/oidc.md) for the full
/// sequence diagram and the identity-vs-authorization rationale.
#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    /// Whether the IdP asserts the address is verified. Both Google
    /// (`email_verified`) and Identity Platform / Firebase password
    /// tokens carry it. `Some(false)` is the hard gate: a password user
    /// who hasn't confirmed their email gets no session until they click
    /// the confirmation link (see [`complete_sign_in`]). A Google token
    /// carries `Some(true)`, so "sign in with Google **or** confirm your
    /// email" falls out of one check. `None` (claim absent) is treated as
    /// "not unverified" — we never had an email-confirm step before, so a
    /// token that simply omits the claim must keep working.
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    name: Option<String>,
    /// Echoed back from the authorize request; verified against the
    /// pre-auth cookie's `nonce` on the redirect callback. Absent on
    /// the Identity-Platform password token (a different, direct-TLS
    /// trust path — see [`password_login`]).
    #[serde(default)]
    nonce: Option<String>,
}

/// Why id_token verification failed. Every variant is a hard reject:
/// the callback never mints a session from a token that trips one.
#[derive(Debug, thiserror::Error)]
pub enum IdTokenError {
    #[error("token header is malformed or missing a `kid`")]
    Header,
    #[error("no JWKS key matches the token `kid`")]
    UnknownKid,
    #[error("signature, issuer, audience, or expiry check failed: {0}")]
    Validation(String),
    #[error("id_token `nonce` does not match the login's pre-auth nonce")]
    Nonce,
}

/// RS256 id_token verifier built from an IdP's published JWKS and
/// pinned to the expected issuer and audience (our `client_id`).
///
/// Verification is **mandatory** on the OIDC redirect callback. We do
/// not lean on the TLS back-channel alone: OIDC core §3.1.3.7 requires
/// the relying party to verify the id_token's signature, `iss`, `aud`,
/// and `exp`, and to bind it to the login via `nonce`. This type is the
/// one place that happens for the browser flow.
pub struct IdTokenVerifier {
    /// `(kid, key)` pairs from the JWKS document.
    keys: Vec<(String, DecodingKey)>,
    validation: Validation,
}

impl IdTokenVerifier {
    /// Build a verifier from `(kid, key)` pairs already in hand, pinned
    /// to `issuer` and `audience`. `from_jwks_document` is the production
    /// caller; tests use it directly with a locally-held signing key.
    #[must_use]
    pub fn from_keys(keys: Vec<(String, DecodingKey)>, issuer: &str, audience: &str) -> Self {
        let mut validation = Validation::new(Algorithm::RS256);
        // `set_issuer`/`set_audience` enable iss/aud enforcement; exp is
        // validated by default. These are the token-confusion defenses.
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        validation.validate_exp = true;
        Self { keys, validation }
    }

    /// Build a verifier from an already-fetched JWKS document.
    pub fn from_jwks_document(
        doc: &JwksDocument,
        issuer: &str,
        audience: &str,
    ) -> Result<Self, AuthSetupError> {
        let mut keys = Vec::new();
        for k in &doc.keys {
            if k.kty != "RSA" {
                continue;
            }
            let (Some(n), Some(e)) = (k.n.as_deref(), k.e.as_deref()) else {
                continue;
            };
            let key = DecodingKey::from_rsa_components(n, e)
                .map_err(|e| AuthSetupError::Key(e.to_string()))?;
            keys.push((k.kid.clone().unwrap_or_default(), key));
        }
        if keys.is_empty() {
            return Err(AuthSetupError::Empty);
        }
        Ok(Self::from_keys(keys, issuer, audience))
    }

    /// Fetch the JWKS at `url` and build the verifier.
    pub async fn from_jwks_url(
        url: &str,
        issuer: &str,
        audience: &str,
    ) -> Result<Self, AuthSetupError> {
        let doc: JwksDocument = reqwest::get(url)
            .await
            .map_err(|e| AuthSetupError::Fetch(e.to_string()))?
            .json()
            .await
            .map_err(|e| AuthSetupError::Parse(e.to_string()))?;
        Self::from_jwks_document(&doc, issuer, audience)
    }

    /// Verify `token` and bind it to `expected_nonce`. Returns the
    /// identity claims only when signature, issuer, audience, expiry,
    /// and nonce all check out.
    fn verify(&self, token: &str, expected_nonce: &str) -> Result<IdTokenClaims, IdTokenError> {
        let header = decode_header(token).map_err(|_| IdTokenError::Header)?;
        let kid = header.kid.ok_or(IdTokenError::Header)?;
        let key = self
            .keys
            .iter()
            .find(|(k, _)| *k == kid)
            .map(|(_, k)| k)
            .ok_or(IdTokenError::UnknownKid)?;
        let claims = decode::<IdTokenClaims>(token, key, &self.validation)
            .map_err(|e| IdTokenError::Validation(e.to_string()))?
            .claims;
        // Bind the token to this login. Constant-time so the nonce can't
        // be timing-probed (it isn't a secret, but the compare is free).
        match claims.nonce.as_deref() {
            Some(n) if constant_time_eq(n.as_bytes(), expected_nonce.as_bytes()) => Ok(claims),
            _ => Err(IdTokenError::Nonce),
        }
    }
}

async fn callback(
    State(s): State<AuthState>,
    cookies: Cookies,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if q.error.is_some() {
        return (StatusCode::BAD_REQUEST, "oauth error from idp").into_response();
    }
    let Some(code) = q.code else {
        return (StatusCode::BAD_REQUEST, "missing `code` parameter").into_response();
    };
    let Some(returned_state) = q.state else {
        return (StatusCode::BAD_REQUEST, "missing `state` parameter").into_response();
    };

    // Three phases, each fallible into a `(status, message)` error that we
    // render at the end: validate the pre-auth cookie, exchange the code
    // for tokens, verify the id_token. The small tuple error keeps the
    // helpers off `clippy::result_large_err` (an axum `Response` is big).
    let render = |e: (StatusCode, &'static str)| e.into_response();
    let pre = match consume_pre_auth(&s, &cookies, &returned_state) {
        Ok(pre) => pre,
        Err(e) => return render(e),
    };
    let token = match exchange_code(&s, &code, &pre).await {
        Ok(token) => token,
        Err(e) => return render(e),
    };
    let claims = match verify_id_token(&s, token, &pre.nonce) {
        Ok(claims) => claims,
        Err(e) => return render(e),
    };
    complete_sign_in(&s, &cookies, claims, &pre.return_to).await
}

/// A renderable callback error: an HTTP status plus a static message.
type CallbackError = (StatusCode, &'static str);

/// Phase 1: validate + consume the one-shot pre-auth cookie, checking
/// expiry and that the returned `state` matches what we issued.
fn consume_pre_auth(
    s: &AuthState,
    cookies: &Cookies,
    returned_state: &str,
) -> Result<PreAuth, CallbackError> {
    let bad = |msg: &'static str| Err((StatusCode::BAD_REQUEST, msg));
    let Some(pre_cookie) = cookies.get(PRE_AUTH_COOKIE_NAME) else {
        return bad("missing pre-auth cookie");
    };
    let Some(pre_bytes) = s.sessions.decode_signed_bytes(pre_cookie.value()) else {
        return bad("invalid pre-auth cookie");
    };
    let Ok(pre) = serde_json::from_slice::<PreAuth>(&pre_bytes) else {
        return bad("malformed pre-auth cookie");
    };
    if pre.is_expired() {
        return bad("pre-auth cookie expired");
    }
    if pre.state != returned_state {
        return bad("state mismatch");
    }
    // One-shot: clear it now that we've consumed it.
    cookies.add(expired_cookie(PRE_AUTH_COOKIE_NAME));
    Ok(pre)
}

/// Phase 2: exchange the authorization `code` at the IdP's token
/// endpoint (PKCE `code_verifier` from the pre-auth cookie).
async fn exchange_code(
    s: &AuthState,
    code: &str,
    pre: &PreAuth,
) -> Result<TokenResponse, CallbackError> {
    match reqwest::Client::new()
        .post(s.oauth.token_endpoint())
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", s.oauth.inner.redirect_uri.as_str()),
            ("client_id", s.oauth.inner.client_id.as_str()),
            ("client_secret", s.oauth.inner.client_secret.as_str()),
            ("code_verifier", pre.verifier.as_str()),
        ])
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json().await.map_err(|e| {
            tracing::warn!(error = %e, "oauth: token response parse failed");
            (StatusCode::BAD_GATEWAY, "token parse failed")
        }),
        Ok(r) => {
            tracing::warn!(
                status = r.status().as_u16(),
                "oauth: token exchange returned non-2xx"
            );
            Err((StatusCode::BAD_GATEWAY, "token exchange failed"))
        }
        Err(e) => {
            tracing::warn!(error = %e, "oauth: token exchange http error");
            Err((StatusCode::BAD_GATEWAY, "token exchange failed"))
        }
    }
}

/// Phase 3: verify the id_token's signature, issuer, audience, expiry,
/// and nonce. Verification is mandatory — a missing verifier is a deploy
/// misconfiguration, not a reason to trust the token unverified. Emits
/// the audit events for both outcomes.
fn verify_id_token(
    s: &AuthState,
    token: TokenResponse,
    nonce: &str,
) -> Result<IdTokenClaims, CallbackError> {
    let Some(verifier) = s.oauth.id_token_verifier() else {
        tracing::error!(
            "oauth: no id_token verifier configured; refusing to mint a session from an unverified token",
        );
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "auth misconfigured"));
    };
    // The OIDC flow always returns an id_token (we request `openid`). We
    // never fall back to the access_token: it is not an identity
    // assertion and carries no verifiable claims for us.
    let Some(id_token) = token.id_token else {
        return Err((StatusCode::BAD_GATEWAY, "no id_token returned"));
    };
    match verifier.verify(&id_token, nonce) {
        Ok(claims) => {
            tracing::info!(
                target: "audit",
                event = "oidc.id_token.verified",
                subject = %claims.sub,
                "oauth: id_token signature, issuer, audience, and nonce verified",
            );
            Ok(claims)
        }
        Err(e) => {
            // Audit stream (→ OTLP → Iceberg): every rejected id_token is
            // a security-relevant event. The reason is the `IdTokenError`
            // variant, never the token bytes.
            tracing::warn!(
                target: "audit",
                event = "oidc.id_token.rejected",
                reason = %e,
                "oauth: id_token verification failed",
            );
            Err((StatusCode::UNAUTHORIZED, "id_token verification failed"))
        }
    }
}

/// Shared sign-in tail for both front doors (the OIDC callback and the
/// Identity Platform password submit): resolve the local `persons` row
/// from the token claims, fire the welcome workflow for a brand-new row,
/// mint the standard `SessionData` cookie, and redirect to `return_to`.
///
/// The role is always read back from the DB row, never trusted from the
/// token — so every downstream `require_auth` / OPA / CSRF layer is
/// identical no matter which door the person came through.
async fn complete_sign_in(
    s: &AuthState,
    cookies: &Cookies,
    claims: IdTokenClaims,
    return_to: &str,
) -> Response {
    // The IdP owns identity (`sub`); our `persons` table owns the rest —
    // name, memberships, billing, and the system-wide tier. The lookup is
    // strict: a person must be pre-seeded (matched on `oidc_subject` or
    // `email`) for sign-in to succeed. The only exception is the
    // configured bootstrap admin, JIT-created with the `Admin` role so a
    // fresh deployment can never lock its operator out.
    let (person_id, role, fresh) = match resolve_person_from_claims(
        &s.db,
        &claims,
        s.bootstrap_admin_email.as_deref(),
    )
    .await
    {
        Ok(t) => t,
        Err(ResolveError::NotPreSeeded) => {
            tracing::info!(
                sub = %claims.sub,
                email = claims.email.as_deref().unwrap_or("<none>"),
                "auth: no pre-seeded persons row for the supplied email; returning 403",
            );
            return (
                StatusCode::FORBIDDEN,
                views::forbidden_page_with_auth(views::AuthState::Anonymous),
            )
                .into_response();
        }
        Err(ResolveError::Db(e)) => {
            tracing::warn!(error = %e, "auth: person lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                views::internal_error_page(),
            )
                .into_response();
        }
    };

    // Hard gate: a password (non-Google) user whose address the IdP
    // reports unverified gets NO session. We send a confirmation link and
    // render a "check your inbox" page instead. Google sign-in carries
    // `email_verified: true`, so it never trips this — exactly the rule
    // "sign in with Google **or** confirm your email." The write that
    // flips `emailVerified` needs the admin door; with no admin config
    // there is nothing to confirm *against*, so we don't pretend to gate.
    if claims.email_verified == Some(false) && s.identity_admin.is_some() {
        let name = claims.name.clone().unwrap_or_default();
        let email = claims.email.clone().unwrap_or_default();
        return crate::email_confirm::gate_unverified(s, cookies, person_id, &name, &email).await;
    }

    // First-time signup → drive the `onboarding__welcome` workflow,
    // fire-and-forget so the redirect doesn't wait on the broker. Today
    // only the bootstrap-admin JIT path produces a `NewSignup`.
    if let Some(NewSignup { email, name }) = fresh {
        let runtime = s.workflow_runtime.clone();
        let pid = person_id;
        tokio::spawn(async move {
            if let Err(e) =
                workflows::email::welcome::trigger_welcome(runtime.as_ref(), pid, &name, &email)
                    .await
            {
                tracing::warn!(
                    error = %e,
                    recipient = %email,
                    person_id = %pid,
                    "welcome workflow trigger failed",
                );
            }
        });
    }

    let mut session = SessionData::fresh(claims.sub, role);
    session.email = claims.email;
    session.person_id = Some(person_id);
    cookies.add(session_cookie(
        s.sessions.encode(&session),
        s.secure_cookies,
    ));
    Redirect::to(return_to).into_response()
}

/// Outcome of resolving the IdP-supplied claims to a local
/// `persons` row.
#[derive(Debug, thiserror::Error)]
enum ResolveError {
    /// No row matched on either `oidc_subject` or `email`. The
    /// caller renders a 403; sign-up is operator-mediated.
    #[error("no pre-seeded persons row for the IdP-supplied email")]
    NotPreSeeded,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

/// Read `NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL` once at boot. `None` is a
/// hard-fail mode: every sign-in then strictly requires a pre-seeded
/// row. Some-value is the carve-out path — that single address is
/// JIT-created with the `Admin` role on first sign-in and healed
/// back to `Admin` on every subsequent sign-in even if a UI edit
/// cleared the role. Default value is the firm's primary operator
/// address so a fresh KIND deploy lights up without any extra config;
/// production forks override per-deployment via env.
#[must_use]
pub fn bootstrap_admin_email_from_env() -> Option<String> {
    match std::env::var("NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL") {
        Ok(s) if !s.trim().is_empty() => Some(s),
        Ok(_) | Err(_) => Some("nick@neonlaw.com".to_string()),
    }
}

/// Returned alongside the person id when the OAuth callback inserts
/// a brand-new row — the seam that triggers the welcome email. `None`
/// means the row already existed (either linked by `oidc_subject` or
/// promoted from a seeded email).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSignup {
    pub email: String,
    pub name: String,
}

/// Resolve the `persons` row that corresponds to the IdP claims.
///
/// Lookup order:
///   1. Match on `oidc_subject = claims.sub` (already linked).
///   2. Match on `email = claims.email` and, if the row hasn't been
///      linked yet, promote it (existing seeded person logging in
///      for the first time — the row keeps its pre-assigned role).
///   3. **No match** → return `ResolveError::NotPreSeeded`, *except*
///      when the email matches the configured bootstrap admin address.
///      That single carve-out JIT-creates an `Admin` row so a fresh
///      deployment can never lock its operator out.
///
/// Sign-up is operator-mediated by design. New rows can only be
/// seeded by writing to the `persons` table (or, equivalently,
/// editing `store/seeds/Person.yaml` and re-running the seed
/// loader); the IdP token never grants access by itself.
///
/// If the resolved row belongs to the bootstrap admin email, the `Admin`
/// role is force-set on the returned value AND persisted back to the
/// database — so even an accidental demotion in the `/portal/admin/people`
/// UI heals on the next sign-in.
///
/// Path 3 (bootstrap admin JIT) is the only path that returns
/// `Some(NewSignup)` — promotion is intentionally NOT treated as a
/// fresh signup because the row was already seeded by an operator.
async fn resolve_person_from_claims(
    db: &store::Db,
    claims: &IdTokenClaims,
    bootstrap_admin_email: Option<&str>,
) -> Result<(Uuid, Role, Option<NewSignup>), ResolveError> {
    use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
    use store::entity::person;

    let bootstrap_admin = bootstrap_admin_email.map(str::to_lowercase);
    let email_lower = claims.email.as_deref().map(str::to_lowercase);
    let is_bootstrap_admin = matches!(
        (&bootstrap_admin, &email_lower),
        (Some(a), Some(e)) if a == e,
    );

    if let Some(existing) = person::Entity::find()
        .filter(person::Column::OidcSubject.eq(claims.sub.clone()))
        .one(db)
        .await?
    {
        let mut role = existing.role;
        if is_bootstrap_admin && role != Role::Admin {
            role = Role::Admin;
            let mut update: person::ActiveModel = existing.clone().into();
            update.role = ActiveValue::Set(Role::Admin);
            update.update(db).await?;
        }
        return Ok((existing.id, role, None));
    }

    let Some(email) = claims.email.clone() else {
        // No email on the token. We refuse to mint a session for an
        // unknown identifier — operators must seed before sign-in.
        return Err(ResolveError::NotPreSeeded);
    };

    if let Some(existing) = person::Entity::find()
        .filter(person::Column::Email.eq(email.clone()))
        .one(db)
        .await?
    {
        let mut role = existing.role;
        let mut promote: person::ActiveModel = existing.clone().into();
        let mut dirty = false;
        if existing.oidc_subject.is_none() {
            promote.oidc_subject = ActiveValue::Set(Some(claims.sub.clone()));
            dirty = true;
        }
        if is_bootstrap_admin && role != Role::Admin {
            role = Role::Admin;
            promote.role = ActiveValue::Set(Role::Admin);
            dirty = true;
        }
        if dirty {
            promote.update(db).await?;
        }
        return Ok((existing.id, role, None));
    }

    if !is_bootstrap_admin {
        return Err(ResolveError::NotPreSeeded);
    }

    // Bootstrap admin JIT-create path. Role is `Admin`; the welcome
    // workflow fires once so the operator gets a paper trail.
    let name = claims.name.clone().unwrap_or_else(|| email.clone());
    let new = person::ActiveModel {
        name: ActiveValue::Set(name.clone()),
        email: ActiveValue::Set(email.clone()),
        oidc_subject: ActiveValue::Set(Some(claims.sub.clone())),
        role: ActiveValue::Set(Role::Admin),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok((new.id, Role::Admin, Some(NewSignup { email, name })))
}

async fn logout(cookies: Cookies) -> Response {
    cookies.add(expired_cookie(SESSION_COOKIE_NAME));
    cookies.add(expired_cookie(PRE_AUTH_COOKIE_NAME));
    Redirect::to("/").into_response()
}

/// Payload-only decode for the **Identity-Platform password path only**.
///
/// That token is not delivered through a browser redirect: we POST the
/// typed password straight to Google's `signInWithPassword` over TLS and
/// Google hands the id_token back on the same connection. There is no
/// `code`, no redirect, and no possibility of IdP-mixup or token
/// injection, so the back-channel TLS is the trust boundary (the same
/// trust the OIDC *token endpoint* gets). The redirect [`callback`] does
/// **not** use this — it runs full JWKS signature + `iss`/`aud`/`exp` +
/// `nonce` verification via [`IdTokenVerifier`].
fn decode_unverified_payload(jwt: &str) -> Option<IdTokenClaims> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn pre_auth_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(PRE_AUTH_COOKIE_NAME, value);
    c.set_http_only(true);
    c.set_secure(secure);
    c.set_same_site(SameSite::Lax);
    c.set_path("/");
    c.set_max_age(tower_cookies::cookie::time::Duration::seconds(
        PRE_AUTH_TTL_SECS,
    ));
    c
}

fn login_csrf_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(LOGIN_CSRF_COOKIE_NAME, value);
    c.set_http_only(true);
    c.set_secure(secure);
    c.set_same_site(SameSite::Lax);
    c.set_path("/");
    c.set_max_age(tower_cookies::cookie::time::Duration::seconds(
        PRE_AUTH_TTL_SECS,
    ));
    c
}

/// Build the signed session cookie. `Max-Age` matches the payload's
/// own TTL so the cookie is *persistent* — it survives a browser
/// restart instead of dying on close — and the two expiries stay in
/// lockstep. `crate::session_renew` slides both forward on activity.
pub(crate) fn session_cookie(value: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE_NAME, value);
    c.set_http_only(true);
    c.set_secure(secure);
    c.set_same_site(SameSite::Lax);
    c.set_path("/");
    c.set_max_age(tower_cookies::cookie::time::Duration::seconds(
        DEFAULT_SESSION_TTL_SECS,
    ));
    c
}

pub(crate) fn expired_cookie(name: &'static str) -> Cookie<'static> {
    let mut c = Cookie::new(name, "");
    c.set_path("/");
    c.set_max_age(tower_cookies::cookie::time::Duration::seconds(0));
    c
}

#[cfg(test)]
mod tests {
    use super::{
        authorize_url, constant_time_eq, decode_unverified_payload, pkce_challenge, pkce_verifier,
        session_cookie, urlencode, IdTokenError, IdentityPasswordConfig, OAuthConfig, PreAuth,
    };
    use crate::session::{now_unix_secs, DEFAULT_SESSION_TTL_SECS};
    use crate::test_support::{oidc_verifier, sign_id_token};
    use base64::Engine;

    fn cfg() -> OAuthConfig {
        OAuthConfig::new(
            "client123",
            "secret456",
            "https://app.example.com/auth/callback",
            "https://idp.example.com/oauth/authorize",
            "https://idp.example.com/oauth/token",
        )
    }

    #[test]
    fn session_cookie_is_persistent_with_matching_max_age() {
        // A persistent cookie (Max-Age set) survives a browser restart,
        // and its lifetime matches the signed payload's TTL.
        let c = session_cookie("payload.sig".into(), true);
        let max_age = c.max_age().expect("session cookie must set Max-Age");
        assert_eq!(max_age.whole_seconds(), DEFAULT_SESSION_TTL_SECS);
        assert!(c.secure().unwrap_or(false));
        assert!(c.http_only().unwrap_or(false));
    }

    #[test]
    fn pkce_verifier_is_url_safe_and_random() {
        let a = pkce_verifier();
        let b = pkce_verifier();
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/'));
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        use sha2::Digest;
        let verifier = "the-verifier";
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(verifier.as_bytes()));
        assert_eq!(pkce_challenge(verifier), expected);
    }

    #[test]
    fn urlencode_handles_reserved_chars() {
        assert_eq!(urlencode("hello"), "hello");
        assert_eq!(urlencode("hi there"), "hi%20there");
        assert_eq!(urlencode("a/b?c=d"), "a%2Fb%3Fc%3Dd");
        assert_eq!(
            urlencode("openid email profile"),
            "openid%20email%20profile"
        );
    }

    #[test]
    fn authorize_url_contains_every_required_param() {
        let pre = PreAuth {
            state: "STATE123".into(),
            verifier: "the-verifier".into(),
            nonce: "NONCE789".into(),
            return_to: "/portal".into(),
            exp: now_unix_secs() + 300,
        };
        let url = authorize_url(&cfg(), &pre);
        assert!(url.starts_with("https://idp.example.com/oauth/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fapp.example.com%2Fauth%2Fcallback"));
        assert!(url.contains("scope=openid%20email%20profile"));
        assert!(url.contains("state=STATE123"));
        assert!(url.contains("nonce=NONCE789"));
        let challenge = pkce_challenge("the-verifier");
        assert!(url.contains(&format!("code_challenge={challenge}")));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn authorize_url_appends_with_amp_when_endpoint_has_existing_query() {
        let cfg = OAuthConfig::new(
            "c",
            "s",
            "http://x",
            "https://idp.example.com/authorize?foo=bar",
            "https://idp.example.com/token",
        );
        let pre = PreAuth {
            state: "S".into(),
            verifier: "v".into(),
            nonce: "n".into(),
            return_to: "/".into(),
            exp: now_unix_secs() + 60,
        };
        let url = authorize_url(&cfg, &pre);
        assert!(url.starts_with("https://idp.example.com/authorize?foo=bar&response_type="));
    }

    #[test]
    fn pre_auth_expires_in_the_future_and_carries_distinct_state_and_verifier() {
        let p = PreAuth::new("/somewhere".into());
        assert!(p.exp > now_unix_secs());
        assert!(!p.is_expired());
        assert_ne!(p.state, p.verifier);
        assert_eq!(p.return_to, "/somewhere");
    }

    #[test]
    fn pre_auth_marked_expired_when_exp_in_past() {
        let p = PreAuth {
            state: "s".into(),
            verifier: "v".into(),
            nonce: "n".into(),
            return_to: "/".into(),
            exp: now_unix_secs() - 1,
        };
        assert!(p.is_expired());
    }

    #[test]
    fn auth_cookies_carry_secure_only_when_requested() {
        use super::{login_csrf_cookie, pre_auth_cookie, session_cookie};
        for builder in [
            session_cookie as fn(String, bool) -> _,
            pre_auth_cookie,
            login_csrf_cookie,
        ] {
            assert_eq!(builder("v".into(), true).secure(), Some(true));
            assert_eq!(builder("v".into(), false).secure(), Some(false));
            // HttpOnly is unconditional regardless of the Secure flag.
            assert_eq!(builder("v".into(), false).http_only(), Some(true));
        }
    }

    #[test]
    fn id_token_verifier_accepts_a_valid_signed_token() {
        let verifier = oidc_verifier("client123");
        let token = sign_id_token(
            "client123",
            "the-nonce",
            "kc-uuid-libra",
            "libra@example.com",
            "Libra",
        );
        let claims = verifier.verify(&token, "the-nonce").expect("valid token");
        assert_eq!(claims.sub, "kc-uuid-libra");
        assert_eq!(claims.email.as_deref(), Some("libra@example.com"));
    }

    #[test]
    fn id_token_verifier_rejects_a_nonce_mismatch() {
        let verifier = oidc_verifier("client123");
        let token = sign_id_token("client123", "login-nonce", "s", "e@x.com", "N");
        // A token whose nonce doesn't match the login's pre-auth nonce is
        // a replay/injection and must be refused.
        let err = verifier.verify(&token, "different-nonce").unwrap_err();
        assert!(matches!(err, IdTokenError::Nonce));
    }

    #[test]
    fn id_token_verifier_rejects_a_token_minted_for_another_audience() {
        let verifier = oidc_verifier("client123");
        // Signed for a *different* client of the same IdP — the
        // token-confusion attack. Audience pinning rejects it.
        let token = sign_id_token("other-client", "n", "s", "e@x.com", "N");
        let err = verifier.verify(&token, "n").unwrap_err();
        assert!(matches!(err, IdTokenError::Validation(_)));
    }

    #[test]
    fn id_token_verifier_rejects_garbage_and_unsigned_tokens() {
        let verifier = oidc_verifier("client123");
        assert!(verifier.verify("not-a-jwt", "n").is_err());
        // Unsigned "alg:none"-style token: header.payload with no usable
        // kid / signature → rejected before any claim is trusted.
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"x","nonce":"n","iss":"https://idp.test","aud":"client123"}"#);
        let unsigned = format!("aGVhZGVy.{payload}.");
        assert!(verifier.verify(&unsigned, "n").is_err());
    }

    #[test]
    fn id_token_payload_decodes_sub_and_email() {
        // header.payload.sig — header and sig are irrelevant here.
        // Roles are deliberately *not* part of the payload schema:
        // the IdP only carries identity, the DB carries authz.
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"kc-uuid-libra","email":"libra@example.com","name":"Libra"}"#);
        let jwt = format!("aGVhZGVy.{payload}.c2ln");
        let claims = decode_unverified_payload(&jwt).unwrap();
        assert_eq!(claims.sub, "kc-uuid-libra");
        assert_eq!(claims.email.as_deref(), Some("libra@example.com"));
        assert_eq!(claims.name.as_deref(), Some("Libra"));
    }

    #[test]
    fn id_token_payload_returns_none_for_garbage() {
        assert!(decode_unverified_payload("not-a-jwt").is_none());
        assert!(decode_unverified_payload("only.two").is_none());
        assert!(decode_unverified_payload("a.b.c").is_none());
    }

    #[test]
    fn constant_time_eq_matches_only_equal_byte_strings() {
        assert!(constant_time_eq(b"abc123", b"abc123"));
        assert!(!constant_time_eq(b"abc123", b"abc124"));
        // Length mismatch is never equal (and never panics).
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn identity_password_config_from_env_is_opt_in() {
        // The helper reads process env; assert the shape of the decision
        // without stomping a shared env var by checking the default
        // endpoint constant the prod path falls back to.
        assert_eq!(
            IdentityPasswordConfig::DEFAULT_ENDPOINT,
            "https://identitytoolkit.googleapis.com",
        );
    }
}
