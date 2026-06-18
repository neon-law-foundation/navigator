//! Cucumber runner for `features/portal_admin_firm_surface.feature`.
//!
//! Verifies that the firm-wide CRUD routes answer at the new
//! `/portal/portal/admin/*` namespace (dual-mounted alongside the legacy
//! `/portal/admin/*` in `web::admin::routes`). OPA is `passthrough` in this
//! suite — the client-blocked scenario lives in a live-KIND smoke
//! test, not here.

// Cucumber's step-attribute macros want `async fn` everywhere.
#![allow(clippy::unused_async)]

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::person;
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{policy::PolicyClient, SessionStore};
use workflows::InMemoryRuntime;

#[derive(Default, World)]
#[world(init = Self::default)]
struct FirmWorld {
    db: Option<Db>,
    app: Option<axum::Router>,
    sessions: Option<SessionStore>,
    persons: HashMap<String, Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
}

impl std::fmt::Debug for FirmWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirmWorld")
            .field("last_status", &self.last_status)
            .finish_non_exhaustive()
    }
}

impl FirmWorld {
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }
    fn sessions(&self) -> &SessionStore {
        self.sessions.as_ref().expect("sessions not built")
    }
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }
}

#[given("the Navigator app is running")]
async fn build_app(world: &mut FirmWorld) {
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("portal-admin-firm").await;
    let sessions = SessionStore::new("test-session-key-not-for-production");
    let state = app_state(
        db.clone(),
        runtime,
        storage,
        PolicyClient::passthrough(),
        None,
        sessions.clone(),
    );
    world.db = Some(db);
    world.sessions = Some(sessions);
    world.app = Some(web::build_router(
        state,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    ));
}

#[given(regex = r#"^a seeded person "([^"]+)" with role "([^"]+)"$"#)]
async fn seed_person(world: &mut FirmWorld, email: String, role: String) {
    let role = match role.as_str() {
        "admin" => person::Role::Admin,
        "staff" => person::Role::Staff,
        _ => person::Role::Client,
    };
    let inserted = person::ActiveModel {
        name: ActiveValue::Set(email.clone()),
        email: ActiveValue::Set(email.clone()),
        oidc_subject: ActiveValue::Set(Some(format!("kc-uuid-{email}"))),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert person");
    world.persons.insert(email, inserted.id);
}

#[when(regex = r#"^"([^"]+)" opens (.+)$"#)]
async fn open(world: &mut FirmWorld, email: String, path: String) {
    let person_id = *world.persons.get(&email).expect("actor seeded");
    let role = role_for(world.db(), person_id).await;
    let session = SessionData {
        sub: format!("kc-uuid-{email}"),
        email: Some(email.clone()),
        person_id: Some(person_id),
        exp: web::session::now_unix_secs() + 60,
        role,
        csrf_token: "test-csrf".into(),
        source: web::session::SessionSource::Browser,
    };
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}",
        world.sessions().encode(&session)
    );
    let resp = world
        .app()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    world.last_status = Some(resp.status());
    world.last_body = body_string(resp).await;
}

async fn role_for(db: &Db, person_id: Uuid) -> person::Role {
    use sea_orm::EntityTrait;
    person::Entity::find_by_id(person_id)
        .one(db)
        .await
        .expect("query")
        .expect("row exists")
        .role
}

#[then(regex = r"^the response status is (\d+)$")]
async fn status_is(world: &mut FirmWorld, code: u16) {
    let actual = world.last_status.expect("no response");
    assert_eq!(
        actual.as_u16(),
        code,
        "expected {code}, got {} (body: {})",
        actual,
        truncate(&world.last_body)
    );
}

#[then(regex = r#"^the response body contains "([^"]+)"$"#)]
async fn body_contains(world: &mut FirmWorld, needle: String) {
    assert!(
        world.last_body.contains(&needle),
        "body did not contain {needle:?}; body: {}",
        truncate(&world.last_body)
    );
}

fn truncate(s: &str) -> String {
    if s.len() <= 400 {
        s.to_string()
    } else {
        format!("{}…", &s[..400])
    }
}

#[tokio::main]
async fn main() {
    FirmWorld::cucumber()
        .run("tests/features/portal_admin_firm_surface.feature")
        .await;
}
