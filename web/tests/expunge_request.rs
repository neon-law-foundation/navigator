#![allow(clippy::too_many_lines)]
//! Integration tests for client-initiated document deletion
//! (git-repos surfaces Task 2): a client requests deletion, a staff/admin
//! authorizes, and the document is scrubbed.
//!
//! Covers:
//!   1. The client portal shows the "Delete this document" control; the
//!      client POSTs a request; the page then shows "Deletion requested";
//!      an admin authorizes it and the bytes + audit row + request status
//!      all reflect a completed governed expunge (category `client_request`).
//!   2. A non-admin cannot authorize — the request stays pending and
//!      nothing is deleted.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::documents::{source, IngestArgs};
use store::entity::expunge_request::{STATUS_AUTHORIZED, STATUS_PENDING};
use store::entity::person::Role;
use store::entity::{blob, expunge_record, person, person_project_role, project};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

static REPO_ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", dir.path());
    dir
});

struct Fixture {
    app: axum::Router,
    db: store::Db,
    storage: Arc<dyn cloud::StorageService>,
    project_id: Uuid,
    doc_id: Uuid,
    storage_key: String,
    client_cookie: String,
    client_csrf: String,
    admin_cookie: String,
    admin_csrf: String,
}

async fn build_fixture() -> Fixture {
    LazyLock::force(&REPO_ROOT);

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("nav-exreq-{}", Uuid::now_v7())))
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
    let admin = person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        role: ActiveValue::Set(Role::Admin),
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

    let args = IngestArgs {
        project_id: proj.id,
        source: source::UPLOAD,
        filename: "old-draft.pdf",
        kind: "unclassified",
        content_type: "application/pdf",
        description: None,
        source_revision_id: None,
    };
    let ingested = web::matter_documents::record_document(
        &db,
        &storage,
        repos::Author {
            name: "Libra",
            email: "libra@example.com",
        },
        &args,
        b"a draft to delete",
    )
    .await
    .unwrap();
    let doc_id = ingested.document_id;
    let storage_key = blob::Entity::find_by_id(ingested.blob_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .storage_key;

    let sessions = SessionStore::new(KEY);
    let mut client = SessionData::fresh("libra-sub", Role::Client);
    client.person_id = Some(libra.id);
    let client_csrf = client.csrf_token.clone();
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&client));
    let mut admin_session = SessionData::fresh("nick-sub", Role::Admin);
    admin_session.person_id = Some(admin.id);
    let admin_csrf = admin_session.csrf_token.clone();
    let admin_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&admin_session));

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        db,
        storage,
        project_id: proj.id,
        doc_id,
        storage_key,
        client_cookie,
        client_csrf,
        admin_cookie,
        admin_csrf,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn get(f: &Fixture, uri: String, cookie: &str) -> axum::http::Response<Body> {
    f.app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", "Bearer dev")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn post(f: &Fixture, uri: String, cookie: &str, form: String) -> axum::http::Response<Body> {
    f.app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", "Bearer dev")
                .header("cookie", cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_requests_then_admin_authorizes_and_document_is_scrubbed() {
    let f = build_fixture().await;

    // Client sees the delete control on their matter page.
    let page = get(
        &f,
        format!("/portal/projects/{}", f.project_id),
        &f.client_cookie,
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let html = body_string(page).await;
    assert!(html.contains("Delete this document"), "html: {html}");
    assert!(html.contains(&format!(
        "/portal/projects/{}/documents/{}/request-deletion",
        f.project_id, f.doc_id
    )));

    // Client requests deletion.
    let resp = post(
        &f,
        format!(
            "/portal/projects/{}/documents/{}/request-deletion",
            f.project_id, f.doc_id
        ),
        &f.client_cookie,
        format!("_csrf={}", f.client_csrf),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // A pending request now exists, and the page reflects it.
    let pending = store::expunge_requests::pending_for_document(&f.db, f.doc_id)
        .await
        .unwrap();
    assert!(pending.is_some(), "a pending request should exist");
    let page = get(
        &f,
        format!("/portal/projects/{}", f.project_id),
        &f.client_cookie,
    )
    .await;
    let html = body_string(page).await;
    assert!(html.contains("Deletion requested"), "html: {html}");

    // Admin sees it in the queue.
    let queue = get(&f, "/portal/admin/expunge-requests".into(), &f.admin_cookie).await;
    assert_eq!(queue.status(), StatusCode::OK);
    let html = body_string(queue).await;
    assert!(html.contains("old-draft.pdf"));
    assert!(html.contains("Authorize deletion"));

    // Admin authorizes → the governed expunge runs.
    let request_id = pending.unwrap().id;
    let resp = post(
        &f,
        format!("/portal/admin/expunge-requests/{request_id}/authorize"),
        &f.admin_cookie,
        format!("_csrf={}", f.admin_csrf),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Audit row written with the client_request category.
    let records = expunge_record::Entity::find().all(&f.db).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].category, expunge_record::CATEGORY_CLIENT_REQUEST);

    // Bytes gone from object storage.
    assert!(matches!(
        f.storage.get(&f.storage_key).await,
        Err(cloud::StorageError::NotFound(_))
    ));

    // Request marked authorized + linked to the audit row.
    let req = store::expunge_requests::by_id(&f.db, request_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req.status, STATUS_AUTHORIZED);
    assert_eq!(req.expunge_record_id, Some(records[0].id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_admin_cannot_authorize() {
    let f = build_fixture().await;

    // Stand up a pending request directly.
    let request_id = store::expunge_requests::create(
        &f.db,
        &store::expunge_requests::NewExpungeRequest {
            project_id: f.project_id,
            document_id: f.doc_id,
            requested_by_person_id: store::entity::person::Entity::find()
                .all(&f.db)
                .await
                .unwrap()
                .into_iter()
                .find(|p| p.role == Role::Client)
                .unwrap()
                .id,
            note: None,
        },
    )
    .await
    .unwrap();

    // The client tries to authorize → 404 (admin-only), nothing deleted.
    let resp = post(
        &f,
        format!("/portal/admin/expunge-requests/{request_id}/authorize"),
        &f.client_cookie,
        format!("_csrf={}", f.client_csrf),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = store::expunge_requests::by_id(&f.db, request_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(req.status, STATUS_PENDING);
    assert!(f.storage.get(&f.storage_key).await.is_ok());
    assert_eq!(
        expunge_record::Entity::find()
            .all(&f.db)
            .await
            .unwrap()
            .len(),
        0
    );
}
