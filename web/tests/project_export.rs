#![allow(clippy::too_many_lines)]
//! Integration tests for the client "download all my documents" export
//! (`GET /portal/projects/:id/documents.zip`).
//!
//! Covers what Task 3 promises:
//!   1. A scoped participant downloads a real ZIP whose entries are the
//!      matter's current files, by their human filenames, with bytes
//!      intact — never a packfile or bundle.
//!   2. A non-participant gets 404 — the matter doesn't exist for them.
//!
//! Documents are filed through the same `matter_documents` seam the
//! portal upload uses, so the repo HEAD the export reads is produced the
//! production way.

use std::io::Read;
use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::documents::{source, IngestArgs};
use store::entity::person::Role;
use store::entity::{person, person_project_role, project};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

/// Process-stable repo root — `NAVIGATOR_GIT_REPO_ROOT` is global, so a
/// single root shared across the parallel tests avoids a set/read race;
/// each test uses its own project id, so repos never collide.
static REPO_ROOT: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", dir.path());
    dir
});

struct Fixture {
    app: axum::Router,
    project_id: Uuid,
    member_cookie: String,
    stranger_cookie: String,
}

async fn build_fixture() -> Fixture {
    LazyLock::force(&REPO_ROOT);

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("nav-export-{}", Uuid::now_v7())))
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
    let stranger = person::ActiveModel {
        name: ActiveValue::Set("Aries".into()),
        email: ActiveValue::Set("aries@example.com".into()),
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

    // File two documents the production way (persist + commit into repo).
    for (filename, bytes) in [
        ("will.pdf", b"the last will and testament".as_slice()),
        ("trust.pdf", b"the revocable living trust".as_slice()),
    ] {
        let args = IngestArgs {
            project_id: proj.id,
            source: source::UPLOAD,
            filename,
            kind: "unclassified",
            content_type: "application/pdf",
            description: None,
            source_revision_id: None,
        };
        web::matter_documents::record_document(
            &db,
            &storage,
            repos::Author {
                name: "Libra",
                email: "libra@example.com",
            },
            &args,
            bytes,
        )
        .await
        .unwrap();
    }

    let sessions = SessionStore::new(KEY);
    let mut member = SessionData::fresh("libra-sub", Role::Client);
    member.person_id = Some(libra.id);
    let member_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&member));
    let mut other = SessionData::fresh("aries-sub", Role::Client);
    other.person_id = Some(stranger.id);
    let stranger_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&other));

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        project_id: proj.id,
        member_cookie,
        stranger_cookie,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_client_downloads_a_zip_of_their_current_documents() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}/documents.zip", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.member_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/zip")
    );
    assert!(resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .contains("libra-estate-plan-documents.zip"));

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    assert_eq!(archive.len(), 2);

    let mut got = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        got.insert(name, content);
    }
    assert_eq!(
        got.get("will.pdf").map(Vec::as_slice),
        Some(b"the last will and testament".as_slice())
    );
    assert_eq!(
        got.get("trust.pdf").map(Vec::as_slice),
        Some(b"the revocable living trust".as_slice())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_participant_gets_404() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}/documents.zip", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.stranger_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
