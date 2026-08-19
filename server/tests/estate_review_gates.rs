#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration test for the Northstar estate review gates (seam 4):
//! the attorney releases the generated drafts (draft → pending_review,
//! lawyer_review → client_review), then the client approves the plan
//! (pending_review → approved, client_review → sent_for_signature__pending).
//!
//! Also pins the human-in-the-loop boundary: a client cannot approve
//! before an attorney has released every draft.

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
const BOUNDARY: &str = "navigatorgatesboundary";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    sessions: SessionStore,
}

async fn build() -> Fixture {
    let repo_root = std::env::temp_dir().join(format!(
        "navigator-estate-gates-repos-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-gates-test"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&surreal, &storage)
        .await
        .expect("canonical seed");

    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_store(surreal.clone()),
    );
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
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

/// A single-field multipart body preceded by the CSRF token — the shape
/// the upload forms render, with `_csrf` first so the handler verifies it
/// before reading the field (see `portal::csrf::require_multipart_csrf`).
fn multipart_text(csrf: &str, name: &str, value: &str) -> Vec<u8> {
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{csrf}\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn attorney_releases_drafts_then_client_approves_the_plan() {
    let f = build().await;

    // Create the estate matter and upload the transcript (→ lawyer_review).
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/lawyer/retainers/new")
                .header("authorization", portal::test_support::lawyer_bearer_header())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "client_email=capricorn%40example.com&retainer_template_code=onboarding__estate",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let project_code = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let project_id = store::projects::find_by_code(&f.surreal, &project_code)
        .await
        .unwrap()
        .expect("redirected project exists")
        .id;
    let notation = store::notations::list_by_project(&f.surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let client_id = notation.person_id;

    // The releasing attorney is on the matter: no tier bypasses participation
    // on the matter surface since ENG-81.
    let releasing_admin = store::persons::create(
        &f.surreal,
        &store::persons::NewPerson::with_role(
            "Releasing Admin",
            "releasing-admin@neonlaw.com",
            Role::Admin,
        ),
    )
    .await
    .expect("seed the releasing admin");
    store::projects::add_participation(&f.surreal, project_id, releasing_admin.id, "attorney")
        .await
        .expect("put the releasing admin on the matter");
    let (admin_cookie, admin_csrf) = cookie_for(&f.sessions, Role::Admin, Some(releasing_admin.id));
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_code}/notations/{}/transcript",
                    notation.id
                ))
                .header("cookie", &admin_cookie)
                .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
                .body(Body::from(multipart_text(
                    &admin_csrf,
                    "transcript_text",
                    "Consent given. Testator: Capricorn. Executor: Aries. Successor trustee: Gemini. \
                     Residuary beneficiary: Leo. Health-care agent: Virgo. Financial agent: Libra.",
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let (client_cookie, client_csrf) = cookie_for(&f.sessions, Role::Client, Some(client_id));

    // Gate: the client cannot approve while drafts are still at `draft`
    // (the attorney has not released them) — 404.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/app/projects/{project_code}/approve-plan"))
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
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/lawyer/notations/{}/release-drafts", notation.id))
                .header("cookie", &admin_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_csrf={admin_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let notation = store::notations::find_by_id(&f.surreal, notation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "client_review");
    let drafts = store::review_documents::for_notation(&f.surreal, notation.id)
        .await
        .unwrap();
    assert!(drafts
        .iter()
        .all(|d| d.status == store::review_documents::STATUS_PENDING_REVIEW));

    // Now the client's matter page offers the approve control.
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{project_code}"))
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
                .uri(format!("/app/projects/{project_code}/approve-plan"))
                .header("cookie", &client_cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_csrf={client_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let notation = store::notations::find_by_id(&f.surreal, notation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "sent_for_signature__pending");
    let drafts = store::review_documents::for_notation(&f.surreal, notation.id)
        .await
        .unwrap();
    assert!(
        drafts
            .iter()
            .all(|d| d.status == store::review_documents::STATUS_APPROVED),
        "every instrument is approved once the client signs off"
    );
}
