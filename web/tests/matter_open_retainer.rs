#![allow(clippy::doc_markdown)]
//! Dev e2e for "open a matter for an existing client and send the retainer
//! in one action."
//!
//! Drives the real HTTP path (`POST /portal/projects`, selecting an existing
//! `Role::Client` as the client DRI) against `StubSignatureProvider`, so
//! nothing reaches DocuSign and the test is CI-safe. Every matter opens on a
//! retainer (there is no plain-project path). Covers:
//!
//!   1. Happy path — the project (with its `client_dri_person_id` column set
//!      to the selected client), the client's `client` participation, and the
//!      retainer Notation land; the workflow parks at `staff_review` (the gate
//!      is not bypassed); the staff **approve** + **send** fires exactly one
//!      `send_for_signature` whose manifest carries the selected client's
//!      email/name, the `{{client.signature}}` anchor, and — because a
//!      matter-open client is emailed — a *non-captive* client recipient.
//!   2. Negative — no client selected → `422` and **no** matter created.
//!   3. Negative — a non-client person chosen as the client DRI → `422`.
//!   4. Negative — no entity → `422` and nothing opened.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed};
use tower::ServiceExt;
use views::assert_renders;
use web::signature::StubSignatureProvider;
use web::AppState;
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

async fn build_app(tag: &str) -> (axum::Router, store::Db, Arc<StubSignatureProvider>) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-matter-open-{tag}")))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();

    // The `document_open__retainer_pdf` step is worker-dispatched, so the
    // in-memory runtime is wrapped in `DispatchingRuntime` (the same
    // in-process path the dev binary uses) — otherwise the PDF is never
    // rendered/persisted and the signature read-back 404s.
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
        signature_provider: stub.clone(),
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        stub,
    )
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Tiny URL-encoder for the form bodies — only escapes what these values
/// actually contain.
fn enc(s: &str) -> String {
    s.replace(' ', "%20").replace('@', "%40")
}

/// Seed a pre-existing `Role::Client` person — the matter-open form now
/// opens a matter *for* an existing client (required `client_dri_person_id`
/// picker), so the client must exist before the POST.
async fn seed_client(db: &store::Db, name: &str, email: &str) -> uuid::Uuid {
    use sea_orm::{ActiveModelTrait, ActiveValue};
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
#[allow(clippy::too_many_lines)]
async fn matter_open_with_retainer_parks_at_staff_review_then_approve_sends_once() {
    let (app, db, stub) = build_app("happy").await;
    let entity_id = store::test_support::seed_entity(&db).await;
    // The client is selected from existing clients — seed them first.
    let client_id = seed_client(&db, "Libra Client", "libra@example.com").await;

    let body = format!(
        "name={}&status=open&entity_id={entity_id}\
         &client_dri_person_id={client_id}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services={}",
        enc("Libra estate"),
        enc("Flat-fee estate planning"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        loc.starts_with("/portal/admin/notations/") && loc.ends_with("/review"),
        "expected a redirect to the review screen, got {loc:?}",
    );
    let notation_id: uuid::Uuid = loc
        .trim_start_matches("/portal/admin/notations/")
        .trim_end_matches("/review")
        .parse()
        .expect("redirect carries the notation id");

    // (a) the project row exists, status `open`.
    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Libra estate"))
        .one(&db)
        .await
        .unwrap()
        .expect("project row inserted");
    assert_eq!(project.status, "open");

    // (b) the pre-existing client is linked via a `client` participation
    // (portal visibility) and is the matter's client-side DRI — now a
    // first-class column on the project, not a participation row.
    let person = entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("libra@example.com"))
        .one(&db)
        .await
        .unwrap()
        .expect("client person exists");
    assert_eq!(person.id, client_id);
    assert_eq!(person.name, "Libra Client");
    let roles = entity::person_project_role::Entity::find()
        .filter(entity::person_project_role::Column::PersonId.eq(person.id))
        .filter(entity::person_project_role::Column::ProjectId.eq(project.id))
        .all(&db)
        .await
        .unwrap();
    let participations: Vec<&str> = roles.iter().map(|r| r.participation.as_str()).collect();
    assert!(participations.contains(&"client"), "{participations:?}");
    // The client is the matter's client-side DRI (the authoritative column).
    assert_eq!(project.client_dri_person_id, Some(person.id));

    // The matter opened against the pre-existing entity.
    assert_eq!(project.entity_id, entity_id);

    // (c) the retainer Notation exists, parked at `staff_review` (gate not
    // bypassed), and marked for emailed delivery (matter-open client).
    let notation = entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .expect("retainer notation inserted");
    assert_eq!(notation.state, "staff_review");
    assert_eq!(notation.delivery, "emailed");
    assert_eq!(notation.person_id, person.id);

    // No envelope has gone out yet — the gate holds until approval.
    assert!(
        stub.calls().is_empty(),
        "no signature should be sent before staff approval",
    );

    // The review screen renders the assembled agreement + approve button.
    let review = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&loc)
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(review.status(), StatusCode::OK);
    let review_html = body_string(review).await;
    assert_renders!(&review_html, "portal.approve_send_signature");
    assert!(review_html.contains("Libra Client"), "html: {review_html}");
    assert!(review_html.contains("Flat-fee estate planning"));

    // (d) staff approve → renders + parks at document_open__retainer_pdf;
    // NO envelope yet (the send is a separate, deliberate command).
    let approve = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/admin/notations/{notation_id}/approve-send"
                ))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::OK);
    assert!(stub.calls().is_empty(), "approve must not send");

    // (e) the deliberate send → exactly one send_for_signature.
    let send = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/notations/{notation_id}/send"))
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(send.status(), StatusCode::OK);

    let calls = stub.calls();
    assert_eq!(calls.len(), 1, "exactly one envelope should be sent");
    let manifest = &calls[0].manifest;
    let client = manifest
        .recipients
        .iter()
        .find(|r| r.role == "client")
        .expect("manifest has a client recipient");
    assert_eq!(client.email, "libra@example.com");
    assert_eq!(client.name, "Libra Client");
    // Emailed delivery ⇒ non-captive: DocuSign emails the signing link.
    assert!(
        client.client_user_id.is_none(),
        "matter-open client should be emailed (non-captive)",
    );
    // The `{{client.signature}}` anchor is carried in the manifest fields.
    assert!(
        manifest
            .fields
            .iter()
            .any(|f| f.recipient_role == "client" && f.anchor.contains("nlsig-client-signature")),
        "manifest should place the client signature anchor; fields: {:?}",
        manifest.fields,
    );

    // The notation reached the terminal send state.
    let row = entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "sent_for_signature__pending");
}

#[tokio::test]
async fn matter_open_with_description_seeds_a_system_scope_clause() {
    // Commit 1: opening a matter with a description persists
    // `projects.description` and seeds the notation's position-0 custom
    // clause from it — System provenance (`authored_by_person_id = None`),
    // a draft the attorney edits at `staff_review`. The clause splices into
    // the agreement at `{{custom_clauses}}`, so it renders on the review
    // screen ahead of the firm's standing terms' trailing clauses.
    let (app, db) = {
        let (app, db, _stub) = build_app("description").await;
        (app, db)
    };

    let entity_id = store::test_support::seed_entity(&db).await;
    let client_id = seed_client(&db, "Capricorn Client", "capricorn@example.com").await;
    let description =
        "This engagement covers the Capricorn family revocable trust and a pour-over \
                       will, recorded in one sitting.";
    let body = format!(
        "name={}&status=open&entity_id={entity_id}&description={}\
         &client_dri_person_id={client_id}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services={}",
        enc("Capricorn estate"),
        enc(description),
        enc("Flat-fee estate planning"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let notation_id: uuid::Uuid = loc
        .trim_start_matches("/portal/admin/notations/")
        .trim_end_matches("/review")
        .parse()
        .expect("redirect carries the notation id");

    // (a) the description is persisted on the project.
    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Capricorn estate"))
        .one(&db)
        .await
        .unwrap()
        .expect("project row inserted");
    assert_eq!(project.description.as_deref(), Some(description));

    // (b) clause-0 is seeded from the description with System provenance.
    let clauses = entity::notation_clause::Entity::find()
        .filter(entity::notation_clause::Column::NotationId.eq(notation_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(clauses.len(), 1, "exactly one auto-seeded clause");
    assert_eq!(clauses[0].position, 0);
    assert_eq!(clauses[0].body_markdown, description);
    assert!(
        clauses[0].authored_by_person_id.is_none(),
        "auto-seeded scope clause must be System-provenance (no staff author)",
    );

    // (c) the clause renders into the agreement on the review screen.
    let review = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&loc)
                .header("authorization", "Bearer dev")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(review.status(), StatusCode::OK);
    let review_html = body_string(review).await;
    assert!(
        review_html.contains("Capricorn family revocable trust"),
        "the seeded scope clause should render in the agreement",
    );
}

#[tokio::test]
async fn matter_open_without_a_client_is_rejected_with_no_matter() {
    let (app, db, stub) = build_app("no-client").await;
    let entity_id = store::test_support::seed_entity(&db).await;

    // Entity + template present, but no client selected. Every matter opens
    // *for* a real client, so this is refused (422) and opens nothing.
    let body = format!(
        "name={}&status=open&entity_id={entity_id}\
         &retainer_template_code=onboarding__retainer",
        enc("Aries debt shield"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(html.to_lowercase().contains("client"), "html: {html}");

    // No half-open matter.
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Aries debt shield"))
            .one(&db)
            .await
            .unwrap()
            .is_none(),
        "no project should be created without a client",
    );
    assert!(stub.calls().is_empty());
}

#[tokio::test]
async fn matter_open_with_a_non_client_person_as_client_is_rejected() {
    let (app, db, stub) = build_app("non-client").await;
    let entity_id = store::test_support::seed_entity(&db).await;

    // `nick@neonlaw.com` is the seeded admin — not a client. Selecting a
    // non-client as the client DRI is refused: the client of record is a
    // client, never a firm attorney.
    let admin = entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("nick@neonlaw.com"))
        .one(&db)
        .await
        .unwrap()
        .expect("seeded admin");
    let body = format!(
        "name={}&status=open&entity_id={entity_id}\
         &client_dri_person_id={}\
         &retainer_template_code=onboarding__retainer",
        enc("Aries debt shield"),
        admin.id,
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(html.to_lowercase().contains("client"), "html: {html}");
    assert!(stub.calls().is_empty());
}

#[tokio::test]
async fn matter_open_without_an_entity_is_rejected() {
    // Commit 4: a matter always opens against a pre-existing entity. A
    // create with no `entity_id` is refused (422) and opens nothing.
    let (app, db, _stub) = build_app("no-entity").await;
    let client_id = seed_client(&db, "Pisces Client", "pisces@example.com").await;
    let body = format!(
        "name={}&status=open&client_dri_person_id={client_id}\
         &retainer_template_code=onboarding__retainer",
        enc("Entityless matter"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(html.to_lowercase().contains("entity"), "html: {html}");
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Entityless matter"))
            .one(&db)
            .await
            .unwrap()
            .is_none(),
        "no project should be created without an entity",
    );
}
