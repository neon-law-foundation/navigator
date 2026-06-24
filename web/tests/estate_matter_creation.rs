#![allow(clippy::doc_markdown)]
//! Integration test for Northstar estate-matter creation (seam 1).
//!
//! `POST /portal/admin/retainers/new` with the `onboarding__estate`
//! template must reuse the retainer's creation plumbing (Person +
//! Project + role + Notation) but, because the estate flow is
//! transcript-driven and has no questionnaire to walk before intake,
//! it must instead **start the workflow machine at BEGIN** and land
//! staff on the matter page (`/portal/projects/:id`) where the
//! transcript-upload form lives — not on the questionnaire walker.
//!
//! This proves the created matter is a live timeline the shipped
//! transcript handler can signal: after creation we fire
//! `transcript_uploaded` through the same runtime and assert it
//! advances onto `document_intake__transcript`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tower::ServiceExt;
use web::AppState;
use workflows::{MachineKind, StateMachineRuntime};

/// Session-cookie signing key shared by [`build_app`] and the tests that
/// mint a logged-in staff cookie against it.
const SESSION_KEY: &str = "test-session-key-not-for-production";

/// Build the app with the `onboarding__estate` template seeded (no
/// notation yet — creation is what the test exercises). Returns the
/// router, the db, and the shared workflow runtime so the test can
/// signal the freshly-started machine.
async fn build_app() -> (axum::Router, store::Db, Arc<dyn StateMachineRuntime>) {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::template;

    let db = store::test_support::pg().await;
    // Every matter now carries a NOT NULL staff DRI; the self-serve walk
    // resolves it to the firm principal (by role) when no staffer is in the
    // room. Seed one so the walk can open the matter.
    store::entity::person::ActiveModel {
        name: ActiveValue::Set("Firm Principal".into()),
        email: ActiveValue::Set("principal@example.com".into()),
        role: ActiveValue::Set(store::entity::person::Role::Admin),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed firm principal");
    template::ActiveModel {
        code: ActiveValue::Set("onboarding__estate".into()),
        title: ActiveValue::Set("Northstar Estate Plan".into()),
        respondent_type: ActiveValue::Set("person".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("seed estate template");

    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-creation-test"))
            .await
            .unwrap(),
    );
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );

    let state = AppState {
        storage,
        workflow_runtime: workflow_runtime.clone(),
        questionnaire_runtime: inner,
        email,
        sessions: web::SessionStore::new(SESSION_KEY),
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        workflow_runtime,
    )
}

#[tokio::test]
async fn creating_an_estate_matter_starts_the_workflow_and_lands_on_the_matter_page() {
    let (app, db, runtime) = build_app().await;

    let resp = app
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

    // The estate flow skips the questionnaire walker: it lands staff on
    // the matter page, where the transcript-upload form lives — not on
    // `/portal/admin/notations/:id/step`.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        location.starts_with("/portal/projects/"),
        "estate creation should land on the matter page, got {location}"
    );
    assert!(
        !location.contains("/notations/"),
        "estate creation must not redirect to the questionnaire walker, got {location}"
    );

    // The four lifecycle rows exist: Person, Project, role, and the
    // estate Notation parked at BEGIN.
    let template = store::entity::template::Entity::find()
        .filter(store::entity::template::Column::Code.eq("onboarding__estate"))
        .one(&db)
        .await
        .unwrap()
        .expect("estate template");
    let notation = store::entity::notation::Entity::find()
        .filter(store::entity::notation::Column::TemplateId.eq(template.id))
        .one(&db)
        .await
        .unwrap()
        .expect("estate notation created");
    assert_eq!(notation.state, "BEGIN");
    assert_eq!(
        location,
        format!("/portal/projects/{}", notation.project_id)
    );

    let person = store::entity::person::Entity::find_by_id(notation.person_id)
        .one(&db)
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

/// The matter page (`GET /portal/projects/:id`) is project-scoped:
/// `can_see_project` 404s a staff member with no `person_project_roles`
/// row on the matter. Estate creation redirects the opener *straight to*
/// that page, so unless creation discloses the opener as the matter's
/// staff DRI they land on a "Not found" — the exact gap the browser e2e
/// `staff_opens_an_estate_matter_and_sees_the_transcript_form` caught.
/// This pins the fix: a logged-in staffer who opens an estate matter is
/// disclosed to it and can load the transcript-upload page.
#[tokio::test]
async fn creating_staff_is_disclosed_to_the_estate_matter_they_open() {
    use http_body_util::BodyExt;
    use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
    use store::entity::person::Role;
    use store::entity::{person, person_project_role};
    use uuid::Uuid;
    use web::session::{SessionData, SESSION_COOKIE_NAME};

    let (app, db, _runtime) = build_app().await;

    // A real logged-in staffer (has a linked Person, unlike the `Bearer
    // dev` bypass which carries no `person_id` and so cannot be disclosed).
    let staff = person::ActiveModel {
        name: ActiveValue::Set("Opening Staffer".into()),
        email: ActiveValue::Set("opener@example.com".into()),
        role: ActiveValue::Set(Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let mut session = SessionData::fresh("opener-sub", Role::Staff);
    session.person_id = Some(staff.id);
    // Cookie-session POSTs to admin forms are CSRF-checked: the body must
    // echo the session's token (the `Bearer dev` path in the test above is
    // exempt, so it needs none).
    let csrf = session.csrf_token.clone();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}",
        web::SessionStore::new(SESSION_KEY).encode(&session)
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/retainers/new")
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
        .strip_prefix("/portal/projects/")
        .expect("estate creation lands on the matter page")
        .parse()
        .expect("redirect carries the project id");

    // The opener is disclosed to the new matter as its staff DRI.
    let dri = person_project_role::Entity::find()
        .filter(person_project_role::Column::ProjectId.eq(project_id))
        .filter(person_project_role::Column::PersonId.eq(staff.id))
        .one(&db)
        .await
        .unwrap()
        .expect("opening staffer is disclosed to the matter they created");
    assert_eq!(
        dri.participation,
        person_project_role::PARTICIPATION_STAFF_DRI
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
