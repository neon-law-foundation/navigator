//! Cucumber runner for `features/onboarding_welcome.feature`.
//!
//! Drives the OAuth callback end-to-end against a wiremock `IdP`, then
//! asserts on the welcome emails captured by the shared
//! [`CapturingEmail`]. The capture is the visible artifact of the
//! `email_send__welcome` dispatch — the workflow trigger fires the
//! `onboarding__welcome` spec via `state.workflow_runtime`, the
//! [`workflows::DispatchingRuntime`] wrapper (from
//! [`features::app_state_with_email`]) catches the
//! `email_send__welcome` transition, and routes the render through
//! the shared [`workflows::EmailService`].
//!
//! The trigger is `tokio::spawn`'d in the callback — assertions
//! `tokio::yield_now` a few times before checking the captured list
//! so the background task gets a turn.

// Cucumber's step-attribute macros want `async fn` everywhere.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::http::StatusCode;
use cucumber::{given, then, when, World};
use features::{
    app_state_with_email, drive_verified_oauth, fs_storage, in_memory_db, verified_oauth_config,
};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::{entity::person, Db};
use web::email::CapturingEmail;
use web::{policy::PolicyClient, SessionStore};
use wiremock::MockServer;
use workflows::{EmailService, InMemoryRuntime};

#[derive(Default, World)]
#[world(init = Self::default)]
struct WelcomeWorld {
    idp: Option<Arc<MockServer>>,
    db: Option<Db>,
    app: Option<axum::Router>,
    captured: Option<Arc<CapturingEmail>>,
    issued_sub: Option<String>,
    issued_email: Option<String>,
    issued_name: Option<String>,
}

impl std::fmt::Debug for WelcomeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WelcomeWorld").finish_non_exhaustive()
    }
}

impl WelcomeWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }

    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }

    fn captured(&self) -> Vec<web::email::OutboundEmail> {
        self.captured
            .as_ref()
            .expect("capturing email not wired")
            .captured()
    }
}

/// The operator's bootstrap admin email. Sign-up is operator-mediated,
/// so the callback JIT-creates exactly one identity — this one — and
/// fires the welcome once. The row it creates carries the ordinary
/// `admin` role; there is no separate "super" tier. Every other
/// unseeded identity is rejected with 403.
const BOOTSTRAP_ADMIN_EMAIL: &str = "nick@neonlaw.com";

async fn build_app(world: &mut WelcomeWorld, idp_uri: &str) {
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("onboarding-welcome").await;
    let oauth = verified_oauth_config(idp_uri);
    let capturing = Arc::new(CapturingEmail::new());
    let capturing_as_service: Arc<dyn EmailService> = capturing.clone();
    let mut state = app_state_with_email(
        db.clone(),
        runtime,
        storage,
        PolicyClient::passthrough(),
        Some(oauth),
        SessionStore::new("test-session-key-not-for-production"),
        capturing_as_service,
    );
    // The welcome only fires on the bootstrap-admin JIT-create path, so
    // the test app must know which email is the operator's admin.
    state.bootstrap_admin_email = Some(BOOTSTRAP_ADMIN_EMAIL.to_string());
    world.db = Some(db);
    world.captured = Some(capturing);
    world.app = Some(web::build_router(
        state,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    ));
}

#[given("a CapturingEmail backend wired into the app")]
async fn capturing_backend(world: &mut WelcomeWorld) {
    // The IdP mock has to be running first because OAuthConfig
    // captures the URI by value at construction time. If
    // `seed_idp_token` hasn't run yet (the Staff scenario runs the
    // `seeded person` step first), spin one up here.
    if world.idp.is_none() {
        let server = MockServer::start().await;
        world.idp = Some(Arc::new(server));
    }
    if world.app.is_none() {
        let uri = world.idp.as_ref().unwrap().uri();
        build_app(world, &uri).await;
    }
}

#[given(regex = r#"^the IdP issues sub="([^"]+)", email="([^"]+)", name="([^"]+)"$"#)]
async fn seed_idp_token(world: &mut WelcomeWorld, sub: String, email: String, name: String) {
    // Only record the identity — the `/token` mock is mounted per
    // login by `drive_verified_oauth`, which has to sign the
    // id_token with that login's `nonce` to satisfy the verifier.
    world.issued_sub = Some(sub);
    world.issued_email = Some(email);
    world.issued_name = Some(name);
}

#[given(regex = r#"^a seeded person with email "([^"]+)" and role "([^"]+)"$"#)]
async fn seed_person(world: &mut WelcomeWorld, email: String, role: String) {
    let role = match role.as_str() {
        "admin" => store::entity::person::Role::Admin,
        "staff" => store::entity::person::Role::Staff,
        _ => store::entity::person::Role::Client,
    };
    person::ActiveModel {
        name: ActiveValue::Set(String::new()),
        email: ActiveValue::Set(email),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .unwrap();
}

async fn drive_oauth(world: &WelcomeWorld) {
    let app = world.app();
    let idp = world.idp.as_ref().expect("idp not started").clone();
    let sub = world.issued_sub.as_deref().expect("identity seeded");
    let email = world.issued_email.as_deref().expect("identity seeded");
    let name = world.issued_name.as_deref().expect("identity seeded");
    let status = drive_verified_oauth(&app, &idp, sub, email, name).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[when(regex = r"^(?:Staff|the bootstrap admin) completes the OAuth login dance(?: again)?$")]
async fn complete_oauth(world: &mut WelcomeWorld) {
    // The welcome dispatch is `tokio::spawn`'d inside the callback so
    // the HTTP response doesn't block on broker latency. Poll (bounded)
    // for it to land rather than racing a fixed yield burst — the burst
    // flaked under full-suite CPU contention. Scenarios that expect no
    // welcome simply exhaust the small budget before asserting empty.
    let before = world.captured().len();
    drive_oauth(world).await;
    for _ in 0..200 {
        if world.captured().len() > before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[then(regex = r"^exactly (\d+) captured emails? exists?$")]
async fn assert_captured_count(world: &mut WelcomeWorld, expected: usize) {
    let captured = world.captured();
    assert_eq!(
        captured.len(),
        expected,
        "captured emails: {:?}",
        captured.iter().map(|e| &e.to).collect::<Vec<_>>()
    );
}

#[then("no captured emails exist")]
async fn assert_no_captured(world: &mut WelcomeWorld) {
    let captured = world.captured();
    assert!(
        captured.is_empty(),
        "expected no welcomes, got: {captured:?}"
    );
}

#[then(regex = r#"^the captured email is addressed to "([^"]+)"$"#)]
async fn assert_captured_to(world: &mut WelcomeWorld, expected: String) {
    let captured = world.captured();
    let first = captured.first().expect("at least one captured email");
    assert_eq!(first.to, expected);
}

#[then(regex = r#"^the captured email subject is "([^"]+)"$"#)]
async fn assert_captured_subject(world: &mut WelcomeWorld, expected: String) {
    let captured = world.captured();
    let first = captured.first().expect("at least one captured email");
    assert_eq!(first.subject, expected);
}

#[then(regex = r#"^the captured email body mentions "([^"]+)"$"#)]
async fn assert_captured_body_contains(world: &mut WelcomeWorld, needle: String) {
    let captured = world.captured();
    let first = captured.first().expect("at least one captured email");
    assert!(
        first.body.contains(&needle),
        "body did not mention {needle:?}: {}",
        first.body
    );
}

#[tokio::main]
async fn main() {
    WelcomeWorld::cucumber()
        .run("tests/features/onboarding_welcome.feature")
        .await;
}
