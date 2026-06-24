#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration tests for the Northstar comment-only review surface
//! (`/portal/projects/:id/review/:doc_id`).
//!
//! Covers the three things the surface promises:
//!   1. A scoped client sees an attorney-advanced draft and its body.
//!   2. A `draft`-status document 404s — the human-in-the-loop gate
//!      (no client-facing auto-generated legal document).
//!   3. A client can post an anchored comment, which then lists back.
//!
//! The surface is row-scoped exactly like the rest of `/portal`, so the
//! test drives it with a real signed session cookie for a person who has
//! a `person_project_roles` row on the matter.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::person::Role;
use store::entity::{notation, person, person_project_role, project, template};
use store::review_documents::{self, NewReviewDocument};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    project_id: Uuid,
    pending_doc: Uuid,
    draft_doc: Uuid,
    cookie: String,
    csrf: String,
}

async fn build_fixture() -> Fixture {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-review-test-storage"))
            .await
            .unwrap(),
    );

    let tmpl = template::ActiveModel {
        code: ActiveValue::Set("onboarding__estate".into()),
        title: ActiveValue::Set("Estate Plan".into()),
        respondent_type: ActiveValue::Set("person".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let libra = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        role: ActiveValue::Set(Role::Client),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Libra estate plan".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
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
    let nid = notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(libra.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // One advanced draft the client may read, one still at `draft`.
    let pending_doc = review_documents::create(
        &db,
        &NewReviewDocument {
            notation_id: nid,
            kind: "will",
            title: "Last Will and Testament",
            body_html: "<h2>Article I</h2><p>I, Libra, declare this my will.</p>",
        },
    )
    .await
    .unwrap();
    review_documents::set_status(
        &db,
        pending_doc,
        store::entity::review_document::STATUS_PENDING_REVIEW,
    )
    .await
    .unwrap();
    let draft_doc = review_documents::create(
        &db,
        &NewReviewDocument {
            notation_id: nid,
            kind: "trust",
            title: "Revocable Living Trust",
            body_html: "<p>Draft trust, not yet reviewed.</p>",
        },
    )
    .await
    .unwrap();

    // A real signed session cookie for Libra with a known CSRF token.
    let sessions = SessionStore::new(KEY);
    let mut session = SessionData::fresh("libra-sub", Role::Client);
    session.person_id = Some(libra.id);
    let csrf = session.csrf_token.clone();
    let cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&session));

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
        pending_doc,
        draft_doc,
        cookie,
        csrf,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn scoped_client_sees_advanced_draft_and_its_body() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{}/review/{}",
                    f.project_id, f.pending_doc
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Last Will and Testament"), "html: {html}");
    assert!(html.contains("<h2>Article I</h2>"));
    assert!(html.contains("<northstar-review"));
}

#[tokio::test]
async fn draft_status_document_is_hidden_from_the_client() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{}/review/{}",
                    f.project_id, f.draft_doc
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn client_can_post_an_anchored_comment_that_lists_back() {
    let f = build_fixture().await;
    let form = format!(
        "_csrf={}&anchor_start=3&anchor_end=8&quoted_text=Libra&body=Use+my+full+legal+name",
        f.csrf
    );
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{}/review/{}/comments",
                    f.project_id, f.pending_doc
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_string(resp).await;
    assert!(json.contains("Use my full legal name"), "json: {json}");
    assert!(json.contains("\"quoted_text\":\"Libra\""));

    // And it lists back on a fresh GET.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{}/review/{}/comments",
                    f.project_id, f.pending_doc
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_string(resp).await;
    assert!(json.contains("Use my full legal name"));
    assert!(json.contains("\"author\":\"Libra\""));
}

#[tokio::test]
async fn comment_post_without_csrf_is_rejected() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{}/review/{}/comments",
                    f.project_id, f.pending_doc
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "anchor_start=3&anchor_end=8&quoted_text=Libra&body=nope",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
