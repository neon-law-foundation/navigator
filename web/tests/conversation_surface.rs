#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration tests for the matter conversation log
//! (`/portal/projects/:id/conversation`).
//!
//! The load-bearing guarantee is the privilege boundary: a client reads the
//! conversation but **never** a firm-internal note. This drives the real
//! route with a signed client session and asserts an internal note's body is
//! absent from the response, then asserts a client's posted message lands as
//! an inbound row that lists back.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::person::Role;
use store::entity::{person, person_project_role, project};
use store::Db;
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    db: Db,
    project_id: Uuid,
    client_cookie: String,
    client_csrf: String,
}

async fn build_fixture() -> Fixture {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-conversation-test-storage"))
            .await
            .unwrap(),
    );

    let libra = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        role: ActiveValue::Set(Role::Client),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Libra estate plan".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    person_project_role::ActiveModel {
        person_id: ActiveValue::Set(libra.id),
        project_id: ActiveValue::Set(proj.id),
        participation: ActiveValue::Set("client".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // One client-visible message and one firm-internal note already on the
    // matter's conversation log.
    store::communications::ingest(
        &db,
        &store::communications::IngestArgs {
            project_id: proj.id,
            channel: store::communications::channel::EMAIL_OUTBOUND,
            direction: store::communications::direction::OUTBOUND,
            author_person_id: None,
            counterparty: Some("libra@example.com"),
            subject: Some("Welcome"),
            body: "Welcome to your matter.",
            source_ref: None,
            blob_id: None,
            occurred_at: "2026-06-08T09:00:00Z",
        },
    )
    .await
    .unwrap();
    store::communications::ingest(
        &db,
        &store::communications::IngestArgs {
            project_id: proj.id,
            channel: store::communications::channel::PORTAL_MESSAGE,
            direction: store::communications::direction::INTERNAL,
            author_person_id: None,
            counterparty: None,
            subject: None,
            body: "INTERNAL STRATEGY DO NOT SHARE",
            source_ref: None,
            blob_id: None,
            occurred_at: "2026-06-08T09:30:00Z",
        },
    )
    .await
    .unwrap();

    let sessions = SessionStore::new(KEY);
    let mut session = SessionData::fresh("libra-sub", Role::Client);
    session.person_id = Some(libra.id);
    let client_csrf = session.csrf_token.clone();
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&session));

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        // The two timelines must share one runtime instance so state
        // advanced on one side is visible on the other.
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        db,
        project_id: proj.id,
        client_cookie,
        client_csrf,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn client_sees_the_conversation_but_never_an_internal_note() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}/conversation", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Welcome to your matter."), "html: {html}");
    assert!(
        !html.contains("INTERNAL STRATEGY DO NOT SHARE"),
        "a client must never see a firm-internal note"
    );
}

#[tokio::test]
async fn client_post_lands_as_an_inbound_message_that_lists_back() {
    let f = build_fixture().await;
    let form = format!("_csrf={}&body=Thanks+for+the+update", f.client_csrf);
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{}/conversation/messages",
                    f.project_id
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("HX-Request", "true")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Thanks for the update"), "fragment: {html}");

    // It persisted as an inbound portal message on the matter's spine.
    let thread = store::communications::for_project(&f.db, f.project_id)
        .await
        .unwrap();
    let posted = thread
        .iter()
        .find(|c| c.body == "Thanks for the update")
        .expect("posted message present");
    assert_eq!(
        posted.channel,
        store::communications::channel::PORTAL_MESSAGE
    );
    assert_eq!(
        posted.direction,
        store::communications::direction::INBOUND,
        "a client's message flows inbound"
    );
}

#[tokio::test]
async fn client_internal_flag_is_ignored() {
    let f = build_fixture().await;
    // A client tries to smuggle internal=1 — it must be ignored; their
    // message still flows inbound (visible to the firm, and to themselves).
    let form = format!("_csrf={}&body=sneaky&internal=1", f.client_csrf);
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{}/conversation/messages",
                    f.project_id
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    // Plain post (no HX-Request) redirects back.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let thread = store::communications::for_project(&f.db, f.project_id)
        .await
        .unwrap();
    let posted = thread
        .iter()
        .find(|c| c.body == "sneaky")
        .expect("posted message present");
    assert_eq!(
        posted.direction,
        store::communications::direction::INBOUND,
        "a client's internal flag must be ignored"
    );
}
