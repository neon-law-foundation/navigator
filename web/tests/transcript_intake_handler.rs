#![allow(clippy::doc_markdown)]
//! Integration test for the Northstar transcript-upload surface.
//!
//! `POST /portal/projects/:id/notations/:nid/transcript` files a
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
use sea_orm::EntityTrait;
use tower::ServiceExt;
use web::AppState;
use workflows::{MachineKind, StateMachineRuntime};

const BOUNDARY: &str = "navigatortestboundary";

/// Build the app with an estate notation whose workflow is started and
/// parked at BEGIN, ready for the transcript upload. Returns the router,
/// the db, the project id, and the notation id.
async fn build_app() -> (axum::Router, store::Db, uuid::Uuid, uuid::Uuid) {
    let db = store::test_support::pg().await;
    let notation_id = store::test_support::seed_notation(&db).await;
    let project_id = store::entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .expect("seeded notation")
        .project_id;

    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-transcript-intake-test"))
            .await
            .unwrap(),
    );

    // The estate workflow must be started so the `transcript_uploaded`
    // signal from BEGIN is valid. Wrap the in-memory runtime in
    // DispatchingRuntime+with_db so the document-intake dispatch files
    // the transcript for real.
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
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
        storage,
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        project_id,
        notation_id,
    )
}

/// One `multipart/form-data` text field.
fn multipart_text_field(name: &str, value: &str) -> Vec<u8> {
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn transcript_text_upload_files_a_document_and_advances_state() {
    let (app, db, project_id, notation_id) = build_app().await;

    let transcript = "Consent recorded. Executor: Aries. Successor trustee: Capricorn.";
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{project_id}/notations/{notation_id}/transcript"
                ))
                .header("authorization", "Bearer dev")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_text_field(
                    "transcript_text",
                    transcript,
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    // Redirect back to the matter.
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        format!("/portal/projects/{project_id}")
    );

    // The transcript filed as a `documents` row on the matter's project.
    let doc = store::entity::document::Entity::find()
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|d| d.project_id == project_id && d.kind == "transcript")
        .expect("a transcript document filed on the project");
    assert_eq!(doc.source, "upload");

    let blob = store::entity::blob::Entity::find_by_id(doc.blob_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blob.content_type, "text/plain");

    // The handler files the transcript and then drives the estate
    // pipeline (extract → drafts → staff_review). This fixture seeds only
    // the estate template (no questions, no instrument templates), so the
    // pipeline persists no answers and renders no drafts, but it still
    // advances the durable machine to the attorney gate — proving the
    // continuation runs and syncs state.
    let notation = store::entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "staff_review");
}

#[tokio::test]
async fn transcript_upload_for_wrong_project_is_not_found() {
    let (app, _db, _project_id, notation_id) = build_app().await;
    // A different (random) project id in the URL must 404 — the
    // cross-resource guard rejects tunnelling the notation through
    // another project's URL.
    let other_project = uuid::Uuid::now_v7();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{other_project}/notations/{notation_id}/transcript"
                ))
                .header("authorization", "Bearer dev")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_text_field("transcript_text", "x")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
