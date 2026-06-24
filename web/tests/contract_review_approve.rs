//! Inbound contract review — the attorney review screen through memo
//! delivery, driven over the real HTTP routes.
//!
//! Sets up a review parked at `staff_review` (via the same public pipeline
//! entry the upload route uses), then as an admin: GETs the review screen,
//! accepts the finding, and approves — asserting the workflow reaches `END`,
//! the review is `approved`, and the memo PDF is filed into the Project
//! (`documents` row + storage).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use tower::ServiceExt;
use uuid::Uuid;

use store::entity::{notation, person, project, template};
use store::playbooks::{NewPlaybook, Position};
use web::session::SessionData;
use web::AppState;
use workflows::{DispatchingRuntime, InMemoryRuntime, IntakeArtifact};

struct Harness {
    app: axum::Router,
    admin_state: web::admin::AdminState,
    db: store::Db,
    storage: Arc<dyn cloud::StorageService>,
    admin_bearer: String,
}

async fn harness() -> Harness {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-contract-approve-test"))
            .await
            .unwrap(),
    );
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(InMemoryRuntime::new());
    let runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone()).with_db(db.clone()),
    );
    let admin_state = web::admin::AdminState {
        db: db.clone(),
        workflow_runtime: runtime.clone(),
        signature_provider: Arc::new(web::signature::StubSignatureProvider::new()),
        retainer_intake_questionnaire: workflows::retainer_intake_questionnaire(),
        questionnaire_runtime: inner.clone(),
        storage: storage.clone(),
        email: email.clone(),
        billing_provider: Arc::new(web::billing::StubBillingProvider::new()),
        contract_reviewer: Arc::new(web::contract_review::StubContractReviewer),
        bootstrap_admin_email: None,
    };
    let state = AppState {
        storage: storage.clone(),
        workflow_runtime: runtime,
        questionnaire_runtime: inner,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // An admin session blob, presented as a Bearer credential (the CLI
    // path) so it injects an admin `SessionData` and bypasses CSRF.
    let sessions = web::SessionStore::new(web::test_support::TEST_SESSION_KEY);
    let admin_bearer = sessions.encode(&SessionData::fresh(
        "nick@neonlaw.com",
        store::entity::person::Role::Admin,
    ));

    Harness {
        app,
        admin_state,
        db,
        storage,
        admin_bearer,
    }
}

async fn seed_review_at_staff_review(h: &Harness) -> (Uuid, Uuid) {
    let entity_id = store::test_support::seed_entity(&h.db).await;
    let __dri = store::test_support::dri_person(&h.db).await;
    let project_id = project::ActiveModel {
        name: ActiveValue::Set("Nexus engagement".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&h.db)
    .await
    .unwrap()
    .id;
    let person_id = person::ActiveModel {
        name: ActiveValue::Set("Aquarius".into()),
        email: ActiveValue::Set(format!("aquarius-{}@example.com", Uuid::now_v7())),
        ..Default::default()
    }
    .insert(&h.db)
    .await
    .unwrap()
    .id;
    template::ActiveModel {
        code: ActiveValue::Set("services__contract_review".into()),
        title: ActiveValue::Set("Inbound Contract Review".into()),
        respondent_type: ActiveValue::Set("person_and_entity".into()),
        ..Default::default()
    }
    .insert(&h.db)
    .await
    .unwrap();

    let positions = vec![Position {
        topic: "Limitation of liability".into(),
        preferred: "Mutual cap at 12 months' fees".into(),
        fallback: "Cap at 2x fees paid".into(),
        walkaway: "Uncapped liability".into(),
        severity: store::playbooks::SEVERITY_HIGH.into(),
    }];
    store::playbooks::create(
        &h.db,
        &NewPlaybook {
            entity_id,
            name: "Vendor MSA playbook",
            positions: &positions,
        },
    )
    .await
    .unwrap();

    let review_id = web::contract_review_walk::drive_contract_review(
        &h.admin_state,
        project_id,
        person_id,
        "vendor-msa.txt",
        "MASTER SERVICES AGREEMENT. Liability is uncapped.",
        IntakeArtifact::Text {
            text: "MASTER SERVICES AGREEMENT. Liability is uncapped.".into(),
        },
    )
    .await
    .unwrap();
    (review_id, project_id)
}

async fn post(h: &Harness, uri: &str, body: &'static str) -> axum::http::Response<Body> {
    h.app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", format!("Bearer {}", h.admin_bearer))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn attorney_accepts_finding_and_approves_delivering_the_memo() {
    let h = harness().await;
    let (review_id, project_id) = seed_review_at_staff_review(&h).await;

    // The review screen renders.
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/portal/admin/contract-reviews/{review_id}"))
                .header("authorization", format!("Bearer {}", h.admin_bearer))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Approving before acting on the finding is refused (no memo, still
    // at staff_review).
    let resp = post(
        &h,
        &format!("/portal/admin/contract-reviews/{review_id}/approve"),
        "",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK); // re-renders with the error
    let notation_row = notation_for_project(&h.db, project_id).await;
    assert_eq!(notation_row.state, "staff_review");

    // Accept the one finding.
    let resp = post(
        &h,
        &format!("/portal/admin/contract-reviews/{review_id}/findings/0"),
        "decision=accept&severity=high&suggested_redline=Add+a+mutual+cap.&attorney_note=Push+this.",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // Now approve — assembles + delivers the memo, drives to END.
    let resp = post(
        &h,
        &format!("/portal/admin/contract-reviews/{review_id}/approve"),
        "",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    // The review is approved and the workflow reached END.
    let review = store::contract_reviews::by_id(&h.db, review_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        review.status,
        store::entity::contract_review::STATUS_APPROVED
    );
    let notation_row = notation_for_project(&h.db, project_id).await;
    assert_eq!(notation_row.state, "END");
    let findings = store::contract_reviews::findings_of(&review).unwrap();
    assert!(findings[0].accepted);
    assert_eq!(findings[0].attorney_note.as_deref(), Some("Push this."));

    // The memo PDF was filed into the Project as a documents row and to
    // storage.
    let memo = store::entity::document::Entity::find()
        .filter(store::entity::document::Column::ProjectId.eq(project_id))
        .filter(store::entity::document::Column::Kind.eq("review_memo"))
        .one(&h.db)
        .await
        .unwrap()
        .expect("review_memo document row exists");
    assert_eq!(memo.filename, "review-memo.pdf");
    assert!(h
        .storage
        .exists(&web::admin_contract_reviews::memo_storage_key(
            notation_row.id
        ))
        .await
        .unwrap());

    // The per-finding decision was recorded as an immutable attribution
    // event (distinct machine kind).
    let events = store::entity::notation_event::Entity::find()
        .filter(store::entity::notation_event::Column::NotationId.eq(notation_row.id))
        .filter(
            store::entity::notation_event::Column::MachineKind
                .eq(web::admin_contract_reviews::MACHINE_CONTRACT_REVIEW),
        )
        .all(&h.db)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].condition, "finding_accepted");
}

#[tokio::test]
async fn rejecting_the_review_ends_without_a_memo() {
    let h = harness().await;
    let (review_id, project_id) = seed_review_at_staff_review(&h).await;

    let resp = post(
        &h,
        &format!("/portal/admin/contract-reviews/{review_id}/reject"),
        "",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let review = store::contract_reviews::by_id(&h.db, review_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        review.status,
        store::entity::contract_review::STATUS_REJECTED
    );
    let notation_row = notation_for_project(&h.db, project_id).await;
    assert_eq!(notation_row.state, "END");

    // No memo document was filed.
    let memo = store::entity::document::Entity::find()
        .filter(store::entity::document::Column::ProjectId.eq(project_id))
        .filter(store::entity::document::Column::Kind.eq("review_memo"))
        .one(&h.db)
        .await
        .unwrap();
    assert!(memo.is_none());
}

async fn notation_for_project(db: &store::Db, project_id: Uuid) -> notation::Model {
    notation::Entity::find()
        .filter(notation::Column::ProjectId.eq(project_id))
        .one(db)
        .await
        .unwrap()
        .expect("notation exists")
}
