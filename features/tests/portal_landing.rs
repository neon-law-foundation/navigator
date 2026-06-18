//! Cucumber runner for `features/portal_landing_per_role.feature`.
//!
//! Drives `GET /portal` end-to-end against `web::build_router`. Each
//! scenario seeds the actor's `persons` row and any project +
//! participation rows in the test schema, then forges a session
//! cookie via the same `SessionStore` the app is built with, and
//! asserts on status / body / `Location` of the response.
//!
//! The forged cookie skips the OIDC dance — we're testing the
//! role-fanout, not the callback. `OAuthConfig` is therefore left
//! `None`. `PolicyClient::passthrough` keeps OPA out of the loop so
//! these scenarios pass regardless of the rego bundle's exact shape
//! — the live policy check is exercised separately under
//! `features/oidc_callback`.

// Cucumber's step-attribute macros want `async fn` everywhere.
#![allow(clippy::unused_async)]

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::{app_state, body_string, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::{person, person_project_role, project};
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{policy::PolicyClient, SessionStore};
use workflows::InMemoryRuntime;

#[derive(Default, World)]
#[world(init = Self::default)]
struct PortalWorld {
    db: Option<Db>,
    app: Option<axum::Router>,
    sessions: Option<SessionStore>,
    persons: HashMap<String, Uuid>,
    projects: HashMap<String, Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
    last_location: Option<String>,
}

impl std::fmt::Debug for PortalWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortalWorld")
            .field("last_status", &self.last_status)
            .field("last_location", &self.last_location)
            .finish_non_exhaustive()
    }
}

impl PortalWorld {
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
async fn build_app(world: &mut PortalWorld) {
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("portal-landing").await;
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
async fn seed_person(world: &mut PortalWorld, email: String, role: String) {
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

#[given(regex = r#"^a project "([^"]+)" with "([^"]+)" as a participant$"#)]
async fn seed_project_with_participant(
    world: &mut PortalWorld,
    project_name: String,
    participant_email: String,
) {
    let project_id = ensure_project(world, &project_name).await;
    let person_id = *world
        .persons
        .get(&participant_email)
        .expect("participant person was seeded earlier");
    person_project_role::ActiveModel {
        person_id: ActiveValue::Set(person_id),
        project_id: ActiveValue::Set(project_id),
        participation: ActiveValue::Set("participant".into()),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert person_project_role");
}

#[given(regex = r#"^a project "([^"]+)" with no participants$"#)]
async fn seed_project_no_participants(world: &mut PortalWorld, project_name: String) {
    ensure_project(world, &project_name).await;
}

async fn ensure_project(world: &mut PortalWorld, project_name: &str) -> Uuid {
    if let Some(id) = world.projects.get(project_name) {
        return *id;
    }
    let entity_id = store::test_support::seed_entity(world.db()).await;
    let inserted = project::ActiveModel {
        name: ActiveValue::Set(project_name.into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert project");
    world.projects.insert(project_name.to_string(), inserted.id);
    inserted.id
}

#[when(regex = r"^an anonymous visitor opens (.+)$")]
async fn anonymous_visit(world: &mut PortalWorld, path: String) {
    let resp = world
        .app()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    capture_response(world, resp).await;
}

#[when(regex = r#"^"([^"]+)" opens (.+)$"#)]
async fn actor_visits(world: &mut PortalWorld, email: String, path: String) {
    let person_id = *world
        .persons
        .get(&email)
        .expect("actor person was seeded earlier");
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
    capture_response(world, resp).await;
}

async fn role_for(db: &Db, person_id: Uuid) -> person::Role {
    use sea_orm::EntityTrait;
    person::Entity::find_by_id(person_id)
        .one(db)
        .await
        .expect("query person")
        .expect("person row exists")
        .role
}

async fn capture_response(world: &mut PortalWorld, resp: axum::http::Response<Body>) {
    world.last_status = Some(resp.status());
    world.last_location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);
    world.last_body = body_string(resp).await;
}

#[then(regex = r"^the response status is (\d+)$")]
async fn status_is(world: &mut PortalWorld, code: u16) {
    let actual = world.last_status.expect("no response captured");
    assert_eq!(
        actual.as_u16(),
        code,
        "expected status {code}, got {} (body: {})",
        actual,
        truncated(&world.last_body)
    );
}

#[then(regex = r#"^the redirect location starts with "([^"]+)"$"#)]
async fn location_starts_with(world: &mut PortalWorld, prefix: String) {
    let loc = world
        .last_location
        .as_deref()
        .expect("response had no Location header");
    assert!(
        loc.starts_with(&prefix),
        "expected Location starting with {prefix:?}, got {loc:?}"
    );
}

#[then(regex = r#"^the redirect location is "([^"]+)"$"#)]
async fn location_equals(world: &mut PortalWorld, expected: String) {
    let loc = world
        .last_location
        .as_deref()
        .expect("response had no Location header");
    assert_eq!(loc, expected);
}

#[then(regex = r#"^the redirect location is the project page for "([^"]+)"$"#)]
async fn location_is_project(world: &mut PortalWorld, project_name: String) {
    let project_id = world
        .projects
        .get(&project_name)
        .expect("project was seeded earlier");
    let expected = format!("/portal/projects/{project_id}");
    let loc = world
        .last_location
        .as_deref()
        .expect("response had no Location header");
    assert_eq!(loc, expected);
}

#[then(regex = r#"^the response body contains "([^"]+)"$"#)]
async fn body_contains(world: &mut PortalWorld, needle: String) {
    assert!(
        world.last_body.contains(&needle),
        "expected body to contain {needle:?}; body was: {}",
        truncated(&world.last_body)
    );
}

#[then(regex = r#"^the response body does not contain "([^"]+)"$"#)]
async fn body_does_not_contain(world: &mut PortalWorld, needle: String) {
    assert!(
        !world.last_body.contains(&needle),
        "expected body NOT to contain {needle:?}; body was: {}",
        truncated(&world.last_body)
    );
}

fn truncated(s: &str) -> String {
    const LIMIT: usize = 400;
    if s.len() <= LIMIT {
        s.to_string()
    } else {
        format!("{}…", &s[..LIMIT])
    }
}

#[tokio::main]
async fn main() {
    PortalWorld::cucumber()
        .run("tests/features/portal_landing_per_role.feature")
        .await;
}
