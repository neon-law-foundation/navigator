#![allow(clippy::doc_markdown)]
//! Integration test for the Northstar transcript-upload surface.
//!
//! `POST /app/projects/:id/notations/:nid/transcript` files a
//! sitting transcript into an estate matter by threading a
//! `workflows::IntakePayload` through the workflow's `transcript_uploaded`
//! signal. The router is wired with a `DispatchingRuntime` (the same
//! in-process path the dev binary and feature suite use), so the
//! document-intake step actually runs and the transcript lands as a
//! `documents` row — proving the surface drives the reusable step
//! end-to-end, not just that it returns a redirect.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::session::SESSION_COOKIE_NAME;
use portal::AppState;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use workflows::{MachineKind, StateMachineRuntime};

const BOUNDARY: &str = "navigatortestboundary";
const SESSION_KEY: &str = "transcript-intake-test-session-key";

/// A session for an Admin who is actually on the matter.
///
/// Since ENG-81 the matter surface requires a firm-side `person_project_roles`
/// row of every tier, so a bare Admin session — which this fixture used to
/// build with no linked person at all — now 404s before the handler's CSRF
/// check. These tests are about the CSRF and intake contracts, not the gate;
/// the gate is pinned in `store::access` and `server/tests/routes.rs`.
fn admin_cookie_and_csrf(person_id: uuid::Uuid) -> (String, String) {
    let sessions = portal::SessionStore::new(SESSION_KEY);
    let mut session = portal::SessionData::fresh("admin@neonlaw.com", store::persons::Role::Admin);
    session.person_id = Some(person_id);
    let csrf = session.csrf_token.clone();
    (
        format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&session)),
        csrf,
    )
}

/// Build the app with an estate notation whose workflow is started and
/// parked at BEGIN, ready for the transcript upload. Returns the router,
/// the db, the project id, and the notation id.
async fn build_app() -> (
    axum::Router,
    store::surreal::SurrealDb,
    uuid::Uuid,
    uuid::Uuid,
    uuid::Uuid,
) {
    let surreal = mem_surreal().await;
    let notation_id = store::test_support::seed_notation(&surreal).await;
    let project_id = store::notations::find_by_id(&surreal, notation_id)
        .await
        .unwrap()
        .expect("seeded notation")
        .project_id;

    let admin = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role(
            "Transcript Admin",
            "admin@neonlaw.com",
            store::persons::Role::Admin,
        ),
    )
    .await
    .expect("seed the acting admin");
    store::projects::add_participation(&surreal, project_id, admin.id, "attorney")
        .await
        .expect("put the acting admin on the matter");

    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-transcript-intake-test"))
            .await
            .unwrap(),
    );

    // The estate workflow must be started so the `transcript_uploaded`
    // signal from BEGIN is valid. Wrap the in-memory runtime in
    // DispatchingRuntime+with_db so the document-intake dispatch files
    // the transcript for real.
    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_store(surreal.clone()),
    );
    let yaml = workflows::bundled_spec_yaml("onboarding__estate").expect("estate spec bundled");
    let spec = workflows::workflow_spec_from_yaml(yaml).expect("estate spec parses");
    StateMachineRuntime::start(
        workflow_runtime.as_ref(),
        MachineKind::Workflow,
        notation_id,
        &spec,
    )
    .await
    .expect("start estate workflow");

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
        project_id,
        notation_id,
        admin.id,
    )
}

/// A `multipart/form-data` body carrying the CSRF token as its first
/// field — the way the upload forms render `_csrf` — followed by `fields`.
/// The upload handler reads `_csrf` first (see
/// `portal::csrf::require_multipart_csrf`), so a cookie-authenticated body
/// that omits it, or carries it out of first position, is rejected.
fn multipart_body(csrf: &str, fields: &[(&str, &str)]) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut parts: Vec<(&str, &str)> = vec![("_csrf", csrf)];
    parts.extend_from_slice(fields);
    let mut body = String::new();
    for (name, value) in parts {
        let _ = write!(
            body,
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        );
    }
    let _ = write!(body, "--{BOUNDARY}--\r\n");
    body.into_bytes()
}

#[tokio::test]
async fn transcript_text_upload_files_a_document_and_advances_state() {
    let (app, surreal, project_id, notation_id, admin_id) = build_app().await;

    let transcript = "Consent recorded. Executor: Aries. Successor trustee: Capricorn.";
    let (cookie, csrf) = admin_cookie_and_csrf(admin_id);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_body(
                    &csrf,
                    &[("transcript_text", transcript)],
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Redirect back to the matter.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        format!("/app/projects/{project_id}")
    );

    // The transcript filed as a document `assets` row on the matter's project.
    let doc = store::assets::for_project(&surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.kind.as_deref() == Some("transcript"))
        .expect("a transcript document filed on the project");
    assert_eq!(doc.source.as_deref(), Some("upload"));
    assert_eq!(doc.content_type, "text/plain");

    // The handler files the transcript and then drives the estate
    // pipeline (extract → drafts → lawyer_review). This fixture seeds only
    // the estate template (no questions, no instrument templates), so the
    // pipeline persists no answers and renders no drafts, but it still
    // advances the durable machine to the attorney gate — proving the
    // continuation runs and syncs state.
    let notation = store::notations::find_by_id(&surreal, notation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "lawyer_review");
}

#[tokio::test]
async fn transcript_upload_for_wrong_project_is_not_found() {
    let (app, _surreal, _project_id, notation_id, admin_id) = build_app().await;
    // A different (random) project id in the URL must 404 — the
    // cross-resource guard rejects tunnelling the notation through
    // another project's URL.
    let other_project = uuid::Uuid::now_v7();
    let (cookie, csrf) = admin_cookie_and_csrf(admin_id);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{other_project}/notations/{notation_id}/transcript"
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_body(
                    &csrf,
                    &[("transcript_text", "x")],
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn transcript_upload_without_csrf_is_forbidden() {
    let (app, _surreal, project_id, notation_id, admin_id) = build_app().await;
    let (cookie, _csrf) = admin_cookie_and_csrf(admin_id);
    // A cookie-authenticated multipart upload that omits `_csrf` is a
    // forged-request shape — the handler rejects it before touching the
    // file, exactly as `require_csrf` rejects a tokenless classic form
    // POST. The body carries only the transcript field, no `_csrf`.
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"transcript_text\"\r\n\r\nx\r\n--{BOUNDARY}--\r\n"
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn transcript_upload_with_mismatched_csrf_is_forbidden() {
    let (app, _surreal, project_id, notation_id, admin_id) = build_app().await;
    let (cookie, _csrf) = admin_cookie_and_csrf(admin_id);
    // The `_csrf` field is present and first, but its value is not the
    // session token — a stale or forged token — so it is rejected.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_body(
                    "not-the-session-token",
                    &[("transcript_text", "x")],
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn transcript_upload_with_csrf_not_first_is_forbidden() {
    let (app, _surreal, project_id, notation_id, admin_id) = build_app().await;
    let (cookie, csrf) = admin_cookie_and_csrf(admin_id);
    // `_csrf` is present and carries the correct value, but it is the
    // second field, not the first. The handler reads only the first field
    // and requires it to be `_csrf` (so a forged upload is rejected before
    // its file is read), so an out-of-order token is refused too.
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"transcript_text\"\r\n\r\nx\r\n\
         --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{csrf}\r\n--{BOUNDARY}--\r\n"
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("cookie", cookie)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bearer_transcript_upload_without_csrf_is_exempt() {
    let (app, surreal, project_id, notation_id, admin_id) = build_app().await;
    // A bearer caller (the `navigator` CLI / MCP / A2A) carries no session
    // cookie, so there is nothing a browser auto-attaches cross-site and
    // no CSRF to forge. `require_multipart_csrf` skips the check on that
    // credential, matching `require_csrf`'s passthrough, so the upload
    // succeeds even though the body carries no `_csrf` field.
    let _ = notation_id;
    let sessions = portal::SessionStore::new(SESSION_KEY);
    let bearer = sessions.encode(&{
        // The bearer credential carries the same SessionData a cookie does, so
        // it needs the linked person too: the matter surface scopes every tier
        // by participation now, bearer callers included.
        let mut session =
            portal::SessionData::fresh("admin@neonlaw.com", store::persons::Role::Admin);
        session.person_id = Some(admin_id);
        session
    });
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"transcript_text\"\r\n\r\n\
         Consent recorded. Executor: Aries. Successor trustee: Capricorn.\r\n--{BOUNDARY}--\r\n"
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/app/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("authorization", format!("Bearer {bearer}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // Not 403: the check is skipped, and the transcript files for real.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let filed = store::assets::for_project(&surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .any(|d| d.project_id == Some(project_id) && d.kind.as_deref() == Some("transcript"));
    assert!(filed, "the bearer upload filed a transcript document");
}
