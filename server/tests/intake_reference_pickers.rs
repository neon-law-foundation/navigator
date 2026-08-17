#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration tests for the DB-backed record/reference pickers on the
//! **client** self-serve intake surface
//! (`/app/projects/:id/intake/:notation_id`) — the demand-side mirror of
//! the lawyer walk covered by `step_candidates.rs`.
//!
//! The retainer cucumber (`features/client_intake.rs`) drives the happy path
//! over free-typed person/project questions; it never reaches a
//! record/reference question, so the reference-resolution branches the PR 1
//! of #349 adds to `intake_save` are exercised here instead. Drives the real
//! route with a signed client session and asserts, against a real
//! questionnaire whose questions default to the client-facing `both`
//! audience:
//!   1. A `country` step renders the seeded picker options in the form.
//!   2. Posting a chosen row's `id` stores the `{"value":name,"name":name,
//!      "id":uuid}` envelope and advances.
//!   3. The browser `<select>` name path resolves the same seeded row and
//!      stores its id.
//!   4. An off-list `country` value is rejected — the form re-renders with
//!      the picker error and nothing persists.
//!   5. A record type (`entity`) still free-types a new row with no id.
//!   6. A frozen (past-intake) notation shows the completion landing on GET
//!      and bounces a POST, writing nothing.
//!   7. A request with no session is bounced to the login door by the shared
//!      session boundary (a uniform 303 that leaks no matter existence), and
//!      a signed non-participant past that boundary 404s from the row-scope
//!      check.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::{SessionData, SESSION_COOKIE_NAME};
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::seed;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use workflows::InMemoryRuntime;

const KEY: &str = "test-session-key-not-for-production";

/// A questionnaire whose client-facing steps are a global reference pick
/// (`country`, which must match a seeded row), a project-scoped record pick
/// (`entity`, which may free-type a new row), and a trailing free-text
/// note. Every step defaults to the `both` audience, so the client walks
/// all three; answering them reaches the completion landing.
const QUESTIONNAIRE: &[u8] = br"---
questionnaire:
  BEGIN:
    _: country__of_birth
  country__of_birth:
    _: entity__company
  entity__company:
    _: custom_text__note
  custom_text__note:
    _: END
  END: {}
---

# Client candidate walk
";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: uuid::Uuid,
    notation_id: uuid::Uuid,
    /// The signed client cookie for the matter's participant.
    client_cookie: String,
    /// That session's CSRF token — required on every form POST.
    client_csrf: String,
}

/// Build the app with a matter whose client (`Libra`) is a participant, a
/// notation on a template carrying [`QUESTIONNAIRE`], and a signed client
/// session for `Libra`.
async fn build() -> Fixture {
    let repo_root =
        std::env::temp_dir().join(format!("navigator-intake-pickers-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!(
            "navigator-intake-pickers-store-{}",
            uuid::Uuid::now_v7()
        )))
        .await
        .unwrap(),
    );
    // Seeds the canonical templates, the question registry rows (all
    // `both`-audience by default, so client-facing), and the seeded
    // jurisdictions (countries) the picker lists.
    seed::seed_canonical(&surreal, &storage).await.unwrap();

    let blob = store::assets::ingest_content(&surreal, &storage, QUESTIONNAIRE, "text/markdown")
        .await
        .unwrap();
    let template = store::templates::save_version(
        &surreal,
        None,
        "test__client_candidate_walk",
        store::templates::Version {
            title: "Client candidate walk".into(),
            respondent_type: "person".into(),
            asset_id: Some(blob),
            form_code: None,
            kind: None,
            source_commit_sha: None,
        },
    )
    .await
    .unwrap()
    .into_model();

    let libra = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let entity_id = store::test_support::seed_entity(&surreal).await;
    let project = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("libra-candidate-{}", uuid::Uuid::now_v7()),
            name: "Libra candidate matter".into(),
            status: "open".into(),
            entity_id,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    store::projects::add_participation(&surreal, project.id, libra.id, "client")
        .await
        .unwrap();
    let notation_id = store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(template.id, libra.id, project.id, "BEGIN"),
    )
    .await
    .unwrap()
    .id;

    let mut session = SessionData::fresh("libra-sub", Role::Client);
    session.person_id = Some(libra.id);
    let client_csrf = session.csrf_token.clone();
    let sessions = SessionStore::new(KEY);
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&session));

    let runtime = Arc::new(InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage,
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    Fixture {
        app: server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
        project_id: project.id,
        notation_id,
        client_cookie,
        client_csrf,
    }
}

impl Fixture {
    fn path(&self) -> String {
        format!(
            "/app/projects/{}/intake/{}",
            self.project_id, self.notation_id
        )
    }

    /// GET the intake page as the signed-in client; return `(status, body)`.
    async fn get(&self) -> (StatusCode, String) {
        let resp = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(self.path())
                    .header("cookie", &self.client_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    /// POST one answer body (CSRF token prepended) as the signed-in client.
    async fn post(&self, form: &str) -> StatusCode {
        self.post_response(form).await.status()
    }

    /// As [`Fixture::post`], but keeping the whole response so a caller can read
    /// the redirect target — a refused save carries its reason in the `?error=`
    /// of that `Location`.
    async fn post_response(&self, form: &str) -> axum::http::Response<Body> {
        let body = format!("_csrf={}&{form}", self.client_csrf);
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(self.path())
                    .header("cookie", &self.client_cookie)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// GET an arbitrary path as the signed-in client; return `(status, body)`.
    async fn get_path(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", &self.client_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }
}

/// The seeded `Mexico` country row's id — what a picker selection posts.
/// Jurisdictions live in SurrealDB since ENG-20.
async fn mexico_id(surreal: &store::surreal::SurrealDb) -> uuid::Uuid {
    store::jurisdictions::find_by_name(surreal, "Mexico")
        .await
        .unwrap()
        .expect("Mexico is a seeded country")
        .id
}

/// The latest stored answer envelope for a walked state, if any.
async fn latest_answer(
    surreal: &store::surreal::SurrealDb,
    nid: uuid::Uuid,
    state_name: &str,
) -> Option<serde_json::Value> {
    // Append-only: the last row for this state is the latest answer.
    store::answers::for_notation(surreal, nid)
        .await
        .unwrap()
        .into_iter()
        .rfind(|a| a.state_name.as_deref() == Some(state_name))
        .map(|a| a.value)
}

#[tokio::test]
async fn the_country_step_renders_the_seeded_picker_options() {
    let f = build().await;
    let (status, body) = f.get().await;
    assert_eq!(status, StatusCode::OK);
    // The country `<select>` lists every seeded country, so the form body
    // carries a seeded option name.
    assert!(
        body.contains("<select"),
        "the country step renders a select: {body}"
    );
    assert!(
        body.contains("Mexico"),
        "the seeded country options are rendered: {body}"
    );
}

#[tokio::test]
async fn posting_a_country_id_stores_the_row_id_and_advances() {
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    assert_eq!(f.post(&format!("id={mx}")).await, StatusCode::SEE_OTHER);

    let value = latest_answer(&f.surreal, f.notation_id, "country__of_birth")
        .await
        .expect("country answer persisted");
    assert_eq!(value["value"], "Mexico");
    assert_eq!(value["name"], "Mexico");
    assert_eq!(
        value["id"],
        mx.to_string(),
        "the picked row id lands in the client's envelope"
    );
}

#[tokio::test]
async fn the_select_name_path_resolves_and_stores_the_id() {
    // The browser `<select>` posts the display name as `value` (no `id`);
    // the client write path still resolves the seeded row and stores its id.
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    assert_eq!(f.post("value=Mexico").await, StatusCode::SEE_OTHER);

    let value = latest_answer(&f.surreal, f.notation_id, "country__of_birth")
        .await
        .expect("country answer persisted");
    assert_eq!(value["name"], "Mexico");
    assert_eq!(
        value["id"],
        mx.to_string(),
        "the name path resolves to the same seeded row id"
    );
}

#[tokio::test]
async fn an_off_list_country_is_rejected_and_re_renders_with_the_picker_error() {
    let f = build().await;
    // A value naming no seeded country does not advance: the save redirects
    // back to the same step carrying the picker error as an `?error=` flash
    // (the page renders through Dioxus, so it cannot re-render inline), and
    // writes nothing.
    let resp = f.post_response("value=Atlantis").await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        loc.starts_with(&format!("{}?error=", f.path())),
        "a refused save goes back to the same step with its reason: {loc}"
    );

    // Following it puts the client back on the country step with the reason
    // visible — not a silent bounce that looks like the answer was taken.
    let (status, body) = f.get_path(&loc).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Mexico") && body.contains("<select"),
        "still on the country step: {body}"
    );
    assert!(body.contains("nav-form-error"), "{body}");
    assert!(
        latest_answer(&f.surreal, f.notation_id, "country__of_birth")
            .await
            .is_none(),
        "an off-list reference value must not persist"
    );
}

#[tokio::test]
async fn a_record_type_still_free_types_a_new_row_without_an_id() {
    // Answer the country pick, then free-type a brand-new entity name the
    // picker doesn't list — a record type keeps the create-a-new-row path,
    // storing a value with no resolved id.
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    assert_eq!(f.post(&format!("id={mx}")).await, StatusCode::SEE_OTHER);

    assert_eq!(
        f.post("value=Bright Star Ventures").await,
        StatusCode::SEE_OTHER
    );
    let value = latest_answer(&f.surreal, f.notation_id, "entity__company")
        .await
        .expect("free-typed entity persisted");
    assert_eq!(value["value"], "Bright Star Ventures");
    assert_eq!(value["name"], "Bright Star Ventures");
    assert!(
        value.get("id").is_none(),
        "a free-typed record answer carries no resolved id: {value}"
    );
}

#[tokio::test]
async fn answering_every_client_question_reaches_the_completion_landing() {
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    f.post(&format!("id={mx}")).await;
    f.post("value=Bright Star Ventures").await;
    f.post("value=Please expedite.").await;
    let (status, body) = f.get().await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("your part is done"),
        "the completion landing renders once every client question is answered: {body}"
    );
}

#[tokio::test]
async fn a_frozen_intake_shows_the_landing_and_bounces_a_write() {
    // Once the document has gone out for signature the client's answers are
    // frozen: GET shows the completion landing, POST bounces back writing
    // nothing.
    let f = build().await;
    store::notations::update_state(&f.surreal, f.notation_id, "sent_for_signature")
        .await
        .unwrap();

    let (status, body) = f.get().await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("your part is done"),
        "a frozen intake shows the completion landing: {body}"
    );

    let mx = mexico_id(&f.surreal).await;
    assert_eq!(
        f.post(&format!("id={mx}")).await,
        StatusCode::SEE_OTHER,
        "a write to a frozen intake bounces back"
    );
    assert!(
        latest_answer(&f.surreal, f.notation_id, "country__of_birth")
            .await
            .is_none(),
        "a frozen intake must not accept a new answer"
    );
}

#[tokio::test]
async fn a_notation_from_another_project_is_not_found() {
    // The path pins both the project and the notation; a notation that
    // belongs to a different project than the URL names is a 404, so a
    // guessed id can't be walked under an unrelated project.
    let f = build().await;
    let stray_project = uuid::Uuid::now_v7();
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/app/projects/{stray_project}/intake/{}",
                    f.notation_id
                ))
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_write_after_completion_bounces_without_persisting() {
    // Once every client question is answered the step is complete; a
    // further POST has nothing to save and bounces back to the landing.
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    f.post(&format!("id={mx}")).await;
    f.post("value=Bright Star Ventures").await;
    f.post("value=Please expedite.").await;

    assert_eq!(
        f.post(&format!("id={mx}")).await,
        StatusCode::SEE_OTHER,
        "a post to a completed intake bounces back"
    );
    // The country answer is untouched by the extra post — still one row,
    // the original pick.
    let value = latest_answer(&f.surreal, f.notation_id, "country__of_birth")
        .await
        .expect("country answer persisted");
    assert_eq!(value["id"], mx.to_string());
}

#[tokio::test]
async fn an_unauthenticated_write_is_not_found() {
    // A form POST with no session cookie never reaches the handler: the
    // shared session boundary bounces the anonymous browser to the login
    // door with a 303, so nothing is attributed or written.
    let f = build().await;
    let mx = mexico_id(&f.surreal).await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(f.path())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("id={mx}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/auth/login?return_to={}", f.path()).as_str()),
        "the boundary carries the reader back after login"
    );
    assert!(
        latest_answer(&f.surreal, f.notation_id, "country__of_birth")
            .await
            .is_none(),
        "an unauthenticated write must not persist"
    );
}

#[tokio::test]
async fn an_unauthenticated_request_is_bounced_to_the_login_door() {
    // No session cookie → the shared session boundary redirects the browser
    // to login with a 303 before the handler runs. The redirect is uniform
    // across the whole surface, so it leaks nothing about whether the matter
    // exists (an existing and a bogus matter answer identically).
    let f = build().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(f.path())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some(format!("/auth/login?return_to={}", f.path()).as_str()),
        "the boundary carries the reader back after login"
    );
}

#[tokio::test]
async fn a_non_participant_is_not_found() {
    // A signed-in client with no participation row for this matter gets a
    // 404 from the row-scope check, never the intake form.
    let f = build().await;
    let stranger = store::persons::create(
        &f.surreal,
        &store::persons::NewPerson::with_role("Aries", "aries@example.com", Role::Client),
    )
    .await
    .unwrap();
    let mut session = SessionData::fresh("aries-sub", Role::Client);
    session.person_id = Some(stranger.id);
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}",
        SessionStore::new(KEY).encode(&session)
    );
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(f.path())
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
