#![allow(clippy::doc_markdown)]
//! Integration tests for `POST /app/api/documents/{id}/deletion-requests` — a
//! client-writable door that records a pending request to delete a document.
//!
//! The command (`request_document_deletion`) is shared with the browser
//! request-deletion form. Like the review-comment door, it admits any
//! authenticated caller (401 only for anon) then enforces **client-lens**
//! matter scope: a matter's client-side participant can ask (201), a firm-side
//! lawyer and a non-participant get a bare 404. It is idempotent — a second ask
//! while one is pending returns the existing request (200).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::documents::{source, IngestArgs};
use store::persons::Role;
use store::seed;
use store::test_support::mem_surreal;
use tower::ServiceExt;

const KEY: &str = "api-deletion-requests-test-key";

fn repo_root() {
    let root = std::env::temp_dir().join("navigator-api-deletion-requests-repos");
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &root);
}

async fn build_app() -> (
    axum::Router,
    store::surreal::SurrealDb,
    Arc<dyn cloud::StorageService>,
) {
    repo_root();
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-api-deletion-requests-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&surreal, &storage).await.unwrap();
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        ..portal::test_support::app_state(surreal.clone()).await
    };
    (
        server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        storage,
    )
}

async fn open_project(surreal: &store::surreal::SurrealDb) -> store::projects::Project {
    store::test_support::seed_project(surreal, "Matter").await
}

async fn actor(
    surreal: &store::surreal::SurrealDb,
    email: &str,
    role: Role,
    project: Option<(uuid::Uuid, &str)>,
) -> String {
    let p = store::persons::create(
        surreal,
        &store::persons::NewPerson::with_role(email, email, role),
    )
    .await
    .unwrap();
    if let Some((project_id, participation)) = project {
        store::projects::add_participation(surreal, project_id, p.id, participation)
            .await
            .unwrap();
    }
    let mut s = SessionData::fresh("api-deletion-sub", role);
    s.person_id = Some(p.id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

/// Record a client-visible document on `project` and return its asset id.
async fn seed_document(
    surreal: &store::surreal::SurrealDb,
    storage: &Arc<dyn cloud::StorageService>,
    project_id: uuid::Uuid,
) -> uuid::Uuid {
    let args = IngestArgs {
        project_id,
        source: source::UPLOAD,
        filename: "old-draft.pdf",
        kind: "unclassified",
        content_type: "application/pdf",
        description: None,
        secondary_storage_key: None,
        visibility: store::documents::visibility::CLIENT,
    };
    portal::matter_documents::record_document(
        surreal,
        storage,
        repos::Author {
            name: "Libra",
            email: "libra@example.com",
        },
        &args,
        b"a draft to delete",
    )
    .await
    .unwrap()
    .asset_id
}

async fn post_request(
    app: &axum::Router,
    auth: Option<&str>,
    doc_id: uuid::Uuid,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/app/api/documents/{doc_id}/deletion-requests"))
        .header("content-type", "application/json");
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn anonymous_is_401() {
    let (app, surreal, storage) = build_app().await;
    let project = open_project(&surreal).await;
    let doc_id = seed_document(&surreal, &storage, project.id).await;

    let resp = post_request(&app, None, doc_id).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn client_participant_requests_and_is_idempotent() {
    let (app, surreal, storage) = build_app().await;
    let project = open_project(&surreal).await;
    let doc_id = seed_document(&surreal, &storage, project.id).await;
    let client = actor(
        &surreal,
        "client@example.com",
        Role::Client,
        Some((project.id, "client")),
    )
    .await;

    // First ask → a fresh pending request (201).
    let resp = post_request(&app, Some(&client), doc_id).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json: serde_json::Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let request_id = json["request_id"]
        .as_str()
        .expect("a request_id")
        .to_string();
    assert_eq!(json["already_pending"], false);

    // Second ask while one is pending → the same request, 200 (idempotent).
    let resp = post_request(&app, Some(&client), doc_id).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(json["request_id"].as_str().unwrap(), request_id);
    assert_eq!(json["already_pending"], true);
}

#[tokio::test]
async fn firm_side_lawyer_is_404_client_lens_gate() {
    let (app, surreal, storage) = build_app().await;
    let project = open_project(&surreal).await;
    let doc_id = seed_document(&surreal, &storage, project.id).await;
    // Lawyer tier, firm-side participation — NOT client-lens.
    let lawyer = actor(
        &surreal,
        "lawyer@example.com",
        Role::Lawyer,
        Some((project.id, "lawyer")),
    )
    .await;

    let resp = post_request(&app, Some(&lawyer), doc_id).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn non_participant_is_404() {
    let (app, surreal, storage) = build_app().await;
    let project = open_project(&surreal).await;
    let doc_id = seed_document(&surreal, &storage, project.id).await;
    let outsider = actor(&surreal, "outsider@example.com", Role::Client, None).await;

    let resp = post_request(&app, Some(&outsider), doc_id).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
