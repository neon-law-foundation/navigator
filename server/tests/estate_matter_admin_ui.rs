#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration test for the Northstar estate admin matter page (seam 2).
//!
//! `GET /app/projects/:code` renders the lawyer matter page. For a
//! transcript-driven estate matter parked at `BEGIN`, that page must
//! carry the phone-friendly transcript-upload form pointing at the
//! shipped handler — but only for lawyer **disclosed to the matter**
//! (a `person_project_roles` row). A lawyer who is not on the
//! matter gets `404`, never a peek: the matter does not exist for them.

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
    project_id: Uuid,
    project_code: String,
    notation_id: Uuid,
    /// A lawyer disclosed to the matter (has a person_project_roles
    /// row) — sees the matter page and the transcript form.
    disclosed_cookie: String,
    /// A lawyer NOT on the matter — gets 404.
    outsider_cookie: String,
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-admin-ui-test"))
            .await
            .unwrap(),
    );

    let tmpl = store::templates::save_version(
        &surreal,
        None,
        "onboarding__estate",
        store::templates::Version {
            title: "Northstar Estate Plan".into(),
            respondent_type: "person".into(),
            asset_id: None,
            form_code: None,
            kind: None,
            source_commit_sha: None,
        },
    )
    .await
    .unwrap()
    .into_model();
    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Capricorn", "capricorn@example.com", Role::Client),
    )
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&surreal).await;
    let proj = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: "capricorn-estate-plan".into(),
            name: "Capricorn estate plan".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let notation_id = store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(tmpl.id, client.id, proj.id, "BEGIN"),
    )
    .await
    .unwrap()
    .id;

    // A lawyer disclosed to the matter: a person_project_roles row.
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer Member", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, proj.id, lawyer.id, "lawyer")
        .await
        .unwrap();

    // A second lawyer with NO row on this matter — the outsider.
    let outsider = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Other Lawyer", "other@example.com", Role::Lawyer),
    )
    .await
    .unwrap();

    let sessions = SessionStore::new(KEY);
    let mut disclosed = SessionData::fresh("lawyer-sub", Role::Lawyer);
    disclosed.person_id = Some(lawyer.id);
    let disclosed_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&disclosed));
    let mut out = SessionData::fresh("outsider-sub", Role::Lawyer);
    out.person_id = Some(outsider.id);
    let outsider_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&out));

    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        project_id: proj.id,
        project_code: proj.code,
        notation_id,
        disclosed_cookie,
        outsider_cookie,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn disclosed_lawyer_sees_the_transcript_upload_form_at_begin() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}", f.project_code))
                .header("cookie", &f.disclosed_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Estate plan — Northstar"), "html: {html}");
    assert!(&html.contains("File the sitting transcript"));
    assert!(html.contains(&format!(
        "action=\"/app/projects/{}/notations/{}/transcript\"",
        f.project_id, f.notation_id
    )));
    assert!(html.contains("enctype=\"multipart/form-data\""));
    // The multipart transcript form carries the hidden CSRF token as a
    // field so the upload handler can verify it — a tokenless render would
    // 403 every real submit (see `portal::csrf::require_multipart_csrf`).
    assert!(html.contains("name=\"_csrf\""), "html: {html}");
}

#[tokio::test]
async fn lawyer_not_disclosed_to_the_matter_gets_404() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}", f.project_code))
                .header("cookie", &f.outsider_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
