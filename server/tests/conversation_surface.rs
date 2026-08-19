#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration tests for the matter conversation log
//! (`/app/projects/:project_code/conversation`).
//!
//! The load-bearing guarantee is the privilege boundary: a client reads the
//! conversation but **never** a firm-internal note. This drives the real
//! route with a signed client session and asserts an internal note's body is
//! absent from the response, then asserts a client's posted message lands as
//! an inbound row that lists back.
//!
//! The composer is a plain `<textarea name="body">` that posts natively — there
//! is no rich-text island, so the page loads no editor script and the handler
//! accepts nothing but the plain body.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::{SessionData, SESSION_COOKIE_NAME};
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    project_code: String,
    client_cookie: String,
    client_csrf: String,
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-conversation-test-storage"))
            .await
            .unwrap(),
    );

    let libra = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let proj = store::test_support::seed_project(&surreal, "Libra estate plan").await;
    store::projects::add_participation(&surreal, proj.id, libra.id, "client")
        .await
        .unwrap();

    // One client-visible message and one firm-internal note already on the
    // matter's conversation log.
    store::communications::ingest(
        &surreal,
        &store::communications::IngestArgs {
            project_id: proj.id,
            channel: store::communications::channel::EMAIL_OUTBOUND,
            direction: store::communications::direction::OUTBOUND,
            author_person_id: None,
            counterparty: Some("libra@example.com"),
            subject: Some("Welcome"),
            body: "Welcome to your matter.",
            source_ref: None,
            asset_id: None,
            occurred_at: "2026-06-08T09:00:00Z",
        },
    )
    .await
    .unwrap();
    store::communications::ingest(
        &surreal,
        &store::communications::IngestArgs {
            project_id: proj.id,
            channel: store::communications::channel::PORTAL_MESSAGE,
            direction: store::communications::direction::INTERNAL,
            author_person_id: None,
            counterparty: None,
            subject: None,
            body: "INTERNAL STRATEGY DO NOT SHARE",
            source_ref: None,
            asset_id: None,
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

    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        // The two timelines must share one runtime instance so state
        // advanced on one side is visible on the other.
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        surreal,
        project_id: proj.id,
        project_code: proj.code.clone(),
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
                .uri(format!("/app/projects/{}/conversation", f.project_code))
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
async fn the_composer_is_a_plain_textarea_and_the_page_loads_no_editor_script() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}/conversation", f.project_code))
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;

    // The composer the client types into: a native, required textarea that
    // posts as `body`. Nothing upgrades it.
    assert!(html.contains("<textarea"), "html: {html}");
    assert!(html.contains(r#"name="body""#), "html: {html}");
    assert!(html.contains(r#"id="conversation-body""#), "html: {html}");

    // No rich-text island: no vendored editor bundle, no initializer, and none
    // of the `data-tiptap-*` hooks either would key off.
    assert!(!html.contains("tiptap"), "no editor asset or hook: {html}");
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
                    "/app/projects/{}/conversation/messages",
                    f.project_code
                ))
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    // The composer is a native form post, so the handler redirects back to the
    // Dioxus thread rather than answering with a swap fragment.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Following that redirect, the message is in the rendered thread.
    let thread_resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}/conversation", f.project_code))
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(thread_resp.status(), StatusCode::OK);
    let html = body_string(thread_resp).await;
    assert!(html.contains("Thanks for the update"), "thread: {html}");

    // It persisted as an inbound portal message on the matter's spine.
    let thread = store::communications::for_project(&f.surreal, f.project_id)
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
async fn a_stale_rich_body_field_is_ignored_and_the_row_stays_plain_text() {
    let f = build_fixture().await;
    // A client that still posts the retired rich-document field gets the plain
    // `body` stored verbatim. The handler stopped reading `body_tiptap`, and
    // the column it fed no longer exists — but a stale client (or a replayed
    // request) can still send the field, so the extra parameter must be inert
    // rather than an error.
    let form = format!(
        "_csrf={}&body=Plain+wins&body_tiptap=%7B%22type%22%3A%22doc%22%7D",
        f.client_csrf
    );
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{}/conversation/messages",
                    f.project_code
                ))
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let thread = store::communications::for_project(&f.surreal, f.project_id)
        .await
        .unwrap();
    thread
        .iter()
        .find(|c| c.body == "Plain wins")
        .expect("the plain body is what persisted");
    // Exactly one row: the stale field neither created a second message nor
    // suppressed the real one.
    assert_eq!(
        thread.iter().filter(|c| c.body == "Plain wins").count(),
        1,
        "the retired field must be inert, not duplicating"
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
                    "/app/projects/{}/conversation/messages",
                    f.project_code
                ))
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    // Plain post (no HX-Request) redirects back.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let thread = store::communications::for_project(&f.surreal, f.project_id)
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
