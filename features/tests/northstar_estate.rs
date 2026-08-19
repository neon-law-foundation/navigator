//! Cucumber runner for `features/northstar_estate.feature`.
//!
//! The flagship cross-surface journey: one estate matter touching the
//! first-party review surface (real web routes, a forged client session +
//! CSRF) and the closing walker (real admin HTTP). It proves the seams
//! between crates, not one handler — including the negative seam, that
//! closing a matter leaves the accounting stub untouched.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cucumber::{given, then, when, World};
use features::body_string;
use features::journey::{client, matter, Journey};
use portal::session::{SessionData, SessionStore, SESSION_COOKIE_NAME};
use store::persons::Role;
use store::review_documents::{self, NewReviewDocument};
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "test-session-key-not-for-production";

#[derive(Default, World)]
#[world(init = Self::default)]
struct NorthstarWorld {
    journey: Option<Journey>,
    person_id: Option<Uuid>,
    project_id: Option<Uuid>,
    project_code: Option<String>,
    doc_id: Option<Uuid>,
    comment_id: Option<Uuid>,
    cookie: Option<String>,
    csrf: Option<String>,
}

impl std::fmt::Debug for NorthstarWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NorthstarWorld")
            .field("project_id", &self.project_id)
            .field("doc_id", &self.doc_id)
            .finish_non_exhaustive()
    }
}

impl NorthstarWorld {
    fn journey(&self) -> &Journey {
        self.journey.as_ref().expect("journey not built")
    }

    fn review_path(&self) -> String {
        format!(
            "/app/projects/{}/review/{}",
            self.project_code.as_deref().unwrap(),
            self.doc_id.unwrap(),
        )
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> planning their estate$"#)]
async fn seed_client(world: &mut NorthstarWorld, name: String, email: String) {
    let journey = Journey::open("northstar").await;
    let person = client(&journey.surreal, &name, &email).await;
    let project_id = matter(&journey.surreal, person.id, "Northstar estate plan").await;

    // A real signed session cookie for Capricorn, so the review surface
    // (cookie + CSRF gated) accepts them as the scoped client.
    let sessions = SessionStore::new(KEY);
    let mut session = SessionData::fresh("capricorn-sub", Role::Client);
    session.person_id = Some(person.id);
    world.csrf = Some(session.csrf_token.clone());
    world.cookie = Some(format!(
        "{SESSION_COOKIE_NAME}={}",
        sessions.encode(&session)
    ));

    world.person_id = Some(person.id);
    world.project_id = Some(project_id);
    world.project_code = Some(
        store::projects::find_by_id(&journey.surreal, project_id)
            .await
            .expect("query project")
            .expect("the matter just opened")
            .code,
    );
    world.journey = Some(journey);
}

#[when("AIDA opens the estate matter and the attorney drafts the will")]
async fn open_and_draft(world: &mut NorthstarWorld) {
    let journey = world.journey();
    // AIDA opens the onboarding__estate notation (the matter's work record).
    let template =
        store::templates::find_by_id(&journey.surreal, estate_template_id(journey).await)
            .await
            .unwrap()
            .expect("estate template");
    let notation_id = store::notations::create(
        &journey.surreal,
        &store::notations::NewNotation::new(
            template.id,
            world.person_id.unwrap(),
            world.project_id.unwrap(),
            "BEGIN",
        ),
    )
    .await
    .unwrap()
    .id;

    // The attorney drafts the will and advances it past `draft` so the
    // client may read it.
    let doc_id = review_documents::create(
        &journey.surreal,
        &NewReviewDocument {
            notation_id,
            kind: "will",
            title: "Last Will and Testament of Capricorn",
            body_html: "<h2>Article I</h2><p>I, Capricorn, declare this my will.</p>",
        },
    )
    .await
    .unwrap();
    review_documents::set_status(
        &journey.surreal,
        doc_id,
        review_documents::STATUS_PENDING_REVIEW,
    )
    .await
    .unwrap();
    world.doc_id = Some(doc_id);
}

async fn estate_template_id(_journey: &Journey) -> Uuid {
    store::templates::resolve(
        &features::shared_surreal().await,
        None,
        "onboarding__estate",
    )
    .await
    .unwrap()
    .expect("onboarding__estate seeded")
    .id
}

#[then("Capricorn can read the will on the review surface")]
async fn assert_read(world: &mut NorthstarWorld) {
    let resp = world
        .journey()
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(world.review_path())
                .header("cookie", world.cookie.clone().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "client should see the draft");
    let body = body_string(resp).await;
    assert!(
        body.contains("Last Will and Testament of Capricorn"),
        "the review page should render the drafted will",
    );
}

#[when("Capricorn leaves a comment on the draft")]
async fn leave_comment(world: &mut NorthstarWorld) {
    let body = format!(
        "_csrf={}&anchor_start=3&anchor_end=12&quoted_text=Capricorn&body=Please+use+my+full+legal+name",
        world.csrf.clone().unwrap(),
    );
    let resp = world
        .journey()
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{}/comments", world.review_path()))
                .header("cookie", world.cookie.clone().unwrap())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "comment POST should succeed");
}

#[then("the comment is recorded on the draft")]
async fn assert_comment(world: &mut NorthstarWorld) {
    let rows = store::document_comments::for_review_document(
        &world.journey().surreal,
        world.doc_id.unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "exactly one comment recorded");
    assert!(!rows[0].resolved, "a fresh comment is unresolved");
    world.comment_id = Some(rows[0].id);
}

#[when("the attorney resolves the comment")]
async fn resolve_comment(world: &mut NorthstarWorld) {
    store::document_comments::set_resolved(
        &world.journey().surreal,
        world.comment_id.unwrap(),
        true,
    )
    .await
    .unwrap()
    .expect("comment exists");
}

#[then("the comment is resolved")]
async fn assert_resolved(world: &mut NorthstarWorld) {
    let rows = store::document_comments::for_review_document(
        &world.journey().surreal,
        world.doc_id.unwrap(),
    )
    .await
    .unwrap();
    assert!(rows[0].resolved, "the comment should be resolved");
}

#[when("the firm signs the closing letter to close the matter")]
async fn close_matter(world: &mut NorthstarWorld) {
    // Open the closing-letter walk for the matter, then walk it to END —
    // the firm signature closes the matter. It raises nothing.
    let resp = world
        .journey()
        .lawyer_post(
            &format!(
                "/app/projects/{}/close",
                world.project_code.as_deref().unwrap()
            ),
            String::new(),
        )
        .await;
    let location = resp
        .location
        .unwrap_or_else(|| panic!("close did not redirect ({})", resp.status));
    let closing_step = location; // /lawyer/notations/<id>/step
    for value in [
        "Capricorn",
        "Estate plan",
        "Wound up the estate plan",
        "paid_in_full",
        "Returned on request, kept 7 years",
        "None",
    ] {
        world
            .journey()
            .lawyer_post(&closing_step, features::journey::answer_body(value))
            .await;
    }
}

#[then("the matter is closed")]
async fn assert_closed(world: &mut NorthstarWorld) {
    let project = store::projects::find_by_id(&world.journey().surreal, world.project_id.unwrap())
        .await
        .unwrap()
        .expect("project exists");
    assert_eq!(project.status, "closed", "the matter should be closed");
}

#[then("the accounting seam was never called")]
async fn assert_nothing_billed(world: &mut NorthstarWorld) {
    let journey = world.journey();
    assert!(
        journey.billing.contact_calls().is_empty(),
        "no Xero contact should be resolved, got {:?}",
        journey.billing.contact_calls()
    );
    assert!(
        journey.billing.calls().is_empty(),
        "no invoice should be raised, got {:?}",
        journey.billing.calls()
    );
    assert!(
        store::xero_invoices::for_projects(&journey.surreal, &[world.project_id.unwrap()])
            .await
            .unwrap()
            .is_empty(),
        "no invoice mirror row should be written"
    );
}

#[tokio::main]
async fn main() {
    NorthstarWorld::cucumber()
        .run_and_exit("tests/features/northstar_estate.feature")
        .await;
}
