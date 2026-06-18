#![allow(clippy::too_many_lines)]
//! Integration tests for the admin governed-expunge surface
//! (`/portal/admin/documents/:doc_id/expunge`).
//!
//! Covers what Task 1 promises:
//!   1. An admin sees the confirmation screen naming the document.
//!   2. An admin POST drives the primitive end-to-end — history
//!      rewritten, bytes deleted, audit row written — and the result
//!      page shows the audit-row id.
//!   3. A non-admin (client) session 404s on both the GET and the POST,
//!      and nothing is touched.
//!
//! The surface lives under the admin sub-router, so the test drives it
//! with a real signed session cookie + CSRF token, exactly like the rest
//! of `/portal`. A matter repo is filed via the same `matter_documents`
//! seam the portal upload uses, so the `documents`/`blobs` rows and the
//! committed repo file are produced the production way.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::documents::{source, IngestArgs};
use store::entity::person::Role;
use store::entity::{blob, document, expunge_record, person, project};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

/// One repo root for the whole test binary. `NAVIGATOR_GIT_REPO_ROOT`
/// is process-global, so per-test tempdirs would race across the
/// parallel tests (one test's value overwriting another's between the
/// commit and the later history rewrite). A single stable root sidesteps
/// the race; each test uses its own project id, so the repos never
/// collide under it.
static REPO_ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", dir.path());
    dir
});

struct Fixture {
    app: axum::Router,
    db: store::Db,
    storage: Arc<dyn cloud::StorageService>,
    doc_id: Uuid,
    storage_key: String,
    admin_cookie: String,
    admin_csrf: String,
    client_cookie: String,
    client_csrf: String,
}

async fn build_fixture() -> Fixture {
    // The matter-documents seam commits into a real repo when
    // NAVIGATOR_GIT_REPO_ROOT is set; the binary-wide stable root is set
    // on first access here.
    LazyLock::force(&REPO_ROOT);

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("nav-expunge-route-{}", Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );

    let admin = person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        role: ActiveValue::Set(Role::Admin),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let client = person::ActiveModel {
        name: ActiveValue::Set("Aries".into()),
        email: ActiveValue::Set("aries@example.com".into()),
        role: ActiveValue::Set(Role::Client),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Aries matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // File a document the production way: persist (blob + document rows)
    // + commit into the matter repo.
    let bytes = b"privileged material";
    let args = IngestArgs {
        project_id: proj.id,
        source: source::UPLOAD,
        filename: "privileged.pdf",
        kind: "unclassified",
        content_type: "application/pdf",
        description: None,
        source_revision_id: None,
    };
    let ingested = web::matter_documents::record_document(
        &db,
        &storage,
        repos::Author {
            name: "Aries",
            email: "aries@example.com",
        },
        &args,
        bytes,
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
    let mut admin_session = SessionData::fresh("nick-sub", Role::Admin);
    admin_session.person_id = Some(admin.id);
    let admin_csrf = admin_session.csrf_token.clone();
    let admin_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&admin_session));

    let mut client_session = SessionData::fresh("aries-sub", Role::Client);
    client_session.person_id = Some(client.id);
    let client_csrf = client_session.csrf_token.clone();
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&client_session));

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
        doc_id,
        storage_key,
        admin_cookie,
        admin_csrf,
        client_cookie,
        client_csrf,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_sees_the_confirmation_screen() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/documents/{}/expunge", f.doc_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("privileged.pdf"), "html: {html}");
    assert!(html.contains("rewrites the matter's history"));
    assert!(html.contains("value=\"sealing\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_post_expunges_and_shows_the_audit_row() {
    let f = build_fixture().await;
    let form = format!(
        "_csrf={}&category=sealing&note=docket+24-CV-1",
        f.admin_csrf
    );
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/documents/{}/expunge", f.doc_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.admin_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Document expunged"), "html: {html}");

    // The audit row exists, scoped to this expunge, and the page shows it.
    let rows = expunge_record::Entity::find().all(&f.db).await.unwrap();
    assert_eq!(rows.len(), 1, "exactly one audit row written");
    let row = &rows[0];
    assert_eq!(row.category, expunge_record::CATEGORY_SEALING);
    assert_eq!(row.path, "privileged.pdf");
    assert_eq!(row.note.as_deref(), Some("docket 24-CV-1"));
    assert!(
        html.contains(&row.id.to_string()),
        "audit id shown on the page"
    );

    // The bytes are gone from object storage.
    assert!(matches!(
        f.storage.get(&f.storage_key).await,
        Err(cloud::StorageError::NotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_category_is_rejected_without_expunging() {
    let f = build_fixture().await;
    let form = format!("_csrf={}&category=whoops&note=", f.admin_csrf);
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/documents/{}/expunge", f.doc_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.admin_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    // Nothing expunged, document bytes intact.
    assert_eq!(
        expunge_record::Entity::find()
            .all(&f.db)
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(f.storage.get(&f.storage_key).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_admin_cannot_see_or_run_the_expunge() {
    let f = build_fixture().await;

    // GET → 404 for a client.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/documents/{}/expunge", f.doc_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // POST → 404 for a client, and nothing is touched.
    let form = format!("_csrf={}&category=sealing&note=", f.client_csrf);
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/documents/{}/expunge", f.doc_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        expunge_record::Entity::find()
            .all(&f.db)
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(f.storage.get(&f.storage_key).await.is_ok());
    // Document row still present.
    assert!(document::Entity::find_by_id(f.doc_id)
        .one(&f.db)
        .await
        .unwrap()
        .is_some());
}
