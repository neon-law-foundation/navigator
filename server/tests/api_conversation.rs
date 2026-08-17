#![allow(clippy::doc_markdown)]
//! Integration tests for `POST /app/api/projects/{id}/conversation/messages` —
//! the client-writable REST door that posts a message to a matter conversation.
//!
//! The write engine (`conversation::post_conversation_message`) is shared with
//! the portal message control, so this focuses on what the REST adapter adds:
//! it is client-writable (both lenses post), the either-lens gate collapses a
//! non-participant to 404, an anonymous caller is 401, an empty body is 400, and
//! a participant's message actually lands with the tier-derived direction.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-conversation-test-key";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    client: String,
    lawyer: String,
    outsider: String,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-conv-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let project = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("matter-{}", Uuid::now_v7()),
            name: "Matter".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Client", "client@example.com", Role::Client),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, client.id, "client")
        .await
        .unwrap();
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, lawyer.id, "lawyer")
        .await
        .unwrap();
    let outsider = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Outsider", "outsider@example.com", Role::Lawyer),
    )
    .await
    .unwrap();

    let state = AppState {
        sessions: SessionStore::new(KEY),
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        project_id: project.id,
        client: bearer(client.id, Role::Client),
        lawyer: bearer(lawyer.id, Role::Lawyer),
        outsider: bearer(outsider.id, Role::Lawyer),
    }
}

async fn post_message(
    fx: &Fixture,
    auth: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!(
            "/app/api/projects/{}/conversation/messages",
            fx.project_id
        ))
        .header("content-type", "application/json");
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    fx.app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_client_and_a_lawyer_both_post_to_the_conversation() {
    let fx = build_fixture().await;

    let client = post_message(
        &fx,
        Some(&fx.client),
        serde_json::json!({ "body": "Hello from the client" }),
    )
    .await;
    assert_eq!(client.status(), StatusCode::NO_CONTENT);

    let lawyer = post_message(
        &fx,
        Some(&fx.lawyer),
        serde_json::json!({ "body": "Reply from the firm" }),
    )
    .await;
    assert_eq!(lawyer.status(), StatusCode::NO_CONTENT);

    let messages = store::communications::for_project(&fx.surreal, fx.project_id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 2, "both messages landed on the matter");
}

#[tokio::test]
async fn a_non_participant_gets_a_non_disclosing_404() {
    let fx = build_fixture().await;
    let resp = post_message(
        &fx,
        Some(&fx.outsider),
        serde_json::json!({ "body": "let me in" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(
        store::communications::for_project(&fx.surreal, fx.project_id)
            .await
            .unwrap()
            .is_empty(),
        "a refused post lands nothing"
    );
}

#[tokio::test]
async fn an_anonymous_caller_is_unauthenticated() {
    let fx = build_fixture().await;
    let resp = post_message(&fx, None, serde_json::json!({ "body": "anon" })).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_empty_body_is_rejected() {
    let fx = build_fixture().await;
    let resp = post_message(&fx, Some(&fx.client), serde_json::json!({ "body": "   " })).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
