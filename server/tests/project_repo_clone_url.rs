#![allow(clippy::too_many_lines)]
//! Integration test: the external repository page on `GET /app/projects/:id`.
//!
//! The per-Project private repository is a lawyer-only GitHub pointer. Lawyer
//! and admin reach the matter page and see its constructed browser link; a
//! client reaches the portal view and must **never** see that internal link.

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
    /// A lawyer disclosed to the matter — sees the admin page.
    lawyer_cookie: String,
    /// The matter's client — reaches the portal view, never the git URL.
    client_cookie: String,
}

/// The forge coordinate every fixture in this binary configures, so the lawyer
/// assertion and the client-absence assertion are both made against a
/// *configured* pointer — a client seeing no link proves the lens, not a
/// missing environment. `lawyer_project_detail_router` reads these once at
/// construction, and every test here wants the same values, so they are set
/// before the router is built and never unset.
///
/// Synthetic on purpose: the organization and host are configuration, so this
/// file spells neither a real organization nor a real forge host.
///
/// **Every test in this binary needs the same values, and that is load-bearing
/// rather than incidental.** `cargo test` runs one process per test *target*, so
/// these `set_var` calls are shared process state across the tests below; a test
/// wanting a *different* deployment would race the others depending on
/// scheduling. The absent-pointer case therefore lives in its own target,
/// `project_repo_pointer_absent.rs`, which is its own process.
const ORG: &str = "an-organization";
const HOST: &str = "forge.example";

async fn build_fixture() -> Fixture {
    std::env::set_var("NAVIGATOR_GIT_HOST", HOST);
    std::env::set_var("NAVIGATOR_GITHUB_ORG", ORG);
    std::env::set_var("NAVIGATOR_GCP_PROJECT_ID", "neon-law-stg");
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-project-clone-url-test"))
            .await
            .unwrap(),
    );

    let tmpl = store::templates::save_version(
        &surreal,
        None,
        "onboarding__retainer",
        store::templates::Version {
            title: "Retainer".into(),
            respondent_type: "person_and_entity".into(),
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
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let proj = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("libra-formation-{}", Uuid::now_v7()),
            name: "Libra formation".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(tmpl.id, client.id, proj.id, "BEGIN"),
    )
    .await
    .unwrap();

    // Lawyer disclosed to the matter (a person_project_roles row).
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer Member", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    for (pid, participation) in [(lawyer.id, "lawyer"), (client.id, "client")] {
        store::projects::add_participation(&surreal, proj.id, pid, participation)
            .await
            .unwrap();
    }

    let sessions = SessionStore::new(KEY);
    let mut lawyer_session = SessionData::fresh("lawyer-sub", Role::Lawyer);
    lawyer_session.person_id = Some(lawyer.id);
    let lawyer_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&lawyer_session));
    let mut client_session = SessionData::fresh("client-sub", Role::Client);
    client_session.person_id = Some(client.id);
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&client_session));

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
        lawyer_cookie,
        client_cookie,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn lawyer_sees_the_external_repository_page() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}", f.project_id))
                .header("cookie", &f.lawyer_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(
        html.contains("Integrations"),
        "lawyer page must have the integrations section"
    );
    // The whole coordinate, and nothing appended to it: the repository name *is*
    // the Project code, so a trailing segment here would mean something is
    // still composing a second identifier into the name.
    let expected = format!("https://{HOST}/{ORG}/{}", f.project_code);
    assert!(
        html.contains(&expected),
        "a lawyer must see the one configured repository coordinate"
    );
    assert!(
        html.contains(&format!("{expected}\"")),
        "the coordinate must end at the Project code — nothing composes a suffix"
    );
    assert!(html.contains("Source repository"));
    assert!(!html.contains("navigator git"));
}

#[tokio::test]
async fn git_token_route_is_not_registered() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/app/projects/{}/git-token", f.project_id))
                .header("cookie", &f.lawyer_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn client_never_sees_the_repository_page() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{}", f.project_id))
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // The client reaches their *rendered* portal view of the matter — a 200,
    // not a redirect or 404. Assert that first, and that the page actually
    // renders the matter it names, so the `.git`-absence check below is proven
    // against a real portal page and can't pass vacuously on an empty body.
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "client must reach their rendered portal view of the matter"
    );
    let html = body_string(resp).await;
    assert!(
        html.contains("Libra formation"),
        "the client portal view must render the matter it names"
    );
    // The pointer is configured for this fixture, so its absence here is the
    // client lens withholding it, not an unconfigured deployment.
    assert!(
        !html.contains(&format!("https://{HOST}/{ORG}/")),
        "the client portal view must never expose the repository page"
    );
    assert!(
        !html.contains("Source repository"),
        "the client portal view must carry no source-control vocabulary"
    );
}
