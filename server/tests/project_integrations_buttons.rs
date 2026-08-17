//! Integration test: the "Integrations" section on `GET /app/projects/:id`
//! — the internal/external Slack channel buttons and the Xero button for a
//! matter's raised invoice.
//!
//! These are lawyer-only, same lens as the application repository links in
//! `project_repo_clone_url.rs`: a client reaches the portal view and must
//! never see them.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::session::{SessionData, SESSION_COOKIE_NAME};
use portal::{AppState, SessionStore};
use store::persons::Role;
use store::test_support::mem_surreal;
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    surreal: store::surreal::SurrealDb,
    project_id: Uuid,
    lawyer_cookie: String,
    client_cookie: String,
}

async fn build_fixture() -> Fixture {
    let surreal = mem_surreal().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-project-integrations-test"))
            .await
            .unwrap(),
    );

    let client = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Libra", "libra@example.com", Role::Client),
    )
    .await
    .unwrap();
    let proj = store::projects::create(
        &surreal,
        &store::projects::NewProject {
            code: format!("libra-integrations-{}", Uuid::now_v7()),
            name: "Libra integrations".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let lawyer = store::persons::create(
        &surreal,
        &store::persons::NewPerson::with_role("Lawyer Member", "lawyer@example.com", Role::Lawyer),
    )
    .await
    .unwrap();
    for (pid, participation) in [(lawyer.id, "lawyer"), (client.id, "client")] {
        store::projects::add_participation(&surreal, proj.id, pid, participation)
            .await
            .unwrap();
    }

    let sessions = SessionStore::new(KEY);
    let mut lawyer_session = SessionData::fresh("lawyer-sub", Role::Lawyer);
    lawyer_session.person_id = Some(lawyer.id);
    let lawyer_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&lawyer_session));
    let mut client_session = SessionData::fresh("client-sub", Role::Client);
    client_session.person_id = Some(client.id);
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&client_session));

    let email: Arc<dyn portal::email::EmailService> =
        Arc::new(portal::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..portal::test_support::app_state(surreal.clone()).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        surreal,
        project_id: proj.id,
        lawyer_cookie,
        client_cookie,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn get_as(app: &axum::Router, project_id: Uuid, cookie: &str) -> String {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/app/projects/{project_id}"))
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_string(resp).await
}

/// A matter with neither Slack link set and no raised invoice shows no
/// "Integrations" section at all — nothing to point at, so no empty shell.
#[tokio::test]
async fn a_matter_with_no_integrations_set_has_no_integrations_section() {
    let f = build_fixture().await;
    let html = get_as(&f.app, f.project_id, &f.lawyer_cookie).await;
    assert!(!html.contains("Integrations"));
}

/// The internal Slack channel always renders when set; the external one is
/// genuinely optional and only appears when the matter has one.
#[tokio::test]
async fn lawyer_sees_the_internal_slack_button_and_the_optional_external_one() {
    const INTERNAL: &str = "https://neonlaw.slack.com/archives/C0INTERNAL";
    let f = build_fixture().await;
    store::projects::update_project(
        &f.surreal,
        f.project_id,
        &store::projects::UpdateProjectCommand {
            name: "Libra integrations".into(),
            entity_id: None,
            description: None,
            internal_slack_channel_url: Some(INTERNAL.into()),
            external_slack_channel_url: None,
            repository_url: None,
        },
    )
    .await
    .unwrap();

    let html = get_as(&f.app, f.project_id, &f.lawyer_cookie).await;
    assert!(html.contains("Integrations"));
    assert!(html.contains(INTERNAL));
    assert!(html.contains("Internal Slack channel"));
    assert!(
        !html.contains("External Slack channel"),
        "no external channel was set"
    );
}

#[tokio::test]
async fn lawyer_sees_the_external_slack_button_when_the_matter_has_one() {
    const EXTERNAL: &str = "https://neonlaw.slack.com/archives/C0EXTERNAL";
    let f = build_fixture().await;
    store::projects::update_project(
        &f.surreal,
        f.project_id,
        &store::projects::UpdateProjectCommand {
            name: "Libra integrations".into(),
            entity_id: None,
            description: None,
            internal_slack_channel_url: None,
            external_slack_channel_url: Some(EXTERNAL.into()),
            repository_url: None,
        },
    )
    .await
    .unwrap();

    let html = get_as(&f.app, f.project_id, &f.lawyer_cookie).await;
    assert!(html.contains(EXTERNAL));
    assert!(html.contains("External Slack channel"));
}

/// The Xero button links straight to the matter's raised invoice, mirrored
/// locally in `xero_invoice` and keyed uniquely on `project_id` — the
/// invoice is already grouped per matter, so the button needs no fan-out.
#[tokio::test]
async fn lawyer_sees_the_xero_button_pointing_at_the_raised_invoice() {
    const XERO_ID: &str = "11111111-2222-3333-4444-555555555555";
    let f = build_fixture().await;
    store::xero_invoices::upsert(
        &f.surreal,
        &store::xero_invoices::UpsertXeroInvoice {
            project_id: f.project_id,
            xero_invoice_id: XERO_ID.into(),
            reference: format!("Matter {}", f.project_id),
            status: "AUTHORISED".into(),
            amount_cents: 50_000,
            currency: "USD".into(),
        },
    )
    .await
    .unwrap();

    let html = get_as(&f.app, f.project_id, &f.lawyer_cookie).await;
    assert!(html.contains(&format!(
        "https://go.xero.com/AccountsReceivable/View.aspx?InvoiceID={XERO_ID}"
    )));
    assert!(html.contains(">Xero<"));
}

/// A client never sees any of the three lawyer-only integration buttons, even
/// when every one of them is present on the matter.
#[tokio::test]
async fn client_never_sees_any_integration_button() {
    let f = build_fixture().await;
    store::projects::update_project(
        &f.surreal,
        f.project_id,
        &store::projects::UpdateProjectCommand {
            name: "Libra integrations".into(),
            entity_id: None,
            description: None,
            internal_slack_channel_url: Some(
                "https://neonlaw.slack.com/archives/C0INTERNAL".into(),
            ),
            external_slack_channel_url: Some(
                "https://neonlaw.slack.com/archives/C0EXTERNAL".into(),
            ),
            repository_url: None,
        },
    )
    .await
    .unwrap();
    store::xero_invoices::upsert(
        &f.surreal,
        &store::xero_invoices::UpsertXeroInvoice {
            project_id: f.project_id,
            xero_invoice_id: "11111111-2222-3333-4444-555555555555".into(),
            reference: format!("Matter {}", f.project_id),
            status: "AUTHORISED".into(),
            amount_cents: 50_000,
            currency: "USD".into(),
        },
    )
    .await
    .unwrap();

    let html = get_as(&f.app, f.project_id, &f.client_cookie).await;
    assert!(
        html.contains("Libra integrations"),
        "renders the client view"
    );
    assert!(!html.contains("slack.com"));
    assert!(!html.contains("go.xero.com"));
    assert!(!html.contains("Integrations"));
}
