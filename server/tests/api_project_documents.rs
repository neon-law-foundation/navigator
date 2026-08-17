#![allow(clippy::doc_markdown)]
//! Integration tests for `POST /app/api/projects/{id}/documents` — the REST door
//! that files a document into a matter.
//!
//! The write engine (`matter_documents::record_document`) is shared with the
//! lawyer upload control, so this focuses on what the REST adapter adds: it takes
//! the bytes base64-encoded (not multipart), lawyer-tier only (client 403, anon
//! 401), the matter-scope gate (a non-participant lawyer is 404), undecodable
//! base64 is 400, and a filed document actually lands.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-project-documents-test-key";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    lawyer: String,
    outsider: String,
    client: String,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-doc-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("nav-api-docs-{}", Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );
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
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Client", "client@example.com", Role::Client),
    )
    .await
    .unwrap();
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        project_id: project.id,
        lawyer: bearer(lawyer.id, Role::Lawyer),
        outsider: bearer(outsider.id, Role::Lawyer),
        client: bearer(client.id, Role::Client),
    }
}

async fn upload(
    fx: &Fixture,
    auth: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/app/api/projects/{}/documents", fx.project_id))
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

fn doc_body() -> serde_json::Value {
    // "test document" base64-encoded.
    serde_json::json!({
        "filename": "note.txt",
        "content_base64": "dGVzdCBkb2N1bWVudA==",
        "content_type": "text/plain"
    })
}

#[tokio::test]
async fn a_participant_lawyer_files_a_document() {
    let fx = build_fixture().await;
    let resp = upload(&fx, Some(&fx.lawyer), doc_body()).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let document_id: Uuid = json["document_id"].as_str().unwrap().parse().unwrap();
    assert!(
        store::assets::find_by_id(&fx.surreal, document_id)
            .await
            .unwrap()
            .is_some(),
        "the filed document is a real asset"
    );
}

#[tokio::test]
async fn a_non_participant_lawyer_is_404() {
    let fx = build_fixture().await;
    let resp = upload(&fx, Some(&fx.outsider), doc_body()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_client_is_403_and_anonymous_is_401() {
    let fx = build_fixture().await;
    assert_eq!(
        upload(&fx, Some(&fx.client), doc_body()).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        upload(&fx, None, doc_body()).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn undecodable_base64_is_400() {
    let fx = build_fixture().await;
    let resp = upload(
        &fx,
        Some(&fx.lawyer),
        serde_json::json!({ "filename": "note.txt", "content_base64": "!!! not base64 !!!" }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
