//! Cucumber runner for `features/closing_letter.feature`.
//!
//! Drives the admin walker (`/portal/admin/notations/:id/step`) over a
//! `closing__letter` notation. The walker is generic over the bound
//! template's questionnaire, so this mirrors `retainer_intake.rs` with
//! the closing template's six-question walk — the firm-signed bookend
//! to the client-signed retainer.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{gherkin::Step, given, then, when, World};
use features::{app_state, body_string, form_encode, fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed, Db};
use tower::ServiceExt;
use uuid::Uuid;
use web::{policy::PolicyClient, SessionStore};
use workflows::{InMemoryRuntime, MachineKind, StateMachineRuntime, StateName};

const TEMPLATE_CODE: &str = "closing__letter";

#[derive(Default, World)]
#[world(init = Self::default)]
struct ClosingWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    notation_id: Option<Uuid>,
    runtime: Option<Arc<InMemoryRuntime>>,
    last_status: Option<StatusCode>,
    last_body: String,
    final_status: Option<StatusCode>,
}

impl std::fmt::Debug for ClosingWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosingWorld")
            .field("notation_id", &self.notation_id)
            .field("last_status", &self.last_status)
            .field("final_status", &self.final_status)
            .finish_non_exhaustive()
    }
}

impl ClosingWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app not built").clone()
    }

    fn db(&self) -> &Db {
        self.db.as_ref().expect("db not built")
    }

    fn runtime(&self) -> &Arc<InMemoryRuntime> {
        self.runtime.as_ref().expect("runtime not built")
    }

    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation_id not built")
    }

    fn substitute(&self, uri: &str) -> String {
        uri.replace(":id", &self.notation_id().to_string())
    }
}

#[given("a fresh Neon Law Navigator app with the canonical templates seeded")]
async fn build_app(world: &mut ClosingWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("closing").await;
    seed::seed_canonical(&db, &storage)
        .await
        .expect("seed canonical");
    let runtime = Arc::new(InMemoryRuntime::new());
    let state = app_state(
        db.clone(),
        runtime.clone(),
        storage,
        PolicyClient::passthrough(),
        None,
        SessionStore::new("test-session-key-not-for-production"),
    );
    let router = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    world.app = Some(router);
    world.db = Some(db);
    world.runtime = Some(runtime);
}

#[given(regex = r#"^a closing notation for "([^"]+)" <([^>]+)> at BEGIN$"#)]
async fn seed_notation(world: &mut ClosingWorld, name: String, email: String) {
    let db = world.db().clone();
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed_canonical inserts closing__letter");
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("closing matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation_id = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    world.notation_id = Some(notation_id);
}

#[when(regex = r"^the staff visits (.+)$")]
async fn staff_visits(world: &mut ClosingWorld, path: String) {
    let uri = world.substitute(&path);
    let resp = world
        .app()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    world.last_status = Some(resp.status());
    world.last_body = body_string(resp).await;
}

#[when("the staff submits the full questionnaire:")]
async fn staff_walks_all(world: &mut ClosingWorld, step: &Step) {
    let table = step.table.as_ref().expect("expected a data table");
    // First row is the header (`value`); skip it.
    let mut last_status = StatusCode::OK;
    for row in table.rows.iter().skip(1) {
        let value = row.first().expect("each row carries one cell").as_str();
        let body = format!("value={}", form_encode(value));
        let uri = world.substitute("/portal/admin/notations/:id/step");
        let resp = world
            .app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        last_status = resp.status();
    }
    world.final_status = Some(last_status);
}

#[then(regex = r"^the response status is (\d+)$")]
async fn assert_status(world: &mut ClosingWorld, code: u16) {
    assert_eq!(
        world.last_status.expect("no status captured").as_u16(),
        code,
        "body: {}",
        world.last_body
    );
}

#[then(regex = r"^the final response status is (\d+)$")]
async fn assert_final_status(world: &mut ClosingWorld, code: u16) {
    assert_eq!(
        world
            .final_status
            .expect("no final status captured")
            .as_u16(),
        code,
    );
}

#[then(regex = r#"^the page asks the "([^"]+)" question$"#)]
async fn assert_question(world: &mut ClosingWorld, code: String) {
    assert!(
        world.last_body.contains(&code),
        "expected page to mention {code}, got:\n{}",
        world.last_body
    );
}

#[then(regex = r#"^the page shows "([^"]+)"$"#)]
async fn assert_page_contains(world: &mut ClosingWorld, needle: String) {
    assert!(
        world.last_body.contains(&needle),
        "expected page to contain {needle:?}, got:\n{}",
        world.last_body
    );
}

#[then(regex = r"^the questionnaire runtime has recorded (\d+) transitions?$")]
async fn assert_transitions(world: &mut ClosingWorld, expected: usize) {
    let events = StateMachineRuntime::events(
        world.runtime().as_ref(),
        MachineKind::Questionnaire,
        world.notation_id(),
    )
    .await;
    assert_eq!(events.len(), expected, "events: {events:?}");
}

#[then(regex = r#"^the last transition lands on "([^"]+)"$"#)]
async fn assert_last_state(world: &mut ClosingWorld, name: String) {
    let events = StateMachineRuntime::events(
        world.runtime().as_ref(),
        MachineKind::Questionnaire,
        world.notation_id(),
    )
    .await;
    let last = events.last().expect("at least one transition");
    let expected = if name == "END" {
        StateName::end()
    } else {
        StateName::from(name.as_str())
    };
    assert_eq!(last.to, expected, "events: {events:?}");
}

#[tokio::main]
async fn main() {
    ClosingWorld::cucumber()
        .run_and_exit("tests/features/closing_letter.feature")
        .await;
}
