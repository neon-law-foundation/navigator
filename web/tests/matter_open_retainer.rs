#![allow(clippy::doc_markdown)]
//! Dev e2e for "open a matter and send the retainer in one action."
//!
//! Drives the real HTTP path (`POST /portal/projects` with the retainer
//! box ticked) against `StubSignatureProvider`, so nothing reaches
//! DocuSign and the test is CI-safe. Covers:
//!
//!   1. Happy path — the project, client Person + `client` role, and the
//!      retainer Notation land; the workflow parks at `staff_review`
//!      (the gate is not bypassed); the staff **approve** step fires
//!      exactly one `send_for_signature` whose manifest carries the
//!      form's signer email/name, the `{{client.signature}}` anchor, and
//!      — because a matter-open client is emailed — a *non-captive*
//!      client recipient.
//!   2. Negative — box ticked but the signer email is missing → `4xx`
//!      and **no** matter created (no half-open matter).
//!   3. Unchecked — a plain project create still works and records **no**
//!      signature call.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::{entity, seed};
use tower::ServiceExt;
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

    let body = format!(
        "name={}&status=open&entity_id={entity_id}&send_retainer=true\
         &retainer_template_code=onboarding__retainer\
         &client_name={}&client_email={}&scope_of_services={}",
        enc("Libra estate"),
        enc("Libra Client"),
        enc("libra@example.com"),
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

    // (b) a client Person + `client` participation role is linked.
    let person = entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("libra@example.com"))
        .one(&db)
        .await
        .unwrap()
        .expect("client person inserted");
    assert_eq!(person.name, "Libra Client");
    let roles = entity::person_project_role::Entity::find()
        .filter(entity::person_project_role::Column::PersonId.eq(person.id))
        .filter(entity::person_project_role::Column::ProjectId.eq(project.id))
        .all(&db)
        .await
        .unwrap();
    let participations: Vec<&str> = roles.iter().map(|r| r.participation.as_str()).collect();
    assert!(participations.contains(&"client"), "{participations:?}");
    // The client is also the matter's client-side DRI.
    assert!(
        participations.contains(&"client_dri"),
        "client should be designated the client DRI: {participations:?}",
    );

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
    assert!(review_html.contains("Approve and send for signature"));
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
    let description =
        "This engagement covers the Capricorn family revocable trust and a pour-over \
                       will, recorded in one sitting.";
    let body = format!(
        "name={}&status=open&entity_id={entity_id}&description={}&send_retainer=true\
         &retainer_template_code=onboarding__retainer\
         &client_name={}&client_email={}&scope_of_services={}",
        enc("Capricorn estate"),
        enc(description),
        enc("Capricorn Client"),
        enc("capricorn@example.com"),
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
async fn retainer_box_ticked_without_signer_email_is_rejected_with_no_matter() {
    let (app, db, stub) = build_app("negative").await;

    // Box ticked, template + name present, but the signer email is blank.
    let body = format!(
        "name={}&status=open&send_retainer=true&retainer_template_code=onboarding__retainer\
         &client_name={}&client_email=",
        enc("Aries debt shield"),
        enc("Aries Client"),
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_string(resp).await;
    assert!(html.contains("client email"), "html: {html}");

    // No half-open matter: neither the project, the person, nor a notation
    // was created.
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Aries debt shield"))
            .one(&db)
            .await
            .unwrap()
            .is_none(),
        "no project should be created on a rejected retainer",
    );
    assert!(entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("aries@example.com"))
        .one(&db)
        .await
        .unwrap()
        .is_none(),);
    assert!(stub.calls().is_empty());
}

#[tokio::test]
async fn matter_open_without_an_entity_is_rejected() {
    // Commit 4: a matter always opens against a pre-existing entity. A
    // create with no `entity_id` is refused (422) and opens nothing.
    let (app, db, _stub) = build_app("no-entity").await;
    let body = format!(
        "name={}&status=open&send_retainer=true&retainer_template_code=onboarding__retainer\
         &client_name={}&client_email={}",
        enc("Entityless matter"),
        enc("Pisces Client"),
        enc("pisces@example.com"),
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

#[tokio::test]
async fn plain_project_create_without_retainer_records_no_signature_call() {
    let (app, db, stub) = build_app("unchecked").await;
    let entity_id = store::test_support::seed_entity(&db).await;

    // No `send_retainer` field at all — a plain project create (still
    // opened against a pre-existing entity).
    let body = format!(
        "name={}&status=open&entity_id={entity_id}",
        enc("Plain matter")
    );
    let resp = post_projects(&app, body).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/portal/projects"),
    );

    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Plain matter"))
        .one(&db)
        .await
        .unwrap()
        .expect("plain project created");
    // No retainer notation hangs off it.
    let notations = entity::notation::Entity::find()
        .filter(entity::notation::Column::ProjectId.eq(project.id))
        .all(&db)
        .await
        .unwrap();
    assert!(
        notations.is_empty(),
        "plain create must not open a retainer"
    );
    assert!(stub.calls().is_empty());
}
