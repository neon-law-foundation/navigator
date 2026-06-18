#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! End-to-end Northstar estate pipeline (seams 3 + 5): from creating an
//! estate matter, uploading the sitting transcript, through extraction
//! (answers, source `extracted`) and draft rendering (one
//! `review_documents` row per instrument at `draft`) to the attorney
//! gate (`staff_review`).
//!
//! Uses the canonical seed so the four `northstar__*` instrument
//! templates and the estate questions exist, then drives the real HTTP
//! routes end to end.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tower::ServiceExt;
use web::AppState;

const BOUNDARY: &str = "navigatorestateboundary";

async fn build_app() -> (axum::Router, store::Db) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-pipeline-test"))
            .await
            .unwrap(),
    );
    store::seed::seed_canonical(&db, &storage)
        .await
        .expect("canonical seed");

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );
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
    )
}

fn multipart_text_field(name: &str, value: &str) -> Vec<u8> {
    format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes()
}

#[tokio::test]
async fn uploading_a_transcript_extracts_answers_and_renders_four_draft_instruments() {
    let (app, db) = build_app().await;

    // Create the estate matter (seam 1): lands on the matter page.
    let resp = app
        .clone()
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
    let notation = store::entity::notation::Entity::find()
        .filter(store::entity::notation::Column::ProjectId.eq(project_id))
        .one(&db)
        .await
        .unwrap()
        .expect("estate notation");

    // Upload the sitting transcript (seam 2 handler + the pipeline).
    let transcript = "Recording consent given. Testator: Capricorn Stone. \
        Executor: Aries Vega. Successor trustee: Gemini Hart. \
        Guardian: Pisces Lake. Residuary beneficiary: Leo Sun. \
        Health-care agent: Virgo Reed. Financial agent: Libra Vale.";
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/projects/{project_id}/notations/{}/transcript",
                    notation.id
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
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // The matter reached the attorney gate.
    let notation = store::entity::notation::Entity::find_by_id(notation.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notation.state, "staff_review");

    // Four instrument drafts, one per kind, all at `draft` (hidden from
    // the client until an attorney advances them).
    let drafts = store::review_documents::for_notation(&db, notation.id)
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
            store::entity::review_document::STATUS_DRAFT,
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

    // The answers were persisted as machine-extracted, not staff/client.
    let extracted = store::entity::answer::Entity::find()
        .filter(store::entity::answer::Column::PersonId.eq(notation.person_id))
        .filter(store::entity::answer::Column::Source.eq(store::entity::answer::SOURCE_EXTRACTED))
        .all(&db)
        .await
        .unwrap();
    assert!(
        extracted.len() >= 7,
        "expected the labelled fields to be extracted, got {}",
        extracted.len()
    );

    // The client cannot see any of these drafts yet (the human gate).
    let client_visible = store::review_documents::client_visible_for_project(&db, project_id)
        .await
        .unwrap();
    assert!(
        client_visible.is_empty(),
        "drafts must be hidden from the client until an attorney advances them"
    );
}
