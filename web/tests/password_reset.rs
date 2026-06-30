#![allow(clippy::doc_markdown)]
//! End-to-end tests for the password-reset and email-confirmation flows
//! (`web::password_reset`, `web::email_confirm`).
//!
//! Passwords live in GCP Identity Platform, so these stand TWO `wiremock`
//! servers next to a real Postgres: a metadata-server stand-in that mints
//! a service-account token, and an Identity-Toolkit stand-in for the admin
//! `accounts:lookup` / `accounts:update` (and `signInWithPassword` for the
//! confirm-gate test). The flows mint our own single-use token, email the
//! link through the capturing backend, and on confirm write to Identity
//! Platform via the admin door.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use serde_json::{json, Value};
use store::entity::person;
use store::Db;
use tower::ServiceExt;
use views::assert_renders;
use web::email::{CapturingEmail, EmailService};
use web::idp_admin::IdentityAdminConfig;
use web::oauth::IdentityPasswordConfig;
use web::{policy, AppState, AuthConfig, OAuthConfig, SessionStore};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A structurally-valid but unsigned Firebase-shaped ID token carrying an
/// explicit `email_verified` flag (the confirm gate keys off it).
fn fake_id_token(sub: &str, email: &str, email_verified: bool) -> String {
    let header = b64url(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = b64url(
        serde_json::to_vec(&json!({
            "sub": sub, "email": email, "email_verified": email_verified,
        }))
        .unwrap()
        .as_slice(),
    );
    format!("{header}.{payload}.")
}

/// Metadata-server stand-in that hands back a service-account access token.
async fn mount_metadata() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/computeMetadata/v1/instance/service-accounts/default/token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ya29.fake", "expires_in": 3599, "token_type": "Bearer",
        })))
        .mount(&server)
        .await;
    server
}

/// Identity-Toolkit admin stand-in. `lookup` is returned for
/// `accounts:lookup`; `accounts:update` always 200s; `signin`, when set,
/// answers `signInWithPassword` (for the confirm-gate test).
async fn mount_idp(lookup: Value, signin: Option<Value>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo-project/accounts:lookup"))
        .respond_with(ResponseTemplate::new(200).set_body_json(lookup))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/demo-project/accounts:update"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "localId": "uid-1" })))
        .mount(&server)
        .await;
    if let Some(body) = signin {
        Mock::given(method("POST"))
            .and(path("/v1/accounts:signInWithPassword"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    server
}

/// A password account (resettable).
fn password_user(email: &str) -> Value {
    json!({ "users": [{
        "localId": "uid-1",
        "email": email,
        "emailVerified": false,
        "providerUserInfo": [{ "providerId": "password" }],
    }]})
}

/// A Google-federated account (no password to reset).
fn google_user(email: &str) -> Value {
    json!({ "users": [{
        "localId": "uid-g",
        "email": email,
        "emailVerified": true,
        "providerUserInfo": [{ "providerId": "google.com" }],
    }]})
}

/// Build an `AppState` with both the password door and the admin door
/// wired to the given mock endpoints. Returns the captured-email handle so
/// a test can read the link that was mailed.
async fn state_with_reset(
    idp: &MockServer,
    metadata: &MockServer,
    password_door: bool,
) -> (AppState, Db, Arc<CapturingEmail>) {
    let db = store::test_support::pg().await;
    let opa = MockServer::start().await;
    let captured = Arc::new(CapturingEmail::new());
    let email: Arc<dyn EmailService> = captured.clone();
    let state = AppState {
        auth: AuthConfig::new(false, None),
        sessions: SessionStore::new("test-session-key-not-for-production"),
        oauth: Some(OAuthConfig::new(
            "navigator-web",
            "secret",
            "http://app.test/auth/callback",
            "http://idp.test/authorize",
            "http://idp.test/token",
        )),
        identity_password: password_door.then(|| IdentityPasswordConfig {
            api_key: "test-browser-key".into(),
            endpoint: idp.uri(),
        }),
        identity_admin: Some(IdentityAdminConfig {
            project_id: "demo-project".into(),
            endpoint: idp.uri(),
            metadata_endpoint: metadata.uri(),
        }),
        email: email.clone(),
        storage: Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-reset-e2e"))
                .await
                .unwrap(),
        ),
        policy: policy::PolicyClient::new(opa.uri()),
        ..web::test_support::app_state(db.clone()).await
    };
    (state, db, captured)
}

async fn seed_person(db: &Db, email: &str) {
    person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set(email.into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(person::Role::Staff),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed person");
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Extract the `navigator_account_csrf` cookie value and the form's hidden
/// `csrf_token` from a rendered account-recovery page.
fn csrf_pair(resp_cookie: &str, html: &str) -> (String, String) {
    let needle = r#"name="csrf_token" value=""#;
    let start = html.find(needle).expect("form carries csrf_token") + needle.len();
    let token = html[start..].split('"').next().unwrap().to_string();
    (resp_cookie.to_string(), token)
}

fn account_csrf_cookie(resp: &axum::response::Response) -> String {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.contains("navigator_account_csrf="))
        .expect("sets the account CSRF cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn has_session(resp: &axum::response::Response) -> bool {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .any(|v| v.to_str().unwrap().contains("navigator_session="))
}

/// Pull the single-use token out of the most recent captured email body.
fn token_from_email(email: &Arc<CapturingEmail>, marker: &str) -> String {
    let captured = email.captured();
    let body = captured
        .iter()
        .rev()
        .find_map(|m| {
            m.body
                .find(marker)
                .map(|i| m.body[i + marker.len()..].to_string())
        })
        .expect("an email carrying the link was sent");
    body.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn post_form(
    app: &axum::Router,
    uri: &str,
    cookie: &str,
    body: String,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("cookie", cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn full_reset_flow_sets_a_new_password_and_signs_in() {
    let meta = mount_metadata().await;
    let idp = mount_idp(password_user("libra@example.com"), None).await;
    let (state, db, email) = state_with_reset(&idp, &meta, true).await;
    seed_person(&db, "libra@example.com").await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // 1. Open the request form, grab the CSRF pair.
    let form = get(&app, "/auth/password/reset").await;
    let cookie = account_csrf_cookie(&form);
    let (_, token) = csrf_pair(&cookie, &body_string(form).await);

    // 2. Submit the email → neutral "check your inbox", and a link is mailed.
    let submit = post_form(
        &app,
        "/auth/password/reset",
        &cookie,
        format!("email=libra%40example.com&csrf_token={token}"),
    )
    .await;
    assert_eq!(submit.status(), StatusCode::OK);
    assert_renders!(&body_string(submit).await, "portal.reset_check_inbox");
    let reset_token = token_from_email(&email, "/auth/password/reset/new?token=");

    // 3. Open the set-password form via the link.
    let new_form = get(
        &app,
        &format!("/auth/password/reset/new?token={reset_token}"),
    )
    .await;
    assert_eq!(new_form.status(), StatusCode::OK);
    let cookie = account_csrf_cookie(&new_form);
    let (_, csrf) = csrf_pair(&cookie, &body_string(new_form).await);

    // 4. Submit the new password → 303 to sign-in with the success notice.
    let confirm = post_form(
        &app,
        "/auth/password/reset/new",
        &cookie,
        format!(
            "token={reset_token}&password=brand-new-pass&confirm=brand-new-pass&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(confirm.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        confirm.headers().get("location").unwrap(),
        "/auth/login?notice=password_reset"
    );

    // 5. The token is single-use: the link no longer opens a form.
    let replay = get(
        &app,
        &format!("/auth/password/reset/new?token={reset_token}"),
    )
    .await;
    assert_renders!(&body_string(replay).await, "portal.auth_link_invalid");
}

#[tokio::test]
async fn request_for_unknown_email_is_neutral_and_sends_nothing() {
    let meta = mount_metadata().await;
    let idp = mount_idp(json!({}), None).await; // lookup finds nobody
    let (state, _db, email) = state_with_reset(&idp, &meta, true).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let form = get(&app, "/auth/password/reset").await;
    let cookie = account_csrf_cookie(&form);
    let (_, token) = csrf_pair(&cookie, &body_string(form).await);
    let submit = post_form(
        &app,
        "/auth/password/reset",
        &cookie,
        format!("email=nobody%40example.com&csrf_token={token}"),
    )
    .await;
    // Same neutral page as a real account — no enumeration.
    assert_eq!(submit.status(), StatusCode::OK);
    assert_renders!(&body_string(submit).await, "portal.reset_check_inbox");
    assert!(email.captured().is_empty(), "no mail for an unknown email");
}

#[tokio::test]
async fn request_for_a_google_user_mails_the_sign_in_with_google_notice() {
    // The reported bug: nick@neonlaw.com signed in with Google, so Identity
    // Platform holds no password to reset. The flow must not stay silent —
    // it mails a "you sign in with Google" notice instead of a reset link.
    let meta = mount_metadata().await;
    let idp = mount_idp(google_user("nick@neonlaw.com"), None).await;
    let (state, db, email) = state_with_reset(&idp, &meta, true).await;
    seed_person(&db, "nick@neonlaw.com").await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let form = get(&app, "/auth/password/reset").await;
    let cookie = account_csrf_cookie(&form);
    let (_, token) = csrf_pair(&cookie, &body_string(form).await);
    let submit = post_form(
        &app,
        "/auth/password/reset",
        &cookie,
        format!("email=nick%40neonlaw.com&csrf_token={token}"),
    )
    .await;
    // Same neutral page as every other request — no enumeration.
    assert_eq!(submit.status(), StatusCode::OK);
    assert_renders!(&body_string(submit).await, "portal.reset_check_inbox");

    // Exactly one email, and it's the Google notice — no reset link.
    let captured = email.captured();
    assert_eq!(captured.len(), 1, "one notice is mailed");
    let msg = &captured[0];
    assert_eq!(msg.to, "nick@neonlaw.com");
    assert!(
        msg.body.contains("Google"),
        "body names Google as the sign-in method: {}",
        msg.body
    );
    assert!(
        !msg.body.contains("/auth/password/reset/new?token="),
        "a Google account is never mailed a password-reset link: {}",
        msg.body
    );
}

#[tokio::test]
async fn request_with_bad_csrf_is_rejected() {
    let meta = mount_metadata().await;
    let idp = mount_idp(password_user("libra@example.com"), None).await;
    let (state, db, _email) = state_with_reset(&idp, &meta, true).await;
    seed_person(&db, "libra@example.com").await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = post_form(
        &app,
        "/auth/password/reset",
        "navigator_account_csrf=tampered",
        "email=libra%40example.com&csrf_token=".to_string(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_unknown_reset_token_shows_the_dead_link_page() {
    let meta = mount_metadata().await;
    let idp = mount_idp(password_user("libra@example.com"), None).await;
    let (state, _db, _email) = state_with_reset(&idp, &meta, true).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = get(&app, "/auth/password/reset/new?token=never-minted").await;
    assert_renders!(&body_string(resp).await, "portal.auth_link_invalid");
}

#[tokio::test]
async fn reset_routes_are_absent_when_the_password_door_is_off() {
    // The existing-deploy invariant: an OIDC-only deploy has no reset
    // surface at all (the routes simply don't mount → 404).
    let meta = mount_metadata().await;
    let idp = mount_idp(password_user("libra@example.com"), None).await;
    let (state, _db, _email) = state_with_reset(&idp, &meta, /* password_door = */ false).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = get(&app, "/auth/password/reset").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unverified_password_sign_in_is_gated_and_confirmable() {
    let meta = mount_metadata().await;
    // signInWithPassword returns a token whose email is NOT verified.
    let idp = mount_idp(
        password_user("libra@example.com"),
        Some(json!({
            "idToken": fake_id_token("uid-1", "libra@example.com", false),
            "email": "libra@example.com",
            "localId": "uid-1",
        })),
    )
    .await;
    let (state, db, email) = state_with_reset(&idp, &meta, true).await;
    seed_person(&db, "libra@example.com").await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // Open the sign-in form for the login CSRF pair.
    let login = get(&app, "/auth/login?return_to=/portal").await;
    let login_cookie = login
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.contains("navigator_login_csrf="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let html = body_string(login).await;
    let needle = r#"name="csrf_token" value=""#;
    let start = html.find(needle).unwrap() + needle.len();
    let login_csrf = html[start..].split('"').next().unwrap().to_string();

    // Sign in → hard-gated: no session, the "confirm your email" page.
    let gated = post_form(
        &app,
        "/auth/password",
        &login_cookie,
        format!(
            "email=libra%40example.com&password=pw&return_to=%2Fportal&csrf_token={login_csrf}"
        ),
    )
    .await;
    assert!(!has_session(&gated), "an unverified user gets no session");
    assert_renders!(&body_string(gated).await, "portal.confirm_email");

    // The confirmation link was mailed; clicking it verifies the address.
    let confirm_token = token_from_email(&email, "/auth/email/confirm?token=");
    let claimed = get(&app, &format!("/auth/email/confirm?token={confirm_token}")).await;
    assert_eq!(claimed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        claimed.headers().get("location").unwrap(),
        "/auth/login?notice=email_confirmed"
    );
}
