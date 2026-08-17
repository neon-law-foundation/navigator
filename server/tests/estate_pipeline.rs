#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! End-to-end Northstar estate pipeline (seams 3 + 5): from creating an
//! estate matter, uploading the sitting transcript, through extraction
//! (answers, source `extracted`) and draft rendering (one
//! `review_documents` row per instrument at `draft`) to the attorney
//! gate (`lawyer_review`).
//!
//! Uses the canonical seed so the four `northstar__*` instrument
//! templates and the estate questions exist, then drives the real HTTP
//! routes end to end.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::SESSION_COOKIE_NAME;
use portal::AppState;
use store::test_support::mem_surreal;
use tower::ServiceExt;

const BOUNDARY: &str = "navigatorestateboundary";
/// Session-cookie signing key shared by [`build_app`], the admin cookie this
/// test mints, and `portal::test_support::lawyer_bearer_header`. The bearer helper
/// signs with `portal::test_support::TEST_SESSION_KEY`, and the session boundary
/// decodes that blob with the router's own `SessionStore`, so this key must
/// match it for the CLI-credential path to authenticate.
const SESSION_KEY: &str = "test-session-key-not-for-production";

/// A session for an Admin who is on `project_id`. The matter surface scopes
/// every tier by participation now, so a bare admin session — which this
/// fixture used to build — 404s before the handler runs.
async fn admin_cookie_and_csrf(
    surreal: &store::surreal::SurrealDb,
    project_id: uuid::Uuid,
) -> (String, String) {
    let admin = store::persons::create(
        surreal,
        &store::persons::NewPerson::with_role(
            "Estate Admin",
            "estate-admin@neonlaw.com",
            store::persons::Role::Admin,
        ),
    )
    .await
    .expect("seed the acting admin");
    store::projects::add_participation(surreal, project_id, admin.id, "attorney")
        .await
        .expect("put the acting admin on the matter");
    let sessions = portal::SessionStore::new(SESSION_KEY);
    let mut session = portal::SessionData::fresh("admin@neonlaw.com", store::persons::Role::Admin);
    session.person_id = Some(admin.id);
    let csrf = session.csrf_token.clone();
    (
        format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&session)),
        csrf,
    )
}

async fn build_app() -> (axum::Router, store::surreal::SurrealDb) {
    let repo_root = std::env::temp_dir().join(format!(
        "navigator-estate-pipeline-repos-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-pipeline-test"))
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
        sessions: portal::SessionStore::new(SESSION_KEY),
        storage,
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    (
        server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
    )
}

/// A single-field multipart body with the CSRF token first — the shape
/// the upload forms render, so the handler verifies `_csrf` before it
/// reads the field (see `portal::csrf::require_multipart_csrf`).
fn multipart_text_field(csrf: &str, name: &str, value: &str) -> Vec<u8> {
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{csrf}\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn uploading_a_transcript_extracts_answers_and_renders_four_draft_instruments() {
    let (app, surreal) = build_app().await;

    // Create the estate matter (seam 1): lands on the matter page.
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
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let project_id = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .parse::<uuid::Uuid>()
        .unwrap();
    let notation = store::notations::list_by_project(&surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("estate notation");

    // Upload the sitting transcript (seam 2 handler + the pipeline).
    let transcript = "Recording consent given. Testator: Capricorn Stone. \
        Executor: Aries Vega. Successor trustee: Gemini Hart. \
        Guardian: Pisces Lake. Residuary beneficiary: Leo Sun. \
        Health-care agent: Virgo Reed. Financial agent: Libra Vale.";
    let (cookie, csrf) = admin_cookie_and_csrf(&surreal, project_id).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{}/transcript",
                    notation.id
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_text_field(
                    &csrf,
                    "transcript_text",
                    transcript,
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // The matter reached the attorney gate.
    let notation = store::notations::find_by_id(&surreal, notation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "lawyer_review");

    // Four instrument drafts, one per kind, all at `draft` (hidden from
    // the client until an attorney advances them).
    let drafts = store::review_documents::for_notation(&surreal, notation.id)
        .await
        .unwrap();
    let mut kinds: Vec<&str> = drafts.iter().map(|d| d.kind.as_str()).collect();
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        vec!["directive_financial", "directive_health", "trust", "will"]
    );
    for d in &drafts {
        assert_eq!(
            d.status,
            store::review_documents::STATUS_DRAFT,
            "every generated instrument starts hidden at draft"
        );
    }

    // The will draft rendered the extracted answers into its HTML body.
    let will = drafts.iter().find(|d| d.kind == "will").unwrap();
    assert!(
        will.body_html.contains("Capricorn Stone"),
        "{}",
        will.body_html
    );
    assert!(will.body_html.contains("Aries Vega"));
    // Placeholders were resolved, not left raw.
    assert!(!will.body_html.contains("{{"));

    // The answers were persisted as machine-extracted, not lawyer/client.
    let extracted: Vec<_> = store::answers::list_all(&surreal)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| {
            a.person_id == notation.person_id && a.source == store::answers::SOURCE_EXTRACTED
        })
        .collect();
    assert!(
        extracted.len() >= 7,
        "expected the labelled fields to be extracted, got {}",
        extracted.len()
    );

    // The client cannot see any of these drafts yet (the human gate).
    let client_visible = store::review_documents::client_visible_for_project(&surreal, project_id)
        .await
        .unwrap();
    assert!(
        client_visible.is_empty(),
        "drafts must be hidden from the client until an attorney advances them"
    );
}
