//! Cucumber runner for `features/portal_projects_writes.feature`.
//!
//! Exercises the role-aware write surface under `/portal/projects/*`.
//! Clients get `404` on every write URL; staff and admin reach the
//! existing CRUD handlers. The lightweight client detail at
//! `/portal/projects/:id` continues to render without admin chrome
//! (no Edit / Upload buttons).

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

const CSRF: &str = "test-csrf";

#[derive(Default, World)]
#[world(init = Self::default)]
struct WritesWorld {
    db: Option<Db>,
    app: Option<axum::Router>,
    sessions: Option<SessionStore>,
    persons: HashMap<String, Uuid>,
    projects: HashMap<String, Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
    last_location: Option<String>,
}

impl std::fmt::Debug for WritesWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WritesWorld")
            .field("last_status", &self.last_status)
            .field("last_location", &self.last_location)
            .finish_non_exhaustive()
    }
}

impl WritesWorld {
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
async fn build_app(world: &mut WritesWorld) {
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("portal-projects-writes").await;
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
async fn seed_person(world: &mut WritesWorld, email: String, role: String) {
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
    world: &mut WritesWorld,
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
async fn seed_project_no_participants(world: &mut WritesWorld, project_name: String) {
    ensure_project(world, &project_name).await;
}

async fn ensure_project(world: &mut WritesWorld, project_name: &str) -> Uuid {
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

#[when(regex = r#"^"([^"]+)" submits "([^"]*)" to (/[^ ]+)$"#)]
async fn submit_to_path(world: &mut WritesWorld, email: String, body: String, path: String) {
    let cookie = session_cookie_for(world, &email).await;
    // Opening a matter requires a pre-existing entity. When the scenario
    // posts the create form without one, seed an entity and append it so
    // the static feature body stays focused on what it's testing.
    let mut body = body;
    if path == "/portal/projects" && body.contains("name=") && !body.contains("entity_id=") {
        let entity_id = store::test_support::seed_entity(world.db()).await;
        body = format!("{body}&entity_id={entity_id}");
    }
    let body_with_csrf = if body.is_empty() {
        format!("_csrf={CSRF}")
    } else {
        format!("{body}&_csrf={CSRF}")
    };
    let resp = world
        .app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("cookie", cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body_with_csrf))
                .unwrap(),
        )
        .await
        .unwrap();
    capture(world, resp).await;
}

#[when(regex = r#"^"([^"]+)" submits "([^"]*)" to the delete action for "([^"]+)"$"#)]
async fn submit_delete(world: &mut WritesWorld, email: String, body: String, project_name: String) {
    let project_id = *world
        .projects
        .get(&project_name)
        .expect("project was seeded earlier");
    let cookie = session_cookie_for(world, &email).await;
    let body_with_csrf = if body.is_empty() {
        format!("_csrf={CSRF}")
    } else {
        format!("{body}&_csrf={CSRF}")
    };
    let resp = world
        .app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/projects/{project_id}/delete"))
                .header("cookie", cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body_with_csrf))
                .unwrap(),
        )
        .await
        .unwrap();
    capture(world, resp).await;
}

#[when(regex = r#"^"([^"]+)" opens the edit page for "([^"]+)"$"#)]
async fn open_edit(world: &mut WritesWorld, email: String, project_name: String) {
    let project_id = *world
        .projects
        .get(&project_name)
        .expect("project was seeded earlier");
    get_path(
        world,
        &email,
        &format!("/portal/projects/{project_id}/edit"),
    )
    .await;
}

#[when(regex = r#"^"([^"]+)" opens the detail page for "([^"]+)"$"#)]
async fn open_detail(world: &mut WritesWorld, email: String, project_name: String) {
    let project_id = *world
        .projects
        .get(&project_name)
        .expect("project was seeded earlier");
    get_path(world, &email, &format!("/portal/projects/{project_id}")).await;
}

async fn get_path(world: &mut WritesWorld, email: &str, path: &str) {
    let cookie = session_cookie_for(world, email).await;
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
    capture(world, resp).await;
}

async fn session_cookie_for(world: &mut WritesWorld, email: &str) -> String {
    let person_id = *world.persons.get(email).expect("actor seeded");
    let role = role_for(world.db(), person_id).await;
    let session = SessionData {
        sub: format!("kc-uuid-{email}"),
        email: Some(email.to_string()),
        person_id: Some(person_id),
        exp: web::session::now_unix_secs() + 60,
        role,
        csrf_token: CSRF.into(),
        source: web::session::SessionSource::Browser,
    };
    format!(
        "{SESSION_COOKIE_NAME}={}",
        world.sessions().encode(&session)
    )
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

async fn capture(world: &mut WritesWorld, resp: axum::http::Response<Body>) {
    world.last_status = Some(resp.status());
    world.last_location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);
    world.last_body = body_string(resp).await;
}

#[then(regex = r"^the response status is (\d+)$")]
async fn status_is(world: &mut WritesWorld, code: u16) {
    let actual = world.last_status.expect("no response captured");
    assert_eq!(
        actual.as_u16(),
        code,
        "expected {code}, got {actual} (body: {})",
        truncate(&world.last_body),
    );
}

#[then(regex = r#"^the response body contains "([^"]+)"$"#)]
async fn body_contains(world: &mut WritesWorld, needle: String) {
    let needle = needle.replace("\\\"", "\"");
    assert!(
        world.last_body.contains(&needle),
        "body did not contain {needle:?}; body: {}",
        truncate(&world.last_body),
    );
}

#[then(regex = r#"^the response body does not contain "([^"]+)"$"#)]
async fn body_does_not_contain(world: &mut WritesWorld, needle: String) {
    let needle = needle.replace("\\\"", "\"");
    assert!(
        !world.last_body.contains(&needle),
        "body unexpectedly contained {needle:?}; body: {}",
        truncate(&world.last_body),
    );
}

#[then(regex = r#"^the response location contains "([^"]+)"$"#)]
async fn location_contains(world: &mut WritesWorld, needle: String) {
    let loc = world
        .last_location
        .as_deref()
        .expect("no Location header on response");
    assert!(
        loc.contains(&needle),
        "expected Location to contain {needle:?}, got {loc:?}",
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
    WritesWorld::cucumber()
        .run("tests/features/portal_projects_writes.feature")
        .await;
}
