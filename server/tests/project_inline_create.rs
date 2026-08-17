#![allow(clippy::doc_markdown)]
//! Dev e2e for the inline entity/client creation on the lawyer `Add project`
//! form (`POST /app/projects/new/{entity,client}`).
//!
//! Drives the real HTTP path against an in-memory app. Both endpoints were
//! HTMX-swapped Bootstrap modals; they are now native form posts on the Dioxus
//! matter-open page, answered post/redirect/get. Each endpoint:
//!
//!   1. **Create** — a valid submit inserts the record and redirects (303) back
//!      to `/app/projects/new?entity=<id>` / `?client=<id>`, so the picker
//!      re-renders with the new record selected, and the row lands in the DB (a
//!      `client`-role person for the client form).
//!   2. **Validation error** — a blank/invalid submit redirects back with
//!      `?entity_error=` / `?client_error=` plus the submitted values echoed,
//!      and creates nothing.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use portal::AppState;
use store::test_support::mem_surreal;
use tower::ServiceExt;

fn admin_bearer() -> String {
    let sessions = portal::SessionStore::new(portal::test_support::TEST_SESSION_KEY);
    let mut session = portal::SessionData::fresh("admin@neonlaw.com", store::persons::Role::Admin);
    session.source = portal::session::SessionSource::Cli;
    format!("Bearer {}", sessions.encode(&session))
}

async fn build_app() -> (axum::Router, store::surreal::SurrealDb) {
    let surreal = mem_surreal().await;
    // Ensure a type + jurisdiction exist for the entity form's pickers.
    store::test_support::seed_entity(&surreal).await;
    let state = AppState {
        ..portal::test_support::app_state(surreal.clone()).await
    };
    (
        server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR)),
        surreal,
    )
}

/// The `Location` a redirect answered with.
fn location(resp: &axum::http::Response<Body>) -> String {
    resp.headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn first_type_id(surreal: &store::surreal::SurrealDb) -> uuid::Uuid {
    // The reference table lives in SurrealDB; the inline form needs a
    // real row because the entity command reads it back before writing.
    store::entity_types::find_or_create(surreal, "Test Org Type")
        .await
        .expect("a seeded entity type")
        .id
}

async fn first_jurisdiction_id(surreal: &store::surreal::SurrealDb) -> uuid::Uuid {
    // The reference table lives in SurrealDB; the inline form needs a
    // real row because the entity command reads it back before writing.
    store::jurisdictions::find_or_create(
        surreal,
        &store::jurisdictions::NewJurisdiction::new("Test State", "TS", "state"),
    )
    .await
    .expect("a seeded jurisdiction")
    .id
}

async fn post(app: &axum::Router, uri: &str, body: String) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", admin_bearer())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn entity_create_redirects_back_with_the_new_entity_selected() {
    let (app, surreal) = build_app().await;
    let type_id = first_type_id(&surreal).await;
    let jur_id = first_jurisdiction_id(&surreal).await;

    let body =
        format!("entity_name=Beta%20Holdings&entity_type_id={type_id}&jurisdiction_id={jur_id}");
    let resp = post(&app, "/app/projects/new/entity", body).await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let created = store::entities::find_by_name(&surreal, "Beta Holdings")
        .await
        .unwrap()
        .expect("the entity was created");
    // The out-of-band `<select>` swap became a redirect naming the new record;
    // the matter-open page preselects it in the Entity picker.
    assert_eq!(
        location(&resp),
        format!("/app/projects/new?entity={}", created.id),
    );
}

#[tokio::test]
async fn entity_blank_name_redirects_back_with_the_error_and_creates_nothing() {
    let (app, surreal) = build_app().await;
    let type_id = first_type_id(&surreal).await;
    let jur_id = first_jurisdiction_id(&surreal).await;
    let before = store::entities::all(&surreal).await.unwrap().len();

    let body = format!("entity_name=&entity_type_id={type_id}&jurisdiction_id={jur_id}");
    let resp = post(&app, "/app/projects/new/entity", body).await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = location(&resp);
    assert!(
        loc.starts_with("/app/projects/new?entity_error=Name%20is%20required."),
        "expected the error flash, got: {loc}",
    );
    // The submitted pickers come back so the disclosure re-opens over them.
    assert!(loc.contains(&format!("entity_type_id={type_id}")), "{loc}");
    assert!(loc.contains(&format!("jurisdiction_id={jur_id}")), "{loc}");
    let after = store::entities::all(&surreal).await.unwrap().len();
    assert_eq!(before, after, "no entity should be created on a blank name");
}

#[tokio::test]
async fn client_create_redirects_back_with_the_new_client_selected() {
    let (app, surreal) = build_app().await;

    let body = "client_name=Libra&client_email=libra-inline%40example.com".to_string();
    let resp = post(&app, "/app/projects/new/client", body).await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let created = store::persons::find_by_email_ci(&surreal, "libra-inline@example.com")
        .await
        .unwrap()
        .expect("the client person was created");
    // The inline client form always mints a `client`-role person — the matter's
    // client-side DRI, never a lawyer/admin.
    assert_eq!(created.role, store::persons::Role::Client);
    assert_eq!(
        location(&resp),
        format!("/app/projects/new?client={}", created.id),
    );
}

#[tokio::test]
async fn client_bad_email_redirects_back_with_the_error_and_creates_nothing() {
    let (app, surreal) = build_app().await;
    let before = store::persons::list_directory(&surreal, "", "", &[])
        .await
        .unwrap()
        .len();

    // No `@` → the shared People command validation rejects it.
    let body = "client_name=Libra&client_email=not-an-email".to_string();
    let resp = post(&app, "/app/projects/new/client", body).await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = location(&resp);
    assert!(loc.contains("client_error="), "{loc}");
    assert!(loc.contains("must%20contain%20an%20@."), "{loc}");
    // The typed values ride back so nothing is retyped.
    assert!(loc.contains("client_name=Libra"), "{loc}");
    assert!(loc.contains("client_email=not-an-email"), "{loc}");
    let after = store::persons::list_directory(&surreal, "", "", &[])
        .await
        .unwrap()
        .len();
    assert_eq!(
        before, after,
        "no person should be created on an invalid email"
    );
}
