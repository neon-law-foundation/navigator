#![allow(clippy::doc_markdown)]
//! Server-side e2e for the `navigator` CLI's bearer path.
//!
//! Proves the CLI drives the **existing** matter-open routes over an
//! `Authorization: Bearer <SessionData>` credential — the same blob the
//! browser cookie carries — instead of the test-only `Bearer dev`
//! pass-through. CI-safe: the `StubSignatureProvider` records the send,
//! so nothing reaches DocuSign.
//!
//! Covers:
//!   1. A real minted `SessionData` bearer opens a matter with the
//!      retainer block and parks at `staff_review`. `approve-send` renders
//!      then parks the PDF at `document_open__retainer_pdf` (no envelope
//!      yet); the separate `send` then dispatches exactly one envelope. A
//!      `send` attempted before the PDF is rendered returns `409`.
//!   2. `GET /portal/admin/notations/:id/review?format=json` returns the
//!      workflow state, signature request id, and `document_ready` (the
//!      `notation status` command's contract).
//!   3. An **expired** session bearer is rejected (the matter-open POST
//!      does not create a project).
//!   4. `GET /auth/cli/whoami` echoes the bearer caller's identity.
//!   5. `GET /auth/cli/start` refuses a non-loopback `redirect`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::entity::person::Role;
use store::{entity, seed};
use tower::ServiceExt;
use web::session::{now_unix_secs, SessionData};
use web::signature::StubSignatureProvider;
use web::{AppState, AuthConfig, SessionStore};
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

const SESSION_KEY: &str = "cli-bearer-test-key-not-for-production";

async fn build_app(tag: &str) -> (axum::Router, store::Db, Arc<StubSignatureProvider>) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!("navigator-cli-bearer-{tag}")))
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
        // Auth ENFORCED via HS256 so the Bearer path is exercised for
        // real: a session blob must reach the handler through
        // `inject_bearer_session`, not through a disabled pass-through.
        auth: AuthConfig::new(false, Some("unused-hs256-secret")),
        sessions: SessionStore::new(SESSION_KEY),
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

/// A fresh admin session bearer, minted exactly as `/auth/cli/start`
/// would, signed with the test session key.
fn admin_bearer() -> String {
    let mut session = SessionData::fresh("cli-admin", Role::Admin);
    session.email = Some("nick@neonlaw.com".into());
    SessionStore::new(SESSION_KEY).encode(&session)
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn cli_bearer_opens_matter_then_approve_parks_and_send_dispatches_once() {
    let (app, db, stub) = build_app("happy").await;
    let bearer = format!("Bearer {}", admin_bearer());
    let entity_id = store::test_support::seed_entity(&db).await;
    // The client is selected from existing clients — seed them first.
    let client_id = seed_client(&db, "Nick Shook", "nick@shook.family").await;

    let body = format!(
        "name={}&status=open&entity_id={entity_id}\
         &client_dri_person_id={client_id}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services={}",
        enc("Shook estate"),
        enc("Flat-fee estate planning"),
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/projects")
                .header("authorization", &bearer)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
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

    // Parked at staff_review — the gate is intact; no envelope yet.
    let notation = entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .expect("retainer notation inserted");
    assert_eq!(notation.state, "staff_review");
    assert_eq!(notation.delivery, "emailed");
    assert!(stub.calls().is_empty());

    // The provenance is attributable: the pre-existing client person is the
    // matter's client-side DRI (now a first-class column on the project, not
    // a `client_dri` participation row).
    let person = entity::person::Entity::find()
        .filter(entity::person::Column::Email.eq("nick@shook.family"))
        .one(&db)
        .await
        .unwrap()
        .expect("client person exists");
    assert_eq!(person.id, client_id);
    assert_eq!(person.name, "Nick Shook");
    let project = entity::project::Entity::find_by_id(notation.project_id)
        .one(&db)
        .await
        .unwrap()
        .expect("project row inserted");
    assert_eq!(project.client_dri_person_id, Some(person.id));

    // A small closure for the repeated bearer POST / GET shapes.
    let get_status = {
        let app = app.clone();
        let bearer = bearer.clone();
        move || {
            let app = app.clone();
            let bearer = bearer.clone();
            async move {
                let resp = app
                    .oneshot(
                        Request::builder()
                            .uri(format!(
                                "/portal/admin/notations/{notation_id}/review?format=json"
                            ))
                            .header("authorization", &bearer)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                let json: serde_json::Value =
                    serde_json::from_str(&body_string(resp).await).unwrap();
                json
            }
        }
    };
    let post = |path: String| {
        let app = app.clone();
        let bearer = bearer.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", &bearer)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    // `notation status` JSON view reflects the parked state: no envelope,
    // and no PDF rendered yet (`document_ready:false`).
    let status_json = get_status().await;
    assert_eq!(status_json["state"], "staff_review");
    assert!(status_json["signature_request_id"].is_null());
    assert_eq!(status_json["document_ready"], false);

    // `send` BEFORE the PDF is rendered → 409 with a JSON reason, and no
    // envelope goes out. The readiness gate is what stops a send against a
    // worker that hasn't (or can't) render.
    let early_send = post(format!("/portal/admin/notations/{notation_id}/send")).await;
    assert_eq!(early_send.status(), StatusCode::CONFLICT);
    let early_json: serde_json::Value =
        serde_json::from_str(&body_string(early_send).await).unwrap();
    assert_eq!(early_json["error"], "document_not_ready");
    assert!(early_json["reason"].is_string());
    assert!(stub.calls().is_empty(), "no envelope before send");

    // Staff approve → renders + parks at document_open__retainer_pdf. The
    // in-process DispatchingRuntime renders + persists the PDF inline, so
    // the workflow waits at the document step with the PDF present — but
    // NO envelope has gone out yet.
    let approve = post(format!(
        "/portal/admin/notations/{notation_id}/approve-send"
    ))
    .await;
    assert_eq!(approve.status(), StatusCode::OK);
    assert!(
        stub.calls().is_empty(),
        "approve renders + parks; it must NOT send"
    );
    let row = entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "document_open__retainer_pdf");
    assert!(row.signature_request_id.is_none());

    // Status now shows the parked-and-rendered state: document_ready:true,
    // still no envelope.
    let parked_json = get_status().await;
    assert_eq!(parked_json["state"], "document_open__retainer_pdf");
    assert_eq!(parked_json["document_ready"], true);
    assert!(parked_json["signature_request_id"].is_null());

    // The deliberate send → exactly one envelope, lands at
    // sent_for_signature__pending.
    let send = post(format!("/portal/admin/notations/{notation_id}/send")).await;
    assert_eq!(send.status(), StatusCode::OK);
    assert_eq!(stub.calls().len(), 1, "exactly one envelope should be sent");
    let row = entity::notation::Entity::find_by_id(notation_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "sent_for_signature__pending");
    assert!(row.signature_request_id.is_some());

    // The status JSON now carries the signature request id.
    let sent_json = get_status().await;
    assert_eq!(sent_json["state"], "sent_for_signature__pending");
    assert!(sent_json["signature_request_id"].is_string());

    // `send` again is idempotent: it reuses the existing envelope, fires
    // no second send.
    let resend = post(format!("/portal/admin/notations/{notation_id}/send")).await;
    assert_eq!(resend.status(), StatusCode::OK);
    assert_eq!(stub.calls().len(), 1, "resend must not double-send");
}

#[tokio::test]
async fn expired_session_bearer_is_rejected_with_no_matter() {
    let (app, db, _stub) = build_app("expired").await;

    let mut session = SessionData::fresh("cli-admin", Role::Admin);
    session.exp = now_unix_secs() - 60; // expired a minute ago
    let token = SessionStore::new(SESSION_KEY).encode(&session);

    let body = format!("name={}&status=open", enc("Expired matter"));
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/projects")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // The expired blob never resolves to a session; with auth enforced
    // and no AuthClaims injected, require_auth rejects it.
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        entity::project::Entity::find()
            .filter(entity::project::Column::Name.eq("Expired matter"))
            .one(&db)
            .await
            .unwrap()
            .is_none(),
        "an expired token must not open a matter",
    );
}

#[tokio::test]
async fn whoami_echoes_the_bearer_identity() {
    let (app, _db, _stub) = build_app("whoami").await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/auth/cli/whoami")
                .header("authorization", format!("Bearer {}", admin_bearer()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(json["email"], "nick@neonlaw.com");
    assert_eq!(json["role"], "admin");
    assert!(json["exp"].is_number());
}

#[tokio::test]
async fn whoami_without_a_bearer_is_unauthorized() {
    let (app, _db, _stub) = build_app("whoami-none").await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/auth/cli/whoami")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cli_start_refuses_a_non_loopback_redirect() {
    let (app, _db, _stub) = build_app("redirect-guard").await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/auth/cli/start?redirect=http://evil.example/cb&state=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
