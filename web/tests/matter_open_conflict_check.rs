#![allow(clippy::doc_markdown)]
//! Dev e2e for the pre-matter conflict check wired into `POST
//! /portal/projects`.
//!
//! Drives the real HTTP path against the in-memory workflow runtime, so
//! nothing reaches DocuSign. Covers the three gate outcomes:
//!
//!   1. **Block** — the proposed client is directly `adverse_to` a current
//!      client → `422`, no matter created, no override offered.
//!   2. **Review, not acknowledged** — the proposed matter shares an entity
//!      with another client's open matter → `422` with the acknowledgment
//!      checkbox, no matter created.
//!   3. **Review, acknowledged** — the same submit with `conflict_ack=1` →
//!      the matter opens (`303`) and a `relationship_logs` row records the
//!      override.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed};
use tower::ServiceExt;
use web::signature::StubSignatureProvider;
use web::AppState;
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

async fn build_app(tag: &str) -> (axum::Router, store::Db) {
    let repo_root = std::env::temp_dir().join(format!(
        "navigator-conflict-repos-{tag}-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&repo_root).unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", &repo_root);

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-conflict-{tag}")))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();
    let runtime = Arc::new(InMemoryRuntime::new());
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(DispatchingRuntime::new(
        runtime.clone(),
        email.clone(),
        storage.clone(),
    ));
    let stub = Arc::new(StubSignatureProvider::new());
    let state = AppState {
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime,
        signature_provider: stub,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
    )
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn enc(s: &str) -> String {
    s.replace(' ', "%20").replace('@', "%40")
}

async fn seed_client(db: &store::Db, name: &str, email: &str) -> uuid::Uuid {
    entity::person::ActiveModel {
        name: ActiveValue::Set(name.into()),
        email: ActiveValue::Set(email.into()),
        role: ActiveValue::Set(entity::person::Role::Client),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

/// Open an existing matter for `client` against `entity_id`, so the graph
/// sees that entity / person as a party the firm already serves.
async fn seed_open_project(db: &store::Db, entity_id: uuid::Uuid, client: uuid::Uuid) {
    let staff = store::test_support::dri_person(db).await;
    entity::project::ActiveModel {
        name: ActiveValue::Set("Existing matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(staff)),
        client_dri_person_id: ActiveValue::Set(Some(client)),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn post_projects(app: &axum::Router, body: String) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/projects")
                .header("authorization", "Bearer dev")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn adverse_to_current_client_blocks_the_open() {
    let (app, db) = build_app("block").await;

    // The opponent is already a client of the firm (an open matter).
    let opponent = seed_client(&db, "Opposing Party", "opponent@example.com").await;
    let opp_entity = store::test_support::seed_entity(&db).await;
    seed_open_project(&db, opp_entity, opponent).await;

    // The proposed client is directly adverse to that current client.
    let proposed = seed_client(&db, "New Client", "newclient@example.com").await;
    entity::relationship_edge::ActiveModel {
        from_type: ActiveValue::Set(entity::relationship_edge::NODE_PERSON.into()),
        from_id: ActiveValue::Set(proposed),
        to_type: ActiveValue::Set(entity::relationship_edge::NODE_PERSON.into()),
        to_id: ActiveValue::Set(opponent),
        kind: ActiveValue::Set(entity::relationship_edge::KIND_ADVERSE_TO.into()),
        confidence_pct: ActiveValue::Set(100),
        source_kind: ActiveValue::Set(entity::relationship_edge::SOURCE_MANUAL.into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let new_entity = store::test_support::seed_entity(&db).await;
    let body = format!(
        "name={}&status=open&entity_id={new_entity}\
         &client_dri_person_id={proposed}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services={}",
        enc("Adverse matter"),
        enc("Some work"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(
        html.contains("adverse to a current client"),
        "expected the block message, got: {html}",
    );
    // A hard block offers no override checkbox.
    assert!(
        !html.contains("name=\"conflict_ack\""),
        "a blocking conflict must not offer an override",
    );
    // Nothing was created.
    let count = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Adverse matter"))
        .all(&db)
        .await
        .unwrap();
    assert!(count.is_empty(), "no matter should open on a hard block");
}

#[tokio::test]
async fn shared_party_warns_then_opens_on_acknowledgment() {
    let (app, db) = build_app("review").await;

    // The firm already runs a matter on this entity for another client.
    let existing = seed_client(&db, "Existing Client", "existing@example.com").await;
    let shared_entity = store::test_support::seed_entity(&db).await;
    seed_open_project(&db, shared_entity, existing).await;

    let proposed = seed_client(&db, "Second Client", "second@example.com").await;
    let base = format!(
        "name={}&status=open&entity_id={shared_entity}\
         &client_dri_person_id={proposed}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services={}",
        enc("Shared entity matter"),
        enc("Some work"),
    );

    // First submit, no acknowledgment → review warning + checkbox, nothing
    // created.
    let resp = post_projects(&app, base.clone()).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(
        html.contains("flagged this matter for review"),
        "expected the review message, got: {html}",
    );
    assert!(
        html.contains("name=\"conflict_ack\""),
        "review-level findings must offer an acknowledgment checkbox",
    );
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Shared entity matter"))
            .all(&db)
            .await
            .unwrap()
            .is_empty(),
        "no matter should open before acknowledgment",
    );

    // A crafted POST with the field present but no checked value is still not
    // an acknowledgment.
    let resp = post_projects(&app, format!("{base}&conflict_ack=")).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(
        html.contains("flagged this matter for review"),
        "empty conflict_ack must keep the review gate in place, got: {html}",
    );
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Shared entity matter"))
            .all(&db)
            .await
            .unwrap()
            .is_empty(),
        "empty conflict_ack must not open the matter",
    );

    // Second submit with the acknowledgment → the matter opens and the
    // override is recorded to the relationship log.
    let resp = post_projects(&app, format!("{base}&conflict_ack=1")).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Shared entity matter"))
        .one(&db)
        .await
        .unwrap()
        .expect("matter opens after acknowledgment");

    let audit = entity::relationship_log::Entity::find()
        .filter(entity::relationship_log::Column::SubjectId.eq(project.id))
        .filter(entity::relationship_log::Column::Action.eq("conflict_review_acknowledged"))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(
        audit.len(),
        1,
        "the override should leave exactly one audit entry",
    );
}
