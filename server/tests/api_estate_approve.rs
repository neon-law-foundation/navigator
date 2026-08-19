#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration tests for `POST /app/api/projects/{id}/approve-plan` — the
//! client-writable REST door that approves a released estate plan.
//!
//! The write engine (`estate::approve_estate_plan`) is shared with the client
//! approve form, so this focuses on what the REST adapter adds: it is
//! client-writable (not lawyer-only), the client-lens gate collapses a
//! non-client and a matter with nothing to approve to a non-disclosing 404, an
//! anonymous caller is 401, and the client's live approval advances the plan.
//!
//! The pipeline to `client_review` mirrors `estate_review_gates.rs`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::{SessionData, SESSION_COOKIE_NAME};
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "test-session-key-not-for-production";
const BOUNDARY: &str = "navigatorapproveboundary";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    notation_id: Uuid,
    client_id: Uuid,
}

fn bearer(person_id: Uuid, role: Role) -> String {
    let mut s = SessionData::fresh("api-approve-sub", role);
    s.person_id = Some(person_id);
    format!("Bearer {}", SessionStore::new(KEY).encode(&s))
}

fn cookie_for(role: Role, person_id: Uuid) -> (String, String) {
    let mut s = SessionData::fresh("sub", role);
    s.person_id = Some(person_id);
    let csrf = s.csrf_token.clone();
    (
        format!(
            "{SESSION_COOKIE_NAME}={}",
            SessionStore::new(KEY).encode(&s)
        ),
        csrf,
    )
}

fn multipart_text(csrf: &str, name: &str, value: &str) -> Vec<u8> {
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{csrf}\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

/// Drive a fresh estate matter all the way to `client_review` with every draft
/// released to `pending_review`, exactly as the lawyer surface does.
async fn build_at_client_review() -> Fixture {
    let repo_root =
        std::env::temp_dir().join(format!("navigator-api-approve-repos-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-api-approve-storage"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&surreal, &storage)
        .await
        .unwrap();
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
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));

    // Create the estate matter and read back its notation + client.
    let resp = app
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
    let project_id = store::projects::find_by_code(&surreal, &project_code)
        .await
        .unwrap()
        .expect("redirected project exists")
        .id;
    let notation = store::notations::list_by_project(&surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let client_id = notation.person_id;

    // An admin on the matter uploads the transcript (→ lawyer_review) and
    // releases the drafts (→ client_review, drafts pending_review).
    let admin = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role(
            "Releasing Admin",
            "releasing-admin@neonlaw.com",
            Role::Admin,
        ),
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project_id, admin.id, "attorney")
        .await
        .unwrap();
    let (admin_cookie, admin_csrf) = cookie_for(Role::Admin, admin.id);

    let resp = app
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

    let resp = app
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
    let notation = store::notations::find_by_id(&surreal, notation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "client_review");

    Fixture {
        app,
        surreal,
        project_id,
        notation_id: notation.id,
        client_id,
    }
}

async fn approve(
    app: &axum::Router,
    project_id: Uuid,
    auth: Option<&str>,
) -> axum::http::Response<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/app/api/projects/{project_id}/approve-plan"));
    if let Some(auth) = auth {
        req = req.header("authorization", auth);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn the_client_approves_the_released_plan_through_the_api() {
    let fx = build_at_client_review().await;
    let client = bearer(fx.client_id, Role::Client);

    let resp = approve(&fx.app, fx.project_id, Some(&client)).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let notation = store::notations::find_by_id(&fx.surreal, fx.notation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        notation.state, "sent_for_signature__pending",
        "the client's approval advances the plan"
    );
    let drafts = store::review_documents::for_notation(&fx.surreal, fx.notation_id)
        .await
        .unwrap();
    assert!(
        drafts
            .iter()
            .all(|d| d.status == store::review_documents::STATUS_APPROVED),
        "every instrument is approved once the client signs off"
    );
}

#[tokio::test]
async fn an_anonymous_caller_is_unauthenticated() {
    let fx = build_at_client_review().await;
    let resp = approve(&fx.app, fx.project_id, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_non_client_caller_gets_a_non_disclosing_404() {
    let fx = build_at_client_review().await;
    // A lawyer who is not this matter's client: reaches the door (client-writable
    // at the policy layer) but the client-lens gate denies with a bare 404.
    let stranger = store::persons::create(
        &fx.surreal,
        &store::persons::NewPerson::with_role("Stranger", "stranger@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    let resp = approve(
        &fx.app,
        fx.project_id,
        Some(&bearer(stranger.id, Role::Lawyer)),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // The plan is untouched by the refused approval.
    let notation = store::notations::find_by_id(&fx.surreal, fx.notation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "client_review");
}
