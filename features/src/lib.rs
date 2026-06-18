//! Shared scaffolding for the Navigator BDD feature suite.
//!
//! Each `tests/<name>.rs` runner owns its own `cucumber::World` and
//! step set; this library only carries pieces that more than one
//! runner would otherwise duplicate — an in-memory `AppState`
//! constructor, a signed-id_token OAuth driver for the OIDC
//! scenarios, and a tiny body-reader.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use store::Db;
use web::{
    policy, AppState, AuthConfig, CanonicalHost, MarketingIndex, OAuthConfig, SessionStore,
    WorkshopIndex,
};
use workflows::{DispatchingRuntime, EmailService, InMemoryRuntime, StateMachineRuntime};

pub mod journey;
pub mod template_shapes;

#[cfg(feature = "webdriver")]
pub mod webdriver;

/// Spin up a fresh per-test Postgres schema (via
/// `store::test_support::pg`), apply migrations, and hand back the
/// connection. The seed pass is left to the caller — only the
/// retainer-intake scenarios need the canonical templates.
pub async fn in_memory_db() -> Db {
    store::test_support::pg().await
}

/// Assemble an [`AppState`] suitable for `oneshot` tests. Callers
/// pass a shared `InMemoryRuntime` so they can assert on its event
/// log; the runtime stands in for both the workflow and
/// questionnaire timelines (the production binary uses two distinct
/// trait objects, but a single runtime satisfies both).
///
/// Internally allocates a fresh [`web::email::CapturingEmail`]; use
/// [`app_state_with_email`] when the scenario needs to share the
/// concrete `CapturingEmail` with assertions (e.g. counting welcome
/// emails dispatched through the workflow path).
pub fn app_state(
    db: Db,
    runtime: Arc<InMemoryRuntime>,
    storage: Arc<dyn cloud::StorageService>,
    policy_client: policy::PolicyClient,
    oauth: Option<OAuthConfig>,
    sessions: SessionStore,
) -> AppState {
    app_state_with_email(
        db,
        runtime,
        storage,
        policy_client,
        oauth,
        sessions,
        Arc::new(web::email::CapturingEmail::new()),
    )
}

/// Variant of [`app_state`] that lets the caller inject a specific
/// `EmailService` (typically a shared [`web::email::CapturingEmail`]
/// so scenarios can assert on what was dispatched). The workflow
/// timeline is wrapped in [`workflows::DispatchingRuntime`] backed
/// by the same service, mirroring the prod worker — a transition
/// into an `email_send__*` state dispatches the email inline.
pub fn app_state_with_email(
    db: Db,
    runtime: Arc<InMemoryRuntime>,
    storage: Arc<dyn cloud::StorageService>,
    policy_client: policy::PolicyClient,
    oauth: Option<OAuthConfig>,
    sessions: SessionStore,
    email: Arc<dyn EmailService>,
) -> AppState {
    // Back the dispatching runtime with the db so compliance-submission
    // and matter-close (`firm_signature__*`) side effects run in-process,
    // mirroring the prod worker.
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        DispatchingRuntime::new(runtime.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );
    AppState {
        db,
        workshops: WorkshopIndex::empty(),
        docs: web::DocsIndex::empty(),
        marketing: MarketingIndex::empty(),
        blog: web::BlogIndex::empty(),
        auth: AuthConfig::new(true, None),
        google_oauth: web::google_oauth::GoogleOauthConfig::passthrough(),
        rate_limit: web::rate_limit::RateLimit::disabled(),
        canonical_host: CanonicalHost::new(None),
        portal_only: web::PortalOnly::default(),
        sessions,
        oauth,
        storage,
        policy: policy_client,
        workflow_runtime,
        questionnaire_runtime: runtime,
        signature_provider: Arc::new(web::signature::StubSignatureProvider::new()),
        billing_provider: Arc::new(web::billing::StubBillingProvider::new()),
        contract_reviewer: Arc::new(web::contract_review::StubContractReviewer),
        esignature_webhook_secret: None,
        esignature_hmac_key: None,
        email,
        inbound_email_secret: None,
        email_events_secret: None,
        sendgrid_events_public_key: None,
        bootstrap_admin_email: None,
        identity_password: None,
        identity_admin: None,
        a2a_router: None,
    }
}

/// Stand up a filesystem-backed `StorageService` rooted in a
/// per-suite temp directory. The path includes `suite` so parallel
/// integration tests don't trample each other.
pub async fn fs_storage(suite: &str) -> Arc<dyn cloud::StorageService> {
    Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-features-{suite}")))
            .await
            .expect("create FsStorage temp root"),
    )
}

/// Drain a response body into a `String`. The Navigator handlers
/// always emit UTF-8.
pub async fn body_string(resp: Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).expect("response body is UTF-8")
}

/// The OAuth `client_id` the OIDC BDD apps register; the test
/// `id_token` verifier is pinned to it as the expected `aud`.
pub const OAUTH_CLIENT_ID: &str = "navigator-web";

/// Build the [`OAuthConfig`] the OIDC BDD suites hand to
/// [`app_state`], pointed at a wiremock `IdP` and carrying the shared
/// test `id_token` verifier (`web::test_support::oidc_verifier`) — so
/// `/auth/callback` runs the full production signature + `iss`/`aud`/
/// `nonce` verification instead of refusing with 500.
#[must_use]
pub fn verified_oauth_config(idp_uri: &str) -> OAuthConfig {
    web::test_support::oauth_config_with_verifier(
        OAuthConfig::new(
            OAUTH_CLIENT_ID,
            "navigator-web-secret",
            "http://app.test/auth/callback",
            format!("{idp_uri}/authorize"),
            format!("{idp_uri}/token"),
        ),
        OAUTH_CLIENT_ID,
    )
}

/// Drive `/auth/login` → `/auth/callback` end-to-end against `app`,
/// programming `idp`'s `/token` endpoint to return a properly-signed
/// `id_token` only once the login leg reveals the per-request `nonce`
/// (the verifier binds the token to the login via that claim, so the
/// mock cannot be mounted up-front). The `IdP` is reset first so a
/// repeat login never replays a stale-nonce token.
///
/// Returns the callback's status — `303` on a successful link, `403`
/// when the identity isn't pre-seeded (sign-up is operator-mediated).
pub async fn drive_verified_oauth(
    app: &axum::Router,
    idp: &wiremock::MockServer,
    sub: &str,
    email: &str,
    name: &str,
) -> StatusCode {
    use tower::ServiceExt;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, ResponseTemplate};

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login?return_to=/portal")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let location = login
        .headers()
        .get("location")
        .expect("login redirects to the IdP")
        .to_str()
        .unwrap()
        .to_string();
    let qp = |name: &str| {
        let needle = format!("{name}=");
        location
            .split('&')
            .find_map(|p| p.strip_prefix(&needle))
            .unwrap_or_else(|| panic!("`{name}` missing from {location}"))
            .to_string()
    };
    let state_param = qp("state");
    let nonce = qp("nonce");
    let pre_auth_cookie = login
        .headers()
        .get("set-cookie")
        .expect("login set-cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    idp.reset().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id_token": web::test_support::sign_id_token(OAUTH_CLIENT_ID, &nonce, sub, email, name),
            "token_type": "Bearer",
        })))
        .mount(idp)
        .await;

    let cb = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/auth/callback?code=any-code&state={state_param}"))
                .header("cookie", pre_auth_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    cb.status()
}

/// Tiny URL encoder for the four characters our feature payloads
/// actually contain (space and `@`). Enough for retainer answers
/// like `Libra` and `libra@example.com`; not a general
/// `application/x-www-form-urlencoded` implementation.
#[must_use]
pub fn form_encode(s: &str) -> String {
    s.replace(' ', "%20").replace('@', "%40")
}
