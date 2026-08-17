#![allow(clippy::doc_markdown)]
//! Integration tests for the client document-deletion REST doors:
//! `POST /app/api/expunge-requests/{id}/authorize` (admin) and `.../deny`
//! (lawyer/admin).
//!
//! The write engines (`expunge_request_route::authorize_expunge_request` and
//! `deny_expunge_request`) are shared with the lawyer queue forms, so this
//! focuses on what the REST adapters add: authorize is admin-only (a lawyer is
//! 403), deny is lawyer-tier (a client is 403), anonymous is 401, an unknown or
//! already-resolved request is 404/409, and the live authorize actually scrubs.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::SessionData;
use portal::{AppState, SessionStore};
use store::documents::{source, IngestArgs};
use store::expunge_requests::{STATUS_AUTHORIZED, STATUS_DENIED};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "api-expunge-test-key";

static REPO_ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", dir.path());
    dir
});

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    storage: Arc<dyn cloud::StorageService>,
    project_id: Uuid,
    client_id: Uuid,
    admin: String,
    lawyer: String,
    client: String,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-expunge-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

async fn build_fixture() -> Fixture {
    LazyLock::force(&REPO_ROOT);
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("nav-api-exreq-{}", Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let admin = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Nick", "nick@neonlaw.com", Role::Admin),
    )
    .await
    .unwrap();
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Counsel", "counsel@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    let project = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("libra-estate-{}", Uuid::now_v7()),
            name: "Libra estate plan".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, client.id, "client")
        .await
        .unwrap();

    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        storage,
        project_id: project.id,
        client_id: client.id,
        admin: bearer(admin.id, Role::Admin),
        lawyer: bearer(lawyer.id, Role::Lawyer),
        client: bearer(client.id, Role::Client),
    }
}

/// Ingest a fresh client-visible document and stand up a pending expunge request
/// against it; return the request id.
async fn seed_pending_request(fx: &Fixture) -> Uuid {
    let args = IngestArgs {
        project_id: fx.project_id,
        source: source::UPLOAD,
        filename: "old-draft.pdf",
        kind: "unclassified",
        content_type: "application/pdf",
        description: None,
        secondary_storage_key: None,
        visibility: store::documents::visibility::CLIENT,
    };
    let ingested = portal::matter_documents::record_document(
        &fx.surreal,
        &fx.storage,
        repos::Author {
            name: "Libra",
            email: "libra@example.com",
        },
        &args,
        b"a draft to delete",
    )
    .await
    .unwrap();
    store::expunge_requests::create(
        &fx.surreal,
        &store::expunge_requests::NewExpungeRequest {
            project_id: fx.project_id,
            asset_id: ingested.asset_id,
            requested_by_person_id: fx.client_id,
            note: None,
        },
    )
    .await
    .unwrap()
}

async fn act(
    fx: &Fixture,
    request_id: Uuid,
    action: &str,
    auth: Option<&str>,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/app/api/expunge-requests/{request_id}/{action}"));
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    fx.app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn request_status(fx: &Fixture, request_id: Uuid) -> String {
    store::expunge_requests::by_id(&fx.surreal, request_id)
        .await
        .unwrap()
        .unwrap()
        .status
}

#[tokio::test]
async fn an_admin_authorizes_and_the_document_is_scrubbed() {
    let fx = build_fixture().await;
    let request_id = seed_pending_request(&fx).await;

    let resp = act(&fx, request_id, "authorize", Some(&fx.admin)).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(request_status(&fx, request_id).await, STATUS_AUTHORIZED);

    // Authorizing again is a conflict — it is already resolved.
    let again = act(&fx, request_id, "authorize", Some(&fx.admin)).await;
    assert_eq!(again.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_lawyer_cannot_authorize_an_expunge() {
    let fx = build_fixture().await;
    let request_id = seed_pending_request(&fx).await;

    let resp = act(&fx, request_id, "authorize", Some(&fx.lawyer)).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "authorize is admin-only"
    );
    // Nothing was deleted: the request is still pending.
    assert_eq!(request_status(&fx, request_id).await, "pending");
}

#[tokio::test]
async fn a_client_cannot_authorize_and_anonymous_is_401() {
    let fx = build_fixture().await;
    let request_id = seed_pending_request(&fx).await;

    assert_eq!(
        act(&fx, request_id, "authorize", Some(&fx.client))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        act(&fx, request_id, "authorize", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_lawyer_denies_a_request() {
    let fx = build_fixture().await;
    let request_id = seed_pending_request(&fx).await;

    let resp = act(&fx, request_id, "deny", Some(&fx.lawyer)).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(request_status(&fx, request_id).await, STATUS_DENIED);
}

#[tokio::test]
async fn a_client_cannot_deny_and_anonymous_is_401() {
    let fx = build_fixture().await;
    let request_id = seed_pending_request(&fx).await;

    assert_eq!(
        act(&fx, request_id, "deny", Some(&fx.client))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        act(&fx, request_id, "deny", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn denying_an_unknown_request_is_404() {
    let fx = build_fixture().await;
    let resp = act(&fx, Uuid::now_v7(), "deny", Some(&fx.lawyer)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
