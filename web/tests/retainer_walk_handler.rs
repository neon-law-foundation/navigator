#![allow(clippy::doc_markdown)]
//! Integration tests for the stepwise retainer walker.
//!
//! Covers the full lifecycle:
//!   1. `POST /portal/admin/retainers/new` creates Person + Project +
//!      role + Notation and redirects to `/step`.
//!   2. `GET /portal/admin/notations/:id/step` renders the current
//!      question (read from the runtime + spec).
//!   3. `POST /portal/admin/notations/:id/step` writes the Answer row and
//!      signals the runtime (the runtime — InMemoryRuntime in tests,
//!      the workflows-service worker in production — owns
//!      `notation_events`).
//!   4. The final POST hits END, drives the workflow, and renders
//!      the result page with the rendered retainer.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, EntityTrait};
use store::{entity, seed};
use tower::ServiceExt;
use views::assert_renders;
use web::AppState;
use workflows::{InMemoryRuntime, MachineKind, StateMachineRuntime, StateName};

const TEMPLATE_CODE: &str = "onboarding__retainer";

async fn build_app_and_notation() -> (axum::Router, store::Db, uuid::Uuid, Arc<InMemoryRuntime>) {
    let db = store::test_support::pg().await;
    // Template bodies seed into blob storage; the app reads them back
    // from the same handle, so seed and AppState share one storage.
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-walker-test-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();

    // seed_canonical inserts the bundled `onboarding__retainer`
    // template; reuse it instead of double-inserting (the code
    // column is UNIQUE).
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(TEMPLATE_CODE))
        .one(&db)
        .await
        .unwrap()
        .expect("seed pass inserts onboarding__retainer");

    let libra = entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("Libra retainer".into()),
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
        person_id: ActiveValue::Set(libra.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // Keep a fourth `Arc<InMemoryRuntime>` so the test body can
    // call `runtime.events(MachineKind::Questionnaire, …)` to assert
    // on the recorded transitions — the runtime, not the journal,
    // is the source of truth once the walker stopped writing
    // `notation_events` directly.
    let runtime = Arc::new(InMemoryRuntime::new());
    let runtime_for_assertions = runtime.clone();
    // The `document_open__retainer_pdf` step is worker-dispatched, so
    // wrap the in-memory runtime in `DispatchingRuntime` (the same
    // in-process path the dev binary and feature suite use) — otherwise
    // the PDF is never rendered/persisted and the signature read-back
    // 404s.
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(runtime.clone(), email.clone(), storage.clone()),
    );
    let state = AppState {
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        notation_id,
        runtime_for_assertions,
    )
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn step_get_at_begin_renders_the_first_question() {
    let (app, _db, nid, _runtime) = build_app_and_notation().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/notations/{nid}/step"))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    // First question after BEGIN is the client record.
    assert!(html.contains("person__client"), "html: {html}");
    assert!(html.contains("step 1 of 2"));
    assert!(html.contains(format!("/portal/admin/notations/{nid}/step").as_str()));
}

#[tokio::test]
async fn step_get_prefill_is_scoped_to_current_notation() {
    let (app, db, nid, _runtime) = build_app_and_notation().await;
    let notation = entity::notation::Entity::find_by_id(nid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let other_notation = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(notation.template_id),
        person_id: ActiveValue::Set(notation.person_id),
        entity_id: ActiveValue::Set(notation.entity_id),
        project_id: ActiveValue::Set(notation.project_id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let client_name = entity::question::Entity::find()
        .filter(entity::question::Column::Code.eq("custom_text"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    entity::answer::ActiveModel {
        question_id: ActiveValue::Set(client_name.id),
        person_id: ActiveValue::Set(notation.person_id),
        notation_id: ActiveValue::Set(Some(other_notation.id)),
        state_name: ActiveValue::Set(Some("person__client".into())),
        value: ActiveValue::Set(entity::answer::primitive("Other matter client")),
        source: ActiveValue::Set(entity::answer::SOURCE_STAFF.into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/notations/{nid}/step"))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("person__client"), "html: {html}");
    assert!(
        !html.contains("Other matter client"),
        "stale answer from another notation leaked into prefill: {html}"
    );
}

#[tokio::test]
async fn step_post_writes_answer_signals_runtime_and_redirects_to_next_question() {
    let (app, db, nid, runtime) = build_app_and_notation().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/notations/{nid}/step"))
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("value=Libra"))
                .unwrap(),
        )
        .await
        .unwrap();
    // Redirect to GET /step for the next question.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(location, format!("/portal/admin/notations/{nid}/step"));

    // The runtime saw exactly one transition on the questionnaire
    // timeline: BEGIN → client_name via `_`. The walker no longer
    // writes `notation_events` itself — in production the
    // workflows-service worker journals these via `ctx.run`; in this
    // test the in-memory runtime records them in `Vec<WorkflowEvent>`.
    let events =
        StateMachineRuntime::events(runtime.as_ref(), MachineKind::Questionnaire, nid).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].from, StateName::begin());
    assert_eq!(events[0].to.as_str(), "person__client");
    assert_eq!(events[0].condition, "_");

    // Answer row landed: `answers` is application data, written by
    // the walker (the worker doesn't touch it). Answers are now
    // notation-scoped, so filter by the notation we just walked.
    let our_answers = entity::answer::Entity::find()
        .filter(entity::answer::Column::NotationId.eq(nid))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(our_answers.len(), 1);
    assert_eq!(
        entity::answer::display_value(&our_answers[0].value),
        "Libra"
    );
    assert_eq!(
        our_answers[0].state_name.as_deref(),
        Some("person__client"),
        "the walked state name is recorded on the answer"
    );

    // Next GET asks the staff-side project question.
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/notations/{nid}/step"))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("project__engagement"));
    assert!(html.contains("step 2 of 2"));
}

#[tokio::test]
async fn step_post_for_unknown_notation_returns_404() {
    let (app, _db, _nid, _runtime) = build_app_and_notation().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/admin/notations/{}/step",
                    uuid::Uuid::from_u128(9999)
                ))
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("value=x"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn walking_the_full_questionnaire_records_all_transitions_through_end() {
    let (app, _db, nid, runtime) = build_app_and_notation().await;

    // Walk both questions. The last POST drives the workflow
    // and renders the result page (200); the rest redirect (303).
    for (i, value) in ["Libra", "Estate plan"].iter().enumerate() {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/portal/admin/notations/{nid}/step"))
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("value={}", urlencoding(value))))
                    .unwrap(),
            )
            .await
            .unwrap();
        let expected = if i == 1 {
            StatusCode::OK
        } else {
            StatusCode::SEE_OTHER
        };
        assert_eq!(resp.status(), expected, "value={value}");
    }

    // Runtime: BEGIN → client → project → END = 3
    // events on the questionnaire
    // timeline. The walker no longer writes `notation_events` —
    // in production the workflows-service worker does, via
    // `ctx.run`; here, the InMemoryRuntime is the source of truth.
    let events =
        StateMachineRuntime::events(runtime.as_ref(), MachineKind::Questionnaire, nid).await;
    assert_eq!(
        events.len(),
        3,
        "expected 3 questionnaire transitions, got {events:?}"
    );
    assert_eq!(events.last().unwrap().to, StateName::end());

    // GET after END redirects to /portal/admin (workflow already
    // finished synchronously in the previous POST).
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/notations/{nid}/step"))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/portal/admin")
    );
}

/// Tiny URL-encoder for the test bodies — only escapes the
/// characters the four retainer answers actually contain.
fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20").replace('@', "%40")
}

#[tokio::test]
async fn start_get_renders_the_minimal_create_form() {
    let (app, _db, _nid, _runtime) = build_app_and_notation().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/retainers/new")
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("action=\"/portal/admin/retainers/new\""));
    assert!(html.contains("name=\"client_email\""));
    // The template picker is a dropdown of the onboarding family, not a
    // free-text code field — so staff pick the product, not type a code.
    assert!(html.contains("name=\"retainer_template_code\""));
    assert!(html.contains("form-select"));
    assert!(
        html.contains("onboarding__retainer"),
        "the onboarding retainer should be a selectable option",
    );
    assert!(
        html.contains("onboarding__estate"),
        "every seeded onboarding template should be an option",
    );
    // Only onboarding templates open a matter; a closing letter is not an
    // option here (it belongs to the close flow).
    assert!(
        !html.contains("closing__letter"),
        "the closing letter must not be a matter-open option",
    );
    // The walker collects these; they must NOT be on the create form.
    assert!(!html.contains("name=\"person__client\""));
    assert!(!html.contains("name=\"project_name\""));
}

#[tokio::test]
async fn start_post_creates_person_project_role_notation_and_redirects_to_step() {
    // Fresh app+db (no pre-seeded notation).
    let db = store::test_support::pg().await;
    // seed_canonical inserts the bundled onboarding__retainer
    // template — that's what this test will POST against.
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-walker-start-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();
    let runtime = Arc::new(InMemoryRuntime::new());
    let state = AppState {
        storage,
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/retainers/new")
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "client_email={}&retainer_template_code={TEMPLATE_CODE}",
                    "libra%40example.com"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        loc.starts_with("/portal/admin/notations/"),
        "redirect was {loc:?}"
    );
    assert!(loc.ends_with("/step"));

    // The four rows the walker depends on landed.
    let libra = entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("libra@example.com"))
        .one(&db)
        .await
        .unwrap()
        .expect("person row inserted");
    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("(pending) libra@example.com"))
        .one(&db)
        .await
        .unwrap()
        .expect("project row inserted");
    let role = entity::person_project_role::Entity::find()
        .filter(entity::person_project_role::Column::PersonId.eq(libra.id))
        .filter(entity::person_project_role::Column::ProjectId.eq(project.id))
        .one(&db)
        .await
        .unwrap()
        .expect("role row inserted");
    assert_eq!(role.participation, "client");
    let notations = entity::notation::Entity::find()
        .filter(entity::notation::Column::PersonId.eq(libra.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(notations.len(), 1);
}

#[tokio::test]
async fn close_matter_post_starts_a_closing_walk_for_an_existing_matter() {
    // A matter that already exists with a client — the close acts on
    // it rather than creating it.
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-close-start-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();

    let libra = entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra-close@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let project = entity::project::ActiveModel {
        name: ActiveValue::Set("Libra estate (to close)".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    entity::person_project_role::ActiveModel {
        person_id: ActiveValue::Set(libra.id),
        project_id: ActiveValue::Set(project.id),
        participation: ActiveValue::Set("client".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let runtime = Arc::new(InMemoryRuntime::new());
    let state = AppState {
        storage,
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/projects/{}/close", project.id))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        loc.starts_with("/portal/admin/notations/") && loc.ends_with("/step"),
        "redirect was {loc:?}"
    );

    // A closing__letter notation now hangs off the matter, addressed
    // to its client, at BEGIN — ready to walk.
    let closing_tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq("closing__letter"))
        .one(&db)
        .await
        .unwrap()
        .expect("seed inserts closing__letter");
    let notations = entity::notation::Entity::find()
        .filter(entity::notation::Column::ProjectId.eq(project.id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(notations.len(), 1);
    assert_eq!(notations[0].template_id, closing_tmpl.id);
    assert_eq!(notations[0].person_id, libra.id);
    assert_eq!(notations[0].state, "BEGIN");
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn close_walk_renders_firm_signed_letter_and_closes_the_matter() {
    // An open matter with a client. Walk the close end to end and
    // assert the matter flips to `closed` and the closing letter PDF
    // lands in storage.
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-close-walk-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();

    let libra = entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra-closewalk@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let project = entity::project::ActiveModel {
        name: ActiveValue::Set("Libra estate (closing)".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    entity::person_project_role::ActiveModel {
        person_id: ActiveValue::Set(libra.id),
        project_id: ActiveValue::Set(project.id),
        participation: ActiveValue::Set("client".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // Dispatching runtime with a db so `document_open__closing_letter`
    // renders/persists the PDF and the firm-signature transition runs
    // the `close_matter` side effect (the same in-process path the dev
    // binary uses).
    let inner = Arc::new(InMemoryRuntime::new());
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );
    let state = AppState {
        storage: storage.clone(),
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // Open the close walk.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/projects/{}/close", project.id))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    // /portal/admin/notations/<uuid>/step
    let nid: uuid::Uuid = loc
        .trim_start_matches("/portal/admin/notations/")
        .trim_end_matches("/step")
        .parse()
        .expect("redirect carries the notation id");

    // Walk the six closing questions; the final POST drives the closing
    // workflow to END and redirects to /portal/admin.
    let answers = [
        "Libra",
        "Estate plan",
        "Wound up the LLC",
        "paid_in_full",
        "Kept seven years",
        "None",
    ];
    for (i, value) in answers.iter().enumerate() {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/portal/admin/notations/{nid}/step"))
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("value={}", urlencoding(value))))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER, "answer {i}={value}");
        if i == answers.len() - 1 {
            assert_eq!(
                resp.headers().get("location").and_then(|v| v.to_str().ok()),
                Some("/portal/admin"),
                "final answer should close the matter and return to admin"
            );
        }
    }

    // The matter is closed.
    let row = entity::project::Entity::find_by_id(project.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "closed");

    // The firm-signed closing letter PDF was rendered and persisted.
    let pdf = storage
        .get(&web::retainer_walk::closing_letter_storage_key(nid))
        .await
        .expect("closing letter PDF persisted")
        .bytes;
    assert!(
        pdf.starts_with(b"%PDF"),
        "expected a PDF, got {} bytes",
        pdf.len()
    );
}

#[tokio::test]
async fn start_post_rejects_missing_at_in_client_email_with_validation_error() {
    let (app, _db, _nid, _runtime) = build_app_and_notation().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/retainers/new")
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "client_email=not-an-email&retainer_template_code={TEMPLATE_CODE}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert_renders!(&html, "portal.retainer_client_email_at");
}

#[tokio::test]
async fn final_post_drives_workflow_and_renders_result_with_substituted_template() {
    let (app, db, nid, _runtime) = build_app_and_notation().await;

    // Walk both questions.
    for value in ["Libra", "Estate plan"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/portal/admin/notations/{nid}/step"))
                    .header("authorization", "Bearer dev")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("value={}", urlencoding(value))))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Last POST renders the result page (200); the rest redirect.
        if value == "Estate plan" {
            assert_eq!(resp.status(), StatusCode::OK);
            let html = body_string(resp).await;
            // The result page interpolates the answers into the
            // template body.
            assert!(html.contains("Libra"), "html: {html}");
            assert!(html.contains("Estate plan"));
            assert!(html.contains("sent_for_signature__pending"));
        } else {
            assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        }
    }

    // Notation.state should reflect the workflow's terminal
    // state.
    let row = entity::notation::Entity::find_by_id(nid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "sent_for_signature__pending");
}
