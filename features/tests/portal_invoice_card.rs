//! Cucumber runner for `features/portal_invoice_card.feature`.
//!
//! Grounds the read side of the per-project invoice card: a row in the
//! `xero_invoices` mirror (raised at matter close, reconciled by the
//! nightly `ReconcileInvoices` workflow) drives what the client sees at
//! `GET /portal/projects/:id`. The runner shape mirrors
//! `portal_projects_detail.rs` — forge a session cookie, send the
//! request, assert on the rendered card — adding mirror-row setup via
//! `store::xero_invoices`.

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
use store::xero_invoices::{self, UpsertXeroInvoice};
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{policy::PolicyClient, SessionStore};
use workflows::InMemoryRuntime;

#[derive(Default, World)]
#[world(init = Self::default)]
struct CardWorld {
    db: Option<Db>,
    app: Option<axum::Router>,
    sessions: Option<SessionStore>,
    persons: HashMap<String, Uuid>,
    projects: HashMap<String, Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
}

impl std::fmt::Debug for CardWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardWorld")
            .field("last_status", &self.last_status)
            .finish_non_exhaustive()
    }
}

impl CardWorld {
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }

    fn sessions(&self) -> &SessionStore {
        self.sessions.as_ref().expect("sessions not built")
    }

    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }

    fn project_id(&self, name: &str) -> Uuid {
        *self.projects.get(name).expect("project was seeded earlier")
    }
}

#[given("the Neon Law Navigator app is running")]
async fn build_app(world: &mut CardWorld) {
    let db = in_memory_db().await;
    let runtime = Arc::new(InMemoryRuntime::new());
    let storage = fs_storage("portal-invoice-card").await;
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
async fn seed_person(world: &mut CardWorld, email: String, role: String) {
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
    world: &mut CardWorld,
    project_name: String,
    participant_email: String,
) {
    let entity_id = store::test_support::seed_entity(world.db()).await;
    let __dri = store::test_support::dri_person(world.db()).await;
    let inserted = project::ActiveModel {
        name: ActiveValue::Set(project_name.clone()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert project");
    world.projects.insert(project_name, inserted.id);

    let person_id = *world
        .persons
        .get(&participant_email)
        .expect("participant person was seeded earlier");
    person_project_role::ActiveModel {
        person_id: ActiveValue::Set(person_id),
        project_id: ActiveValue::Set(inserted.id),
        participation: ActiveValue::Set("participant".into()),
        ..Default::default()
    }
    .insert(world.db())
    .await
    .expect("insert person_project_role");
}

#[given(regex = r#"^an AUTHORISED invoice of (\d+) cents is mirrored for "([^"]+)"$"#)]
async fn mirror_invoice(world: &mut CardWorld, amount_cents: i64, project_name: String) {
    let project_id = world.project_id(&project_name);
    xero_invoices::upsert(
        world.db(),
        &UpsertXeroInvoice {
            project_id,
            xero_invoice_id: "INV-TEST-001".into(),
            reference: "Matter close fee".into(),
            status: "AUTHORISED".into(),
            amount_cents,
            currency: "USD".into(),
        },
    )
    .await
    .expect("upsert mirror invoice");
}

#[given(regex = r#"^the invoice for "([^"]+)" is reconciled as paid in full$"#)]
async fn reconcile_paid(world: &mut CardWorld, project_name: String) {
    let project_id = world.project_id(&project_name);
    // Read the mirrored total back, then fold a PAID/paid-in-full
    // result onto it exactly as the reconcile workflow does.
    let row = xero_invoices::for_projects(world.db(), &[project_id])
        .await
        .expect("read mirror")
        .into_iter()
        .next()
        .expect("a mirror row was created earlier");
    xero_invoices::record_reconcile(world.db(), project_id, "PAID", row.amount_cents)
        .await
        .expect("record reconcile")
        .expect("mirror row exists to reconcile");
}

#[when(regex = r#"^"([^"]+)" opens the detail page for "([^"]+)"$"#)]
async fn open_detail(world: &mut CardWorld, email: String, project_name: String) {
    let person_id = *world.persons.get(&email).expect("actor was seeded earlier");
    let project_id = world.project_id(&project_name);
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
                .uri(format!("/portal/projects/{project_id}"))
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
        .expect("query person")
        .expect("person row exists")
        .role
}

#[then(regex = r"^the response status is (\d+)$")]
async fn status_is(world: &mut CardWorld, code: u16) {
    let actual = world.last_status.expect("no response captured");
    assert_eq!(
        actual.as_u16(),
        code,
        "expected {code}, got {} (body: {})",
        actual,
        truncated(&world.last_body)
    );
}

#[then(regex = r#"^the response body contains "([^"]+)"$"#)]
async fn body_contains(world: &mut CardWorld, needle: String) {
    assert!(
        world.last_body.contains(&needle),
        "expected body to contain {needle:?}; body was: {}",
        truncated(&world.last_body)
    );
}

#[then(regex = r#"^the invoice card shows the "([^"]+)" badge$"#)]
async fn card_shows_badge(world: &mut CardWorld, label: String) {
    // The card renders a success "Paid" badge when reconciled paid in
    // full, otherwise a warning "Due" badge (project_detail.rs).
    let class = match label.as_str() {
        "Paid" => "text-bg-success",
        "Due" => "text-bg-warning",
        other => panic!("unknown badge label {other:?}"),
    };
    assert!(
        world.last_body.contains(class) && world.last_body.contains(&label),
        "expected the {label:?} badge ({class}); body was: {}",
        truncated(&world.last_body)
    );
}

#[then("the page shows no invoice card")]
async fn no_invoice_card(world: &mut CardWorld) {
    assert!(
        !world.last_body.contains(">Invoice<"),
        "expected no invoice card, but one rendered; body was: {}",
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
    CardWorld::cucumber()
        .run_and_exit("tests/features/portal_invoice_card.feature")
        .await;
}
