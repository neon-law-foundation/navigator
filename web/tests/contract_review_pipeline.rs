//! Inbound contract-review pipeline (web seam): upload a contract, run the
//! playbook deviation analysis web-side (the deterministic
//! `StubContractReviewer`), open a `contract_reviews` row with findings, and
//! land the matter at `staff_review`.
//!
//! Drives [`web::contract_review_walk::drive_contract_review`] directly (the
//! same public entry the multipart upload route calls) against a real
//! Postgres + the `DispatchingRuntime`, so the `document_intake` side effect
//! files the contract blob exactly as it does in the app.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use store::entity::{notation, person, project, template};
use store::playbooks::{NewPlaybook, Position};
use workflows::{DispatchingRuntime, InMemoryRuntime, IntakeArtifact};

async fn admin_state(db: store::Db) -> web::admin::AdminState {
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-contract-review-test"))
            .await
            .unwrap(),
    );
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(InMemoryRuntime::new());
    let runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone()).with_db(db.clone()),
    );
    web::admin::AdminState {
        db: db.clone(),
        workflow_runtime: runtime.clone(),
        signature_provider: Arc::new(web::signature::StubSignatureProvider::new()),
        retainer_intake_questionnaire: workflows::retainer_intake_questionnaire(),
        questionnaire_runtime: inner,
        assets_storage: storage.clone(),
        forms_registry: Arc::new(forms::registry().unwrap()),
        storage,
        email,
        billing_provider: Arc::new(web::billing::StubBillingProvider::new()),
        contract_reviewer: Arc::new(web::contract_review::StubContractReviewer),
        bootstrap_admin_email: None,
    }
}

/// Seed an Entity + Project + client Person + the contract-review template,
/// returning `(project_id, person_id, entity_id)`.
async fn seed_matter(db: &store::Db) -> (Uuid, Uuid, Uuid) {
    let entity_id = store::test_support::seed_entity(db).await;
    let __dri = store::test_support::dri_person(db).await;
    let project_id = project::ActiveModel {
        name: ActiveValue::Set("Nexus engagement".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;
    let person_id = person::ActiveModel {
        name: ActiveValue::Set("Aquarius".into()),
        email: ActiveValue::Set(format!("aquarius-{}@example.com", Uuid::now_v7())),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;
    template::ActiveModel {
        code: ActiveValue::Set("services__contract_review".into()),
        title: ActiveValue::Set("Inbound Contract Review".into()),
        respondent_type: ActiveValue::Set("person_and_entity".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    (project_id, person_id, entity_id)
}

fn sample_positions() -> Vec<Position> {
    vec![
        Position {
            topic: "Limitation of liability".into(),
            preferred: "Mutual cap at 12 months' fees".into(),
            fallback: "Cap at 2x fees paid".into(),
            walkaway: "Uncapped liability".into(),
            severity: store::playbooks::SEVERITY_HIGH.into(),
        },
        Position {
            topic: "Governing law".into(),
            preferred: "Nevada".into(),
            fallback: "Delaware".into(),
            walkaway: "A jurisdiction with no nexus".into(),
            severity: store::playbooks::SEVERITY_MEDIUM.into(),
        },
    ]
}

#[tokio::test]
async fn upload_runs_analysis_and_parks_at_staff_review() {
    let db = store::test_support::pg().await;
    let state = admin_state(db.clone()).await;
    let (project_id, person_id, entity_id) = seed_matter(&db).await;

    let positions = sample_positions();
    let playbook_id = store::playbooks::create(
        &db,
        &NewPlaybook {
            entity_id,
            name: "Vendor MSA playbook",
            positions: &positions,
        },
    )
    .await
    .unwrap();

    let review_id = web::contract_review_walk::drive_contract_review(
        &state,
        project_id,
        person_id,
        "vendor-msa.txt",
        "MASTER SERVICES AGREEMENT\nLiability is uncapped. Governed by the laws of Mars.",
        IntakeArtifact::Text {
            text: "MASTER SERVICES AGREEMENT\nLiability is uncapped.".into(),
        },
    )
    .await
    .expect("pipeline runs");

    // The review row carries the playbook, a risk summary, and one finding
    // per playbook position — every one un-accepted (the attorney must act).
    let review = store::contract_reviews::by_id(&db, review_id)
        .await
        .unwrap()
        .expect("review row exists");
    assert_eq!(review.playbook_id, playbook_id);
    assert_eq!(
        review.status,
        store::entity::contract_review::STATUS_ANALYZED
    );
    let findings = store::contract_reviews::findings_of(&review).unwrap();
    assert_eq!(findings.len(), 2, "one finding per playbook position");
    assert!(findings.iter().all(|f| !f.accepted));
    assert!(review.risk_summary.is_some());

    // The inbound contract was filed into the project as a documents row.
    assert!(
        review.document_id.is_some(),
        "the filed inbound-contract document is linked"
    );

    // The notation reached the attorney gate.
    let notation_row = notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(project_id))
        .one(&db)
        .await
        .unwrap()
        .expect("notation exists");
    assert_eq!(notation_row.state, "staff_review");
    assert_eq!(notation_row.entity_id, Some(entity_id));
}

#[tokio::test]
async fn upload_without_a_playbook_is_rejected() {
    let db = store::test_support::pg().await;
    let state = admin_state(db.clone()).await;
    let (project_id, person_id, _entity_id) = seed_matter(&db).await;

    let err = web::contract_review_walk::drive_contract_review(
        &state,
        project_id,
        person_id,
        "vendor-msa.txt",
        "contract body",
        IntakeArtifact::Text {
            text: "contract body".into(),
        },
    )
    .await
    .expect_err("no playbook on file");
    assert!(matches!(
        err,
        web::contract_review_walk::ContractReviewError::NoPlaybook
    ));

    // No notation was opened — we fail before touching the workflow.
    let count = notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(project_id))
        .all(&db)
        .await
        .unwrap()
        .len();
    assert_eq!(count, 0);
}
