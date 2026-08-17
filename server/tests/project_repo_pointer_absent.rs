//! Integration test: a deployment with no configured forge coordinate shows no
//! repository pointer on `GET /app/projects/:id`.
//!
//! A Project's repository is a **derived coordinate that may not exist**. With
//! no deployment named there is no organization and no host, so there is no
//! coordinate — and that is a legitimate outcome rather than a degraded one. The
//! lawyer matter page must simply omit the pointer.
//!
//! This is the behaviour the configuration change makes visible. The forge host
//! used to fall back to a **public** forge, so an unset variable produced a
//! confident link into a namespace the Firm does not control, rendered on this
//! very page.
//!
//! # Why this is its own test target
//!
//! `ProjectRepositoryLink::from_env` reads process-global environment at router
//! construction, and `cargo test` runs one process per test *target* — so tests
//! sharing a binary share that state. `project_repo_clone_url.rs` needs a
//! *configured* coordinate for every one of its tests; this one needs an
//! unconfigured one. Two different process-global values need two processes, and
//! a separate target is what a separate process is.
//!
//! Putting both in one binary passes under `cargo nextest` (a process per test)
//! and fails under `cargo test` depending on which test constructs its router
//! last. CI runs the latter.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::{SessionData, SESSION_COOKIE_NAME};
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;

const KEY: &str = "test-session-key-not-for-production";

/// A host and organization are configured; the *deployment* is not one Navigator
/// recognises, which is the same absence as leaving them unset and keeps every
/// `set_var` in this target pointing one direction.
///
/// Asserting against a configured host that still yields nothing is the stronger
/// claim: it proves the deployment resolution is what withholds the pointer,
/// not a missing variable that a fallback could have filled in.
const HOST: &str = "forge.example";

#[tokio::test]
async fn an_unconfigured_deployment_has_no_repository_section() {
    std::env::set_var("NAVIGATOR_GIT_HOST", HOST);
    std::env::set_var("NAVIGATOR_GITHUB_ORG", "an-organization");
    std::env::set_var("NAVIGATOR_GCP_PROJECT_ID", "not-a-deployment");

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-repo-pointer-absent-test"))
            .await
            .unwrap(),
    );

    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let project = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: "libra-formation".into(),
            name: "Libra formation".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer Member", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    for (person_id, participation) in [(lawyer.id, "lawyer"), (client.id, "client")] {
        store::projects::add_participation(&surreal, project.id, person_id, participation)
            .await
            .unwrap();
    }

    let sessions = SessionStore::new(KEY);
    let mut lawyer_session = SessionData::fresh("lawyer-sub", Role::Lawyer);
    lawyer_session.person_id = Some(lawyer.id);
    let lawyer_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&lawyer_session));

    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}", project.id))
                .header("cookie", &lawyer_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();

    // The page renders — so the assertions below are about an absent pointer on
    // a real matter page, not a vacuous pass on an empty body.
    assert!(
        html.contains("Libra formation"),
        "the lawyer matter page must render the matter it names"
    );
    assert!(
        !html.contains("Source repository"),
        "no configured deployment means no repository pointer"
    );
    assert!(
        !html.contains(&format!("https://{HOST}/")),
        "and no coordinate on a host this deployment never resolved"
    );
}
