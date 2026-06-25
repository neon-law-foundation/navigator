//! Cucumber runner for `features/intake_language.feature`.
//!
//! Mirrors `retainer_intake` but for a Spanish-speaking client: the
//! person's `preferred_language` is `es`, so the web questionnaire
//! walker renders the attorney-reviewed Spanish prompts (seeded into
//! `question_translations` by `seed_canonical`) and the walk reaches END.

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

const TEMPLATE_CODE: &str = "onboarding__retainer";

#[derive(Default, World)]
#[world(init = Self::default)]
struct LangWorld {
    app: Option<axum::Router>,
    db: Option<Db>,
    runtime: Option<Arc<InMemoryRuntime>>,
    notation_id: Option<Uuid>,
    last_status: Option<StatusCode>,
    last_body: String,
    final_status: Option<StatusCode>,
}

impl std::fmt::Debug for LangWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LangWorld")
            .field("notation_id", &self.notation_id)
            .field("last_status", &self.last_status)
            .finish_non_exhaustive()
    }
}

impl LangWorld {
    fn app(&self) -> axum::Router {
        self.app.as_ref().expect("app").clone()
    }
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db")
    }
    fn runtime(&self) -> &Arc<InMemoryRuntime> {
        self.runtime.as_ref().expect("runtime")
    }
    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation")
    }
    fn substitute(&self, uri: &str) -> String {
        uri.replace(":id", &self.notation_id().to_string())
    }
}

#[given("a fresh Neon Law Navigator app with the canonical templates seeded")]
async fn build_app(world: &mut LangWorld) {
    let db = in_memory_db().await;
    let storage = fs_storage("intake-language").await;
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
    world.app = Some(web::build_router(
        state,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    ));
    world.db = Some(db);
    world.runtime = Some(runtime);
}

#[given(
    regex = r#"^a Spanish-speaking client "([^"]+)" <([^>]+)> with a retainer notation at BEGIN$"#
)]
async fn seed_spanish_notation(world: &mut LangWorld, name: String, email: String) {
    let db = world.db().clone();
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed inserts onboarding__retainer");
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set(name),
        email: ActiveValue::Set(email),
        preferred_language: ActiveValue::Set("es".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("asunto de retención".into()),
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
async fn staff_visits(world: &mut LangWorld, path: String) {
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
async fn staff_walks_all(world: &mut LangWorld, step: &Step) {
    let table = step.table.as_ref().expect("data table");
    let mut last_status = StatusCode::OK;
    for row in table.rows.iter().skip(1) {
        let value = row.first().expect("one cell").as_str();
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
async fn assert_status(world: &mut LangWorld, code: u16) {
    assert_eq!(
        world.last_status.expect("status").as_u16(),
        code,
        "body: {}",
        world.last_body
    );
}

#[then(regex = r#"^the page shows "([^"]+)"$"#)]
async fn assert_page_shows(world: &mut LangWorld, needle: String) {
    assert!(
        world.last_body.contains(&needle),
        "expected page to contain {needle:?}, got:\n{}",
        world.last_body
    );
}

#[then(regex = r"^the final response status is (\d+)$")]
async fn assert_final_status(world: &mut LangWorld, code: u16) {
    assert_eq!(world.final_status.expect("final status").as_u16(), code);
}

#[then(regex = r#"^the last questionnaire transition lands on "([^"]+)"$"#)]
async fn assert_last_state(world: &mut LangWorld, name: String) {
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
    LangWorld::cucumber()
        .run("tests/features/intake_language.feature")
        .await;
}
