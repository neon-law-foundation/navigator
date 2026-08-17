#![allow(clippy::doc_markdown)]
//! Integration test for Northstar estate-matter creation (seam 1).
//!
//! `POST /lawyer/retainers/new` with the `onboarding__estate`
//! template must reuse the retainer's creation plumbing (Person +
//! Project + role + Notation) but, because the estate flow is
//! transcript-driven and has no questionnaire to walk before intake,
//! it must instead **start the workflow machine at BEGIN** and land
//! lawyer on the matter page (`/app/projects/:id`) where the
//! transcript-upload form lives — not on the questionnaire walker.
//!
//! This proves the created matter is a live timeline the shipped
//! transcript handler can signal: after creation we fire
//! `transcript_uploaded` through the same runtime and assert it
//! advances onto `document_intake__transcript`.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::AppState;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;
use workflows::{MachineKind, StateMachineRuntime};

/// Session-cookie signing key shared by [`build_app`] and the tests that
/// mint a logged-in lawyer cookie against it.
const SESSION_KEY: &str = "test-session-key-not-for-production";

/// `NAVIGATOR_GIT_REPO_ROOT` is process-global, so this test binary uses one
/// stable repo root instead of per-test roots that can race under tokio.
fn repo_root() -> &'static PathBuf {
    static REPO_ROOT: OnceLock<PathBuf> = OnceLock::new();
    REPO_ROOT.get_or_init(|| {
        let repo_root = std::env::temp_dir().join(format!(
            "navigator-estate-creation-repos-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&repo_root).unwrap();
        std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);
        repo_root
    })
}

/// Build the app with the `onboarding__estate` template seeded (no
/// notation yet — creation is what the test exercises). Returns the
/// router, the db, and the shared workflow runtime so the test can
/// signal the freshly-started machine.
async fn build_app() -> (
    axum::Router,
    store::surreal::SurrealDb,
    Arc<dyn StateMachineRuntime>,
) {
    let _repo_root = repo_root();
    let surreal = mem_surreal().await;
    // Every matter now carries a NOT NULL lawyer DRI; the self-serve walk
    // resolves it to the firm principal (by role) when no lawyer is in the
    // room. Seed one so the walk can open the matter.
    store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role(
            "Firm Principal",
            "principal@example.com",
            store::persons::Role::Admin,
        ),
    )
    .await
    .expect("seed firm principal");
    let _ = store::templates::save_version(
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
    .expect("seed estate template")
    .into_model();

    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-creation-test"))
            .await
            .unwrap(),
    );
    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_store(surreal.clone()),
    );

    let state = AppState {
        storage,
        workflow_runtime: workflow_runtime.clone(),
        questionnaire_runtime: inner,
        email,
        sessions: portal::SessionStore::new(SESSION_KEY),
        ..portal::test_support::app_state(surreal.clone()).await
    };
    (
        server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        workflow_runtime,
    )
}

#[tokio::test]
async fn creating_an_estate_matter_starts_the_workflow_and_lands_on_the_matter_page() {
    let (app, surreal, runtime) = build_app().await;

    let resp = app
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

    // The estate flow skips the questionnaire walker: it lands lawyer on
    // the matter page, where the transcript-upload form lives — not on
    // `/lawyer/notations/:id/step`.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.starts_with("/app/projects/"),
        "estate creation should land on the matter page, got {location}"
    );
    assert!(
        !location.contains("/notations/"),
        "estate creation must not redirect to the questionnaire walker, got {location}"
    );

    // The four lifecycle rows exist: Person, Project, role, and the
    // estate Notation parked at BEGIN.
    let template = store::templates::resolve(&surreal, None, "onboarding__estate")
        .await
        .unwrap()
        .expect("estate template");
    let project_id: Uuid = location
        .strip_prefix("/app/projects/")
        .expect("estate creation lands on the matter page")
        .parse()
        .expect("redirect carries the project id");
    let notation = store::notations::list_by_project(&surreal, project_id)
        .await
        .unwrap()
        .into_iter()
        .find(|n| n.template_id == template.id)
        .expect("estate notation created");
    assert_eq!(notation.state, "BEGIN");
    assert_eq!(location, format!("/app/projects/{}", notation.project_id));

    let person = store::persons::find_by_id(&surreal, notation.person_id)
        .await
        .unwrap()
        .expect("client person created");
    assert_eq!(person.email, "capricorn@example.com");

    // The workflow machine was actually started — not just the row set
    // to BEGIN. Firing the transcript signal advances it, which would
    // error with "machine not started" had creation only written the row.
    let next = StateMachineRuntime::signal(
        runtime.as_ref(),
        MachineKind::Workflow,
        notation.id,
        "transcript_uploaded",
        Some(
            &serde_json::to_string(&workflows::IntakePayload {
                kind: "transcript".into(),
                filename: "sitting-transcript.txt".into(),
                artifact: workflows::IntakeArtifact::Text {
                    text: "Consent recorded.".into(),
                },
            })
            .unwrap(),
        ),
    )
    .await
    .expect("estate workflow was started at creation and accepts the transcript signal");
    assert_eq!(next.as_str(), "document_intake__transcript");
}

/// The matter page (`GET /app/projects/:id`) is project-scoped:
/// `can_see_project` 404s a lawyer with no `person_project_roles`
/// row on the matter. Estate creation redirects the opener *straight to*
/// that page, so unless creation discloses the opener as the matter's
/// lawyer DRI they land on a "Not found" — the exact gap the browser e2e
/// `lawyer_opens_an_estate_matter_and_sees_the_transcript_form` caught.
/// This pins the fix: a logged-in lawyer who opens an estate matter is
/// disclosed to it and can load the transcript-upload page.
#[tokio::test]
async fn creating_lawyer_is_disclosed_to_the_estate_matter_they_open() {
    use http_body_util::BodyExt;
    use portal::session::{SessionData, SESSION_COOKIE_NAME};
    use store::persons::Role;

    let (app, surreal, _runtime) = build_app().await;

    // A real logged-in lawyer (has a linked Person, unlike the `Bearer
    // dev` bypass which carries no `person_id` and so cannot be disclosed).
    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Opening Lawyer", "opener@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    let mut session = SessionData::fresh("opener-sub", Role::Lawyer);
    session.person_id = Some(lawyer.id);
    // Cookie-session POSTs to admin forms are CSRF-checked: the body must
    // echo the session's token (the `Bearer dev` path in the test above is
    // exempt, so it needs none).
    let csrf = session.csrf_token.clone();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}",
        portal::SessionStore::new(SESSION_KEY).encode(&session)
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/lawyer/retainers/new")
                .header("cookie", &cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "client_email=aries%40example.com&retainer_template_code=onboarding__estate&_csrf={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let project_id: Uuid = location
        .strip_prefix("/app/projects/")
        .expect("estate creation lands on the matter page")
        .parse()
        .expect("redirect carries the project id");

    // The opener is disclosed to the new matter as its lawyer DRI.
    let dri = store::projects::participation_for_person(&surreal, lawyer.id, project_id)
        .await
        .unwrap()
        .expect("opening lawyer is disclosed to the matter they created");
    assert!(
        dri.is_lawyer_dri,
        "the opening lawyer is the matter's accountable lawyer"
    );

    // …and therefore can actually load the matter page (not a 404) and see
    // the transcript-upload form, end to end.
    let page = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&location)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let html = String::from_utf8(
        page.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        html.contains("File the sitting transcript"),
        "the opener should see the transcript-upload form on the matter page"
    );
}
