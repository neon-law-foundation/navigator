//! `raise_matter_close_fee` persists a Xero invoice mirror row.
//!
//! When the firm signs a matter's closing letter, the flat close fee is
//! raised through the `BillingProvider` seam *and* mirrored locally into
//! `xero_invoices` so the portal can show the per-project invoice without
//! calling Xero live. These tests pin that wiring against the in-process
//! `StubBillingProvider`: a raise creates exactly one Xero invoice and
//! one mirror row, and a replay (double-close) never writes a second row.

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::entity::{notation, person, product, project, template, xero_invoice};
use uuid::Uuid;
use web::admin::AdminState;
use web::billing::StubBillingProvider;
use web::signature::StubSignatureProvider;
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

/// Build a minimal `AdminState` over a real test database, keeping the
/// concrete `StubBillingProvider` handle so the test can read its
/// recorded calls.
async fn build_state() -> (AdminState, store::Db, Arc<StubBillingProvider>) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-close-fee-storage"))
            .await
            .unwrap(),
    );
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let runtime = Arc::new(InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(DispatchingRuntime::new(
        runtime.clone(),
        email.clone(),
        storage.clone(),
    ));
    let billing = Arc::new(StubBillingProvider::new());
    let state = AdminState {
        db: db.clone(),
        workflow_runtime: workflow_runtime.clone(),
        signature_provider: Arc::new(StubSignatureProvider::new()),
        retainer_intake_questionnaire: workflows::retainer_intake_questionnaire(),
        questionnaire_runtime: runtime,
        storage,
        email,
        billing_provider: billing.clone(),
        contract_reviewer: Arc::new(web::contract_review::StubContractReviewer),
        bootstrap_admin_email: None,
    };
    (state, db, billing)
}

async fn seed_template(db: &store::Db, code: &str) -> Uuid {
    template::ActiveModel {
        code: ActiveValue::Set(code.into()),
        title: ActiveValue::Set(code.into()),
        respondent_type: ActiveValue::Set("person_and_entity".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn seed_notation(
    db: &store::Db,
    project_id: Uuid,
    person_id: Uuid,
    template_id: Uuid,
) -> Uuid {
    notation::ActiveModel {
        template_id: ActiveValue::Set(template_id),
        person_id: ActiveValue::Set(person_id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

/// Seed the Northstar product row so the catalog lookup behind
/// `flat_fee_cents` resolves `onboarding__estate` to its $3,333 close fee.
/// `flat_fee_cents` reads the `products` table (`store::products`) rather
/// than a hard-coded match, so a matter-close test must seed the product
/// or every raise early-returns as a no-op. Mirrors `store/seeds/Product.yaml`.
async fn seed_northstar_product(db: &store::Db) {
    product::ActiveModel {
        code: ActiveValue::Set("northstar".into()),
        display_name: ActiveValue::Set("Neon Law Northstar".into()),
        list_price_cents: ActiveValue::Set(333_300),
        currency: ActiveValue::Set("USD".into()),
        cadence: ActiveValue::Set(product::CADENCE_ONCE.into()),
        billing_kind: ActiveValue::Set(product::BILLING_KIND_MATTER_CLOSE_FLAT.into()),
        active: ActiveValue::Set(true),
        xero_item_code: ActiveValue::Set(Some("NORTHSTAR".into())),
        matter_close_template_code: ActiveValue::Set(Some("onboarding__estate".into())),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

/// Seed a closed estate matter (the work notation carries a flat fee) and
/// return the closing notation's id + the project id.
async fn seed_closed_estate(db: &store::Db) -> (Uuid, Uuid) {
    seed_northstar_product(db).await;
    let estate_tmpl = seed_template(db, "onboarding__estate").await;
    let closing_tmpl = seed_template(db, "closing__letter").await;
    let client = person::ActiveModel {
        name: ActiveValue::Set("Capricorn".into()),
        email: ActiveValue::Set("capricorn@example.com".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("estate matter".into()),
        status: ActiveValue::Set("closed".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    seed_notation(db, proj.id, client.id, estate_tmpl).await;
    let closing = seed_notation(db, proj.id, client.id, closing_tmpl).await;
    (closing, proj.id)
}

#[tokio::test]
async fn raise_persists_one_mirror_row_and_bills_once() {
    let (state, db, billing) = build_state().await;
    let (closing_id, project_id) = seed_closed_estate(&db).await;

    web::retainer_walk::raise_matter_close_fee(&state, closing_id)
        .await
        .unwrap();

    // One Xero invoice created via the seam.
    assert_eq!(billing.calls().len(), 1);

    // One mirror row, carrying the Northstar flat fee and the project key.
    let rows = xero_invoice::Entity::find()
        .filter(xero_invoice::Column::ProjectId.eq(project_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 333_300);
    assert_eq!(rows[0].status, "AUTHORISED");
    assert_eq!(rows[0].reference, format!("Matter {project_id}"));
}

#[tokio::test]
async fn replay_does_not_write_a_second_row() {
    let (state, db, _billing) = build_state().await;
    let (closing_id, project_id) = seed_closed_estate(&db).await;

    web::retainer_walk::raise_matter_close_fee(&state, closing_id)
        .await
        .unwrap();
    web::retainer_walk::raise_matter_close_fee(&state, closing_id)
        .await
        .unwrap();

    let rows = xero_invoice::Entity::find()
        .filter(xero_invoice::Column::ProjectId.eq(project_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "a double-close must update the one row");
}

#[tokio::test]
async fn matter_with_no_flat_fee_is_a_noop() {
    let (state, db, billing) = build_state().await;
    // A matter whose only work notation has no flat close fee.
    let other_tmpl = seed_template(&db, "onboarding__nautilus").await;
    let closing_tmpl = seed_template(&db, "closing__letter").await;
    let client = person::ActiveModel {
        name: ActiveValue::Set("Aries".into()),
        email: ActiveValue::Set("aries@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("debt matter".into()),
        status: ActiveValue::Set("closed".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    seed_notation(&db, proj.id, client.id, other_tmpl).await;
    let closing = seed_notation(&db, proj.id, client.id, closing_tmpl).await;

    web::retainer_walk::raise_matter_close_fee(&state, closing)
        .await
        .unwrap();

    assert_eq!(billing.calls().len(), 0, "no flat fee → no Xero invoice");
    assert_eq!(
        xero_invoice::Entity::find().all(&db).await.unwrap().len(),
        0,
        "no flat fee → no mirror row"
    );
}
