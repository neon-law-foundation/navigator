#![allow(clippy::doc_markdown)]
//! End-to-end OIDC + OPA + persons-upsert integration test.
//!
//! This is the test the user demanded — a single test that exercises
//! the *entire* authentication and authorization pipeline against
//! mocked IdP and OPA endpoints, then asserts on the database state
//! the flow produced:
//!
//! 1. Start a `wiremock` `MockServer` and program it to behave like
//!    Keycloak's `/token` endpoint, returning an id_token with `sub`,
//!    `email`, and `name` (the tier lives on the persons row, not the
//!    token).
//! 2. Start a second `wiremock` to act as the OPA decision endpoint
//!    and return `result=true` for any request.
//! 3. Build the real `web::build_router`, sharing the test sessions
//!    store, an in-memory SQLite (with migrations applied), and an
//!    OAuth config pointed at the IdP mock.
//! 4. Hit `/auth/login?return_to=/portal/admin/people`, follow the redirect
//!    back to `/auth/callback`, then hit `/portal/admin/people` with the
//!    resulting session cookie and an `Authorization: Bearer …`
//!    that satisfies the existing bearer-token middleware.
//! 5. Assert:
//!    - the callback created a `persons` row keyed on the OIDC
//!      subject, with the email and name from the id_token;
//!    - the admin route returned 200 (policy allowed);
//!    - swapping the OPA mock to deny causes the same request to
//!      return 403.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use serde_json::json;
use store::entity::person;
use store::Db;
use tower::ServiceExt;
use web::{policy, AppState, AuthConfig, OAuthConfig, SessionStore};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sessions() -> SessionStore {
    SessionStore::new("test-session-key-not-for-production")
}

async fn in_memory_db_with_schema() -> Db {
    store::test_support::pg().await
}

/// Assemble the AppState for one test, sharing a single in-memory
/// SQLite so `/auth/callback` can write a row that the assertions
/// below read back.
async fn state(
    oauth_cfg: OAuthConfig,
    sessions_store: SessionStore,
    policy_client: policy::PolicyClient,
) -> (AppState, Db) {
    let db = in_memory_db_with_schema().await;
    let state = AppState {
        // bearer-token middleware in passthrough mode (no JWT
        // verification) so the test can focus on the OIDC flow.
        auth: AuthConfig::new(false, None),
        sessions: sessions_store,
        oauth: Some(oauth_cfg),
        storage: Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-oidc-e2e"))
                .await
                .unwrap(),
        ),
        policy: policy_client,
        ..web::test_support::app_state(db.clone()).await
    };
    (state, db)
}

/// OAuth `client_id` every test uses; the verifier is pinned to it.
const CLIENT_ID: &str = "navigator-web";

/// An IdP mock plus the identity it will assert. The signed-token mock
/// is mounted lazily by [`complete_oauth_flow`] / [`callback_response`]
/// once they know the login's per-request `nonce`, so the token can
/// carry it and pass full verification.
struct TestIdp {
    server: MockServer,
    sub: String,
    email: String,
    name: String,
}

impl TestIdp {
    fn uri(&self) -> String {
        self.server.uri()
    }
}

/// Start an IdP mock that will assert the given identity. The role is
/// *intentionally* never in the token — it lives in the DB.
async fn idp_returning(sub: &str, email: &str, name: &str) -> TestIdp {
    TestIdp {
        server: MockServer::start().await,
        sub: sub.into(),
        email: email.into(),
        name: name.into(),
    }
}

/// Mount `idp`'s `/token` endpoint to return a properly-signed id_token
/// that carries `nonce` (so it passes signature + iss/aud/nonce checks).
/// Resets first so a repeated login on the same mock can't return a
/// stale nonce.
async fn mount_token_endpoint(idp: &TestIdp, nonce: &str) {
    idp.server.reset().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id_token": web::test_support::sign_id_token(
                CLIENT_ID, nonce, &idp.sub, &idp.email, &idp.name,
            ),
            "token_type": "Bearer",
        })))
        .mount(&idp.server)
        .await;
}

/// Wrap `OAuthConfig::new` with the test id_token verifier pinned to
/// [`CLIENT_ID`], pointed at `idp`.
fn oauth_cfg(idp: &TestIdp) -> OAuthConfig {
    web::test_support::oauth_config_with_verifier(
        OAuthConfig::new(
            CLIENT_ID,
            "navigator-web-secret",
            "http://app.test/auth/callback",
            format!("{}/authorize", idp.uri()),
            format!("{}/token", idp.uri()),
        ),
        CLIENT_ID,
    )
}

/// Insert a `persons` row up-front so a downstream `/auth/callback`
/// can promote (link the `oidc_subject`) rather than 403. Sign-up is
/// operator-mediated — every test that drives the callback to a
/// successful session has to call this first.
async fn seed_person(db: &Db, email: &str, name: &str, role: store::entity::person::Role) {
    person::ActiveModel {
        name: ActiveValue::Set(name.into()),
        email: ActiveValue::Set(email.into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed person");
}

/// `/auth/login` → extract state/nonce → drive `/auth/callback`, mounting
/// the signed-token endpoint with the login's nonce. Returns the raw
/// callback response so callers can assert success *or* failure.
async fn drive_callback(
    app: &axum::Router,
    idp: &TestIdp,
    return_to: &str,
) -> axum::http::Response<Body> {
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/auth/login?return_to={return_to}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let location = login
        .headers()
        .get("location")
        .unwrap()
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
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    mount_token_endpoint(idp, &nonce).await;

    app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/auth/callback?code=any-code&state={state_param}"))
                .header("cookie", &pre_auth_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Drive the full flow and return the session cookie, asserting the
/// callback succeeded (SEE_OTHER).
async fn complete_oauth_flow(app: &axum::Router, idp: &TestIdp, return_to: &str) -> String {
    let cb = drive_callback(app, idp, return_to).await;
    assert_eq!(cb.status(), StatusCode::SEE_OTHER);
    cb.headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.contains("navigator_session="))
        .unwrap_or_else(|| panic!("expected navigator_session cookie in callback response"))
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn full_oidc_flow_upserts_person_and_passes_opa_allow() {
    // ----- IdP mock (Keycloak stand-in) -----
    let idp = idp_returning("kc-uuid-staff", "staff@neonlaw.com", "Staff").await;

    // ----- OPA mock — every decision allows -----
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;
    let policy_client = policy::PolicyClient::new(opa.uri());

    let (state, db) = state(oauth_cfg(&idp), sessions(), policy_client).await;
    // Pre-seed Staff — sign-up is operator-mediated. The callback
    // promotes (links `oidc_subject`) instead of inserting.
    seed_person(
        &db,
        "staff@neonlaw.com",
        "Staff",
        store::entity::person::Role::Staff,
    )
    .await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // 1. Drive the OAuth dance.
    let session_cookie = complete_oauth_flow(&app, &idp, "/portal/admin/people").await;

    // 2. The callback should have promoted the pre-seeded row by
    //    stamping the IdP subject. Email + name stay as seeded; the
    //    callback never overwrites them from the token.
    let persons = person::Entity::find().all(&db).await.unwrap();
    assert_eq!(persons.len(), 1, "expected exactly one persons row");
    assert_eq!(persons[0].oidc_subject.as_deref(), Some("kc-uuid-staff"));
    assert_eq!(persons[0].email, "staff@neonlaw.com");
    assert_eq!(persons[0].name, "Staff");

    // 3. With OPA returning allow, /portal/admin/people must return 200.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people")
                .header("cookie", &session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "admin request must succeed under allow; got {}",
        resp.status(),
    );
}

#[tokio::test]
async fn opa_deny_blocks_admin_route_with_403() {
    // Same IdP mock — successful login still happens.
    let idp = idp_returning("kc-uuid-taurus", "taurus@example.com", "Taurus").await;

    // OPA denies every decision.
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": false })))
        .mount(&opa)
        .await;
    let policy_client = policy::PolicyClient::new(opa.uri());

    let (state, db) = state(oauth_cfg(&idp), sessions(), policy_client).await;
    // Taurus is pre-seeded as a Client. They can sign in — sign-in only
    // checks that the persons row exists — but the OPA-deny chain
    // then blocks every protected route.
    seed_person(
        &db,
        "taurus@example.com",
        "Taurus",
        store::entity::person::Role::Client,
    )
    .await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let session_cookie = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    // Promotion happened — the row gained `oidc_subject` but kept
    // its Client tier.
    let persons = person::Entity::find().all(&db).await.unwrap();
    assert_eq!(persons.len(), 1);
    assert_eq!(persons[0].oidc_subject.as_deref(), Some("kc-uuid-taurus"));

    // ...but OPA denies, so the admin route returns 403.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .header("cookie", &session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "admin request must be 403 under deny; got {}",
        resp.status(),
    );
}

#[tokio::test]
async fn second_login_with_same_subject_does_not_create_duplicate_person() {
    let idp = idp_returning("kc-uuid-staff", "staff@neonlaw.com", "Staff").await;
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;

    let (state, db) = state(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
    )
    .await;
    seed_person(
        &db,
        "staff@neonlaw.com",
        "Staff",
        store::entity::person::Role::Staff,
    )
    .await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let _ = complete_oauth_flow(&app, &idp, "/portal/admin").await;
    let _ = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    let persons = person::Entity::find().all(&db).await.unwrap();
    assert_eq!(
        persons.len(),
        1,
        "two logins with the same `sub` must produce one person row, got {}",
        persons.len(),
    );
}

// ---------- DB-sourced role gating across multiple admin routes ----------
//
// The next two tests prove the architectural claim documented in
// `docs/oidc.md` + `docs/access-model.md`: the IdP token *cannot*
// grant access on its own. The system-wide tier lives on
// `persons.role` and is read into the session at callback time. OPA
// evaluates `input.session.role`, which therefore reflects whatever
// the DB says regardless of what the IdP claimed.

/// Spin up an OPA mock that allows iff `input.session.role == "staff"`.
/// Returns the wiremock so the test can pass `.uri()` to
/// `PolicyClient::new`.
async fn opa_allowing_only_staff() -> MockServer {
    let opa = MockServer::start().await;
    // Build a single Mock with a body-matcher that inspects the
    // JSON the web server posts. The matcher allows-when-staff and
    // falls through to a catch-all that returns `result: false`.
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .and(body_partial_json(json!({
            "input": { "session": { "role": "staff" } }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": false })))
        .mount(&opa)
        .await;
    opa
}

const ADMIN_ROUTES: &[&str] = &[
    "/portal/admin",
    "/portal/admin/people",
    "/portal/admin/entities",
    "/portal/admin/jurisdictions",
    "/portal/admin/entity-types",
    "/portal/admin/templates",
    "/portal/admin/questions",
    "/portal/projects",
];

#[tokio::test]
async fn user_with_db_staff_role_can_hit_every_admin_route() {
    let idp = idp_returning("kc-uuid-staff", "staff@neonlaw.com", "Staff").await;
    let opa = opa_allowing_only_staff().await;
    let policy_client = policy::PolicyClient::new(opa.uri());

    let (state, db) = state(oauth_cfg(&idp), sessions(), policy_client).await;

    // Pre-seed the persons row with email + staff role. The OAuth
    // callback will *promote* this row when Staff logs in for the
    // first time (link by email, stamp the subject).
    person::ActiveModel {
        name: ActiveValue::Set("Staff".into()),
        email: ActiveValue::Set("staff@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let session_cookie = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    // The promoted row now has the OIDC subject linked.
    let staff = person::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.email == "staff@neonlaw.com")
        .unwrap();
    assert_eq!(staff.oidc_subject.as_deref(), Some("kc-uuid-staff"));
    assert_eq!(
        staff.role,
        store::entity::person::Role::Staff,
        "the seeded role must survive the promotion",
    );

    // Hit every admin GET route — each should pass the DB-role gate
    // and render with HTTP 200.
    for route in ADMIN_ROUTES {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(*route)
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{route} must succeed under DB-staff role; got {}",
            resp.status(),
        );
    }
}

#[tokio::test]
async fn user_with_client_role_is_denied_from_admin_routes() {
    // Cancer is pre-seeded as a Client. The IdP says nothing about a
    // tier (the callback ignores any claim anyway). After the
    // callback, `persons.role = 'client'`. Every admin route must 403.
    let idp = idp_returning("kc-uuid-cancer", "cancer@example.com", "Cancer").await;
    let opa = opa_allowing_only_staff().await;
    let (state, db) = state(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
    )
    .await;
    seed_person(
        &db,
        "cancer@example.com",
        "Cancer",
        store::entity::person::Role::Client,
    )
    .await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let session_cookie = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    let cancer = person::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.email == "cancer@example.com")
        .unwrap();
    assert_eq!(
        cancer.role,
        store::entity::person::Role::Client,
        "seeded Client tier must persist across login",
    );

    for route in ADMIN_ROUTES {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(*route)
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{route} must 403 for a Client-tier user; got {}",
            resp.status(),
        );
    }
}

#[tokio::test]
async fn db_role_revocation_takes_effect_on_next_login() {
    // Staff starts as Staff, logs in, hits admin (success). Then an
    // admin demotes them to Client. The *existing* session keeps
    // working (sessions are signed snapshots), but their next login
    // picks up the Client tier and admin starts returning 403.
    let idp = idp_returning("kc-uuid-staff", "staff@neonlaw.com", "Staff").await;
    let opa = opa_allowing_only_staff().await;
    let (state, db) = state(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
    )
    .await;

    person::ActiveModel {
        name: ActiveValue::Set("Staff".into()),
        email: ActiveValue::Set("staff@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let first_session = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .header("cookie", &first_session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    // Revoke the role.
    let staff = person::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.email == "staff@neonlaw.com")
        .unwrap();
    let mut update: person::ActiveModel = staff.into();
    update.role = ActiveValue::Set(store::entity::person::Role::Client);
    update.update(&db).await.unwrap();

    // Next login picks up the Client tier → 403.
    let second_session = complete_oauth_flow(&app, &idp, "/portal/admin").await;
    let second = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .header("cookie", &second_session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::FORBIDDEN,
        "after DB role revocation, the next session must be denied; got {}",
        second.status(),
    );
}

// ---------- Pre-seed requirement ----------

/// Drive `/auth/login` → `/auth/callback` and return the raw callback
/// response. Used by tests that expect the callback to *fail* (no
/// pre-seeded persons row) — `complete_oauth_flow` asserts SEE_OTHER
/// on the callback, which is exactly what we don't want here.
async fn callback_response(
    app: &axum::Router,
    idp: &TestIdp,
    return_to: &str,
) -> axum::http::Response<Body> {
    drive_callback(app, idp, return_to).await
}

/// Build an `AppState` that already carries the supplied
/// `bootstrap_admin_email`, sharing `db` with the test so assertions can
/// read back what the callback wrote.
async fn state_with_bootstrap_admin(
    oauth_cfg: OAuthConfig,
    sessions_store: SessionStore,
    policy_client: policy::PolicyClient,
    bootstrap_admin: Option<String>,
) -> (AppState, Db) {
    let (mut s, db) = state(oauth_cfg, sessions_store, policy_client).await;
    s.bootstrap_admin_email = bootstrap_admin;
    (s, db)
}

#[tokio::test]
async fn callback_returns_403_html_when_email_is_not_pre_seeded() {
    // Scorpio logs in with a perfectly valid id_token but no operator
    // has ever inserted a `persons` row for `scorpio@example.com`. The
    // callback must refuse to mint a session and render the styled
    // 403 page instead — sign-up is operator-mediated by design.
    let idp = idp_returning("kc-uuid-scorpio", "scorpio@example.com", "Scorpio").await;
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;
    let (s, db) = state_with_bootstrap_admin(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
        // Bootstrap admin override deliberately points elsewhere so scorpio@
        // is NOT the carve-out.
        Some("nobody@unreachable.invalid".into()),
    )
    .await;
    let app = web::build_router(s, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = callback_response(&app, &idp, "/portal/admin").await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(html.starts_with("<!DOCTYPE html>"), "got: {html}");
    assert!(html.contains("Forbidden"), "got: {html}");

    // No persons row created — operator must seed first.
    let persons = person::Entity::find().all(&db).await.unwrap();
    assert!(
        persons.is_empty(),
        "callback must not create a row when sign-up is operator-mediated; got {persons:?}",
    );
}

#[tokio::test]
async fn callback_jit_creates_bootstrap_admin_with_admin_role_when_absent() {
    // The bootstrap admin email is configured via env. If that operator
    // signs in to a fresh deployment where no `persons` row exists
    // yet, the callback JIT-creates the row WITH the `admin` role.
    // This is the sole carve-out from the pre-seed rule — it exists
    // so a brand-new cluster can never lock its operator out.
    let idp = idp_returning("kc-uuid-nick", "nick@neonlaw.com", "Nick").await;
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;
    let (s, db) = state_with_bootstrap_admin(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
        Some("nick@neonlaw.com".into()),
    )
    .await;
    let app = web::build_router(s, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let _ = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    let persons = person::Entity::find().all(&db).await.unwrap();
    assert_eq!(persons.len(), 1, "bootstrap admin row was JIT-created");
    assert_eq!(persons[0].email, "nick@neonlaw.com");
    assert_eq!(persons[0].role, store::entity::person::Role::Admin);
}

#[tokio::test]
async fn bootstrap_admin_role_heals_back_after_being_cleared() {
    // The bootstrap admin email is "always admin" — even if an
    // administrator demotes the row in the UI, the next sign-in
    // restores `admin`. Belt-and-suspenders: a fork's operator
    // cannot accidentally lock themselves out.
    let idp = idp_returning("kc-uuid-nick", "nick@neonlaw.com", "Nick").await;
    let opa = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/data/navigator/authz/allow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": true })))
        .mount(&opa)
        .await;
    let (s, db) = state_with_bootstrap_admin(
        oauth_cfg(&idp),
        sessions(),
        policy::PolicyClient::new(opa.uri()),
        Some("nick@neonlaw.com".into()),
    )
    .await;
    // Pre-seed as Client — simulating a malicious or mistaken
    // demotion of the bootstrap admin row.
    seed_person(
        &db,
        "nick@neonlaw.com",
        "Nick",
        store::entity::person::Role::Client,
    )
    .await;
    let app = web::build_router(s, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let _ = complete_oauth_flow(&app, &idp, "/portal/admin").await;

    let row = person::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.email == "nick@neonlaw.com")
        .unwrap();
    assert_eq!(
        row.role,
        store::entity::person::Role::Admin,
        "bootstrap admin role must heal back after sign-in",
    );
}
