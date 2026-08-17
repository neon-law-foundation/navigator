#![allow(clippy::doc_markdown)]
//! Integration tests for the contract-review playbook REST doors:
//! `POST /app/api/playbooks` and `PUT /app/api/playbooks/{id}`.
//!
//! The write engines (`store::playbooks::create` / `update_positions`) are
//! shared with the lawyer playbook forms, so this focuses on what the REST
//! adapters add: they take structured positions (not a pipe-delimited textarea),
//! lawyer-tier only (client 403, anon 401), a blank name or empty set is 400, a
//! duplicate name is 409, an unknown playbook is 404, and a create/update lands.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-playbooks-test-key";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    entity_id: Uuid,
    lawyer: String,
    client: String,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-playbook-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let entity_id = store::test_support::seed_entity(&surreal).await;
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Counsel", "counsel@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Client", "client@example.com", Role::Client),
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
        entity_id,
        lawyer: bearer(lawyer.id, Role::Lawyer),
        client: bearer(client.id, Role::Client),
    }
}

fn position(topic: &str) -> serde_json::Value {
    serde_json::json!({
        "topic": topic,
        "preferred": "preferred",
        "fallback": "fallback",
        "walkaway": "walkaway",
        "severity": "high"
    })
}

async fn post_create(
    fx: &Fixture,
    auth: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/app/api/playbooks")
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

async fn put_update(
    fx: &Fixture,
    id: Uuid,
    auth: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("PUT")
        .uri(format!("/app/api/playbooks/{id}"))
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

async fn json(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn a_lawyer_creates_then_updates_a_playbook() {
    let fx = build_fixture().await;
    let create = post_create(
        &fx,
        Some(&fx.lawyer),
        serde_json::json!({
            "entity_id": fx.entity_id,
            "name": "Vendor MSA",
            "positions": [position("Liability"), position("Governing law")]
        }),
    )
    .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let id: Uuid = json(create).await["playbook_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let stored = store::playbooks::by_id(&fx.surreal, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(store::playbooks::positions_of(&stored).unwrap().len(), 2);

    // Replace with a single position.
    let update = put_update(
        &fx,
        id,
        Some(&fx.lawyer),
        serde_json::json!({ "positions": [position("Term")] }),
    )
    .await;
    assert_eq!(update.status(), StatusCode::NO_CONTENT);
    let stored = store::playbooks::by_id(&fx.surreal, id)
        .await
        .unwrap()
        .unwrap();
    let positions = store::playbooks::positions_of(&stored).unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].topic, "Term");
}

#[tokio::test]
async fn a_duplicate_name_is_a_conflict() {
    let fx = build_fixture().await;
    let body = serde_json::json!({
        "entity_id": fx.entity_id,
        "name": "Vendor MSA",
        "positions": [position("Liability")]
    });
    assert_eq!(
        post_create(&fx, Some(&fx.lawyer), body.clone())
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        post_create(&fx, Some(&fx.lawyer), body).await.status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn a_blank_name_or_empty_positions_is_400() {
    let fx = build_fixture().await;
    let blank_name = post_create(
        &fx,
        Some(&fx.lawyer),
        serde_json::json!({ "entity_id": fx.entity_id, "name": "  ", "positions": [position("Liability")] }),
    )
    .await;
    assert_eq!(blank_name.status(), StatusCode::BAD_REQUEST);

    let empty = post_create(
        &fx,
        Some(&fx.lawyer),
        serde_json::json!({ "entity_id": fx.entity_id, "name": "Empty", "positions": [] }),
    )
    .await;
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn updating_an_unknown_playbook_is_404() {
    let fx = build_fixture().await;
    let resp = put_update(
        &fx,
        Uuid::now_v7(),
        Some(&fx.lawyer),
        serde_json::json!({ "positions": [position("Term")] }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_client_is_403_and_anonymous_is_401() {
    let fx = build_fixture().await;
    let body = serde_json::json!({
        "entity_id": fx.entity_id,
        "name": "Vendor MSA",
        "positions": [position("Liability")]
    });
    assert_eq!(
        post_create(&fx, Some(&fx.client), body.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post_create(&fx, None, body).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
