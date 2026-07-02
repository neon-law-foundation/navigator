#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration test for the Northstar estate review gates (seam 4):
//! the attorney releases the generated drafts (draft → pending_review,
//! staff_review → client_review), then the client approves the plan
//! (pending_review → approved, client_review → sent_for_signature__pending).
//!
//! Also pins the human-in-the-loop boundary: a client cannot approve
//! before an attorney has released every draft.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::entity::person::Role;
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";
const BOUNDARY: &str = "navigatorgatesboundary";

struct Fixture {
    app: axum::Router,
    db: store::Db,
    sessions: SessionStore,
}

async fn build() -> Fixture {
    let repo_root = std::env::temp_dir().join(format!(
        "navigator-estate-gates-repos-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-gates-test"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&db, &storage)
        .await
        .expect("canonical seed");

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    Fixture {
        app: web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        sessions: SessionStore::new(KEY),
    }
}

fn cookie_for(sessions: &SessionStore, role: Role, person_id: Option<Uuid>) -> (String, String) {
    let mut s = SessionData::fresh("sub", role);
    s.person_id = person_id;
    let csrf = s.csrf_token.clone();
    (
        format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&s)),
        csrf,
    )
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn multipart_text(name: &str, value: &str) -> Vec<u8> {
    format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n")
        .into_bytes()
}

#[tokio::test]
async fn attorney_releases_drafts_then_client_approves_the_plan() {
    let f = build().await;

    // Create the estate matter and upload the transcript (→ staff_review).
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/retainers/new")
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "client_email=capricorn%40example.com&retainer_template_code=onboarding__estate",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let project_id = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let notation = store::entity::notation::Entity::find()
        .filter(store::entity::notation::Column::ProjectId.eq(project_id))
        .one(&f.db)
        .await
        .unwrap()
        .unwrap();
    let client_id = notation.person_id;

    f.app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{project_id}/notations/{}/transcript",
                    notation.id
                ))
                .header("authorization", "Bearer dev")
                .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
                .body(Body::from(multipart_text(
                    "transcript_text",
                    "Consent given. Testator: Capricorn. Executor: Aries. Successor trustee: Gemini. \
                     Residuary beneficiary: Leo. Health-care agent: Virgo. Financial agent: Libra.",
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    let (client_cookie, client_csrf) = cookie_for(&f.sessions, Role::Client, Some(client_id));

    // Gate: the client cannot approve while drafts are still at `draft`
    // (the attorney has not released them) — 404.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/projects/{project_id}/approve-plan"))
                .header("authorization", "Bearer dev")
                .header("cookie", &client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_csrf={client_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "client must not approve before the attorney releases the drafts"
    );

    // The attorney (admin bypasses row-scoping) releases the drafts.
    let (admin_cookie, admin_csrf) = cookie_for(&f.sessions, Role::Admin, None);
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/admin/notations/{}/release-drafts",
                    notation.id
                ))
                .header("authorization", "Bearer dev")
                .header("cookie", &admin_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_csrf={admin_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let notation = store::entity::notation::Entity::find_by_id(notation.id)
        .one(&f.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "client_review");
    let drafts = store::review_documents::for_notation(&f.db, notation.id)
        .await
        .unwrap();
    assert!(drafts
        .iter()
        .all(|d| d.status == store::entity::review_document::STATUS_PENDING_REVIEW));

    // Now the client's matter page offers the approve control.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{project_id}"))
                .header("authorization", "Bearer dev")
                .header("cookie", &client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_string(resp).await.contains("Approve my plan"));

    // The client approves: → sent_for_signature__pending, all approved.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/projects/{project_id}/approve-plan"))
                .header("authorization", "Bearer dev")
                .header("cookie", &client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_csrf={client_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let notation = store::entity::notation::Entity::find_by_id(notation.id)
        .one(&f.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "sent_for_signature__pending");
    let drafts = store::review_documents::for_notation(&f.db, notation.id)
        .await
        .unwrap();
    assert!(
        drafts
            .iter()
            .all(|d| d.status == store::entity::review_document::STATUS_APPROVED),
        "every instrument is approved once the client signs off"
    );
}
