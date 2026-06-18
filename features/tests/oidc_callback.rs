//! Cucumber runner for `features/oidc_callback.feature`.
//!
//! Stands up wiremock as Keycloak, drives `/auth/login` →
//! `/auth/callback` end-to-end, and asserts on the resulting
//! `persons` table state. Mirrors the patterns in
//! `web/tests/oidc_e2e.rs`.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::http::StatusCode;
use cucumber::{given, then, when, World};
use features::{app_state, drive_verified_oauth, fs_storage, in_memory_db, verified_oauth_config};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::{entity::person, Db};
use web::{policy::PolicyClient, SessionStore};
use wiremock::MockServer;
use workflows::InMemoryRuntime;

#[derive(Default, World)]
#[world(init = Self::default)]
struct OidcWorld {
    idp: Option<Arc<MockServer>>,
    db: Option<Db>,
    app: Option<axum::Router>,
    issued_sub: Option<String>,
    issued_email: Option<String>,
    issued_name: Option<String>,
    callback_status: Option<StatusCode>,
}

impl std::fmt::Debug for OidcWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcWorld")
            .field("issued_sub", &self.issued_sub)
            .field("issued_email", &self.issued_email)
            .finish_non_exhaustive()
    }
}

impl OidcWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }

    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }
}

/// Build the `AppState` + Router once we know the `IdP` URI. The
/// per-scenario identity is recorded by `seed_idp_token` and signed
/// into the `/token` response by [`drive_verified_oauth`] once the
/// login leg reveals the nonce; the `OAuthConfig` carries the test
/// `id_token` verifier so the callback runs full verification.
async fn build_app(world: &mut OidcWorld) {
    let idp = world.idp.as_ref().expect("idp mock not started");
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("oidc").await;
    let oauth = verified_oauth_config(&idp.uri());
    let state = app_state(
        db.clone(),
        runtime,
        storage,
        PolicyClient::passthrough(),
        Some(oauth),
        SessionStore::new("test-session-key-not-for-production"),
    );
    world.db = Some(db);
    world.app = Some(web::build_router(
        state,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    ));
}

#[given("a mock IdP returning an id_token")]
async fn start_idp(world: &mut OidcWorld) {
    let server = MockServer::start().await;
    world.idp = Some(Arc::new(server));
}

#[given(regex = r#"^the IdP issues sub="([^"]+)", email="([^"]+)", name="([^"]+)"$"#)]
async fn seed_idp_token(world: &mut OidcWorld, sub: String, email: String, name: String) {
    // Only record the identity — the `/token` mock is mounted per
    // login by `drive_verified_oauth`, which has to sign the
    // id_token with that login's `nonce` to satisfy the verifier.
    world.issued_sub = Some(sub);
    world.issued_email = Some(email);
    world.issued_name = Some(name);
    // The IdP mock has to be live before the AppState is built — the
    // OAuthConfig captures the URI by value at construction time.
    if world.app.is_none() {
        build_app(world).await;
    }
}

#[given(regex = r#"^a seeded person with email "([^"]+)" and role "([^"]+)"$"#)]
async fn seed_person(world: &mut OidcWorld, email: String, role: String) {
    // The seeded row has to land in the same DB the callback writes
    // to. Build the app first if the previous step hasn't.
    if world.app.is_none() {
        build_app(world).await;
    }
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

#[when(regex = r"^(?:Libra|Staff|Cancer) completes the OAuth login dance(?: again)?$")]
async fn complete_oauth(world: &mut OidcWorld) {
    let app = world.app();
    let idp = world.idp.as_ref().expect("idp not started").clone();
    let sub = world.issued_sub.clone().expect("identity seeded");
    let email = world.issued_email.clone().expect("identity seeded");
    let name = world.issued_name.clone().expect("identity seeded");
    world.callback_status = Some(drive_verified_oauth(&app, &idp, &sub, &email, &name).await);
}

#[then("the callback redirects with 303")]
async fn callback_redirects(world: &mut OidcWorld) {
    assert_eq!(world.callback_status, Some(StatusCode::SEE_OTHER));
}

#[then("the callback is rejected with 403")]
async fn callback_rejected(world: &mut OidcWorld) {
    assert_eq!(
        world.callback_status,
        Some(StatusCode::FORBIDDEN),
        "an unseeded identity must be rejected — sign-up is operator-mediated",
    );
}

#[then(regex = r"^exactly (\d+) persons rows? exists?$")]
async fn count_persons(world: &mut OidcWorld, expected: usize) {
    let persons = person::Entity::find().all(world.db()).await.unwrap();
    assert_eq!(persons.len(), expected, "rows: {persons:?}");
}

#[then(regex = r#"^the persons row has oidc_subject "([^"]+)"$"#)]
async fn assert_subject(world: &mut OidcWorld, expected: String) {
    let persons = person::Entity::find().all(world.db()).await.unwrap();
    let row = persons.first().expect("at least one persons row");
    assert_eq!(row.oidc_subject.as_deref(), Some(expected.as_str()));
}

#[then(regex = r#"^the persons row has email "([^"]+)"$"#)]
async fn assert_email(world: &mut OidcWorld, expected: String) {
    let persons = person::Entity::find().all(world.db()).await.unwrap();
    let row = persons.first().expect("at least one persons row");
    assert_eq!(row.email, expected);
}

#[then(regex = r#"^the persons row has name "([^"]+)"$"#)]
async fn assert_name(world: &mut OidcWorld, expected: String) {
    let persons = person::Entity::find().all(world.db()).await.unwrap();
    let row = persons.first().expect("at least one persons row");
    assert_eq!(row.name, expected);
}

#[then(regex = r#"^the persons row keeps the "([^"]+)" role$"#)]
async fn assert_role_preserved(world: &mut OidcWorld, role: String) {
    let expected = match role.as_str() {
        "admin" => store::entity::person::Role::Admin,
        "staff" => store::entity::person::Role::Staff,
        _ => store::entity::person::Role::Client,
    };
    let persons = person::Entity::find().all(world.db()).await.unwrap();
    let row = persons.first().expect("at least one persons row");
    assert_eq!(row.role, expected);
}

#[tokio::main]
async fn main() {
    OidcWorld::cucumber()
        .run("tests/features/oidc_callback.feature")
        .await;
}
