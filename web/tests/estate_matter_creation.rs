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

/// Build the app with the `onboarding__estate` template seeded (no
/// notation yet — creation is what the test exercises). Returns the
/// router, the db, and the shared workflow runtime so the test can
/// signal the freshly-started machine.
async fn build_app() -> (axum::Router, store::Db, Arc<dyn StateMachineRuntime>) {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::template;

    let db = store::test_support::pg().await;
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
