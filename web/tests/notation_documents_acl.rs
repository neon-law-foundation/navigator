#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Commit 2: notation-PDF access is gated by **project participation**,
//! not notation ownership, and the project page surfaces each notation's
//! three PDFs (rendered / signed / certificate) by plain name.
//!
//! Proves:
//!   - a project *participant* who is not the notation owner can download
//!     the rendered + signed PDFs (200 — `FsStorage` streams through);
//!   - a non-participant gets 404 (no leakage, not 403);
//!   - admin bypasses;
//!   - the client project page lists the notation under "Your agreements"
//!     with a working signed-copy link.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity::person::Role;
use store::entity::{notation, person, person_project_role, project, template};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    db: store::Db,
    storage: Arc<dyn cloud::StorageService>,
    sessions: SessionStore,
}

async fn build() -> Fixture {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(
            std::env::temp_dir().join(format!("navigator-doc-acl-{}", Uuid::now_v7())),
        )
        .await
        .unwrap(),
    );
    store::seed::seed_canonical(&db, &storage)
        .await
        .expect("canonical seed");

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let inner = Arc::new(workflows::InMemoryRuntime::new());
    let workflow_runtime: Arc<dyn workflows::StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(inner.clone(), email.clone(), storage.clone())
            .with_db(db.clone()),
    );
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime,
        questionnaire_runtime: inner,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    Fixture {
        app: web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
        storage,
        sessions: SessionStore::new(KEY),
    }
}

fn cookie_for(sessions: &SessionStore, role: Role, person_id: Option<Uuid>) -> String {
    let mut s = SessionData::fresh("sub", role);
    s.person_id = person_id;
    format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&s))
}

async fn mk_person(db: &store::Db, email: &str, role: Role) -> Uuid {
    person::ActiveModel {
        name: ActiveValue::Set(email.into()),
        email: ActiveValue::Set(email.into()),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn get(app: &axum::Router, uri: &str, cookie: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn notation_pdfs_are_gated_by_project_participation_and_listed_on_the_project() {
    let f = build().await;

    // A retainer template to hang the notation off (any seeded onboarding
    // template works; pick the retainer by code).
    let tmpl = template::Entity::find()
        .one(&f.db)
        .await
        .unwrap()
        .expect("at least one seeded template");

    // The notation owner, a co-client participant, and an outsider.
    let owner = mk_person(&f.db, "owner@example.com", Role::Client).await;
    let spouse = mk_person(&f.db, "spouse@example.com", Role::Client).await;
    let outsider = mk_person(&f.db, "outsider@example.com", Role::Client).await;

    let project_id = project::ActiveModel {
        name: ActiveValue::Set("Joint estate".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&f.db).await),
        ..Default::default()
    }
    .insert(&f.db)
    .await
    .unwrap()
    .id;

    // Both owner and spouse participate; the outsider does not.
    for pid in [owner, spouse] {
        person_project_role::ActiveModel {
            person_id: ActiveValue::Set(pid),
            project_id: ActiveValue::Set(project_id),
            participation: ActiveValue::Set("client".into()),
            ..Default::default()
        }
        .insert(&f.db)
        .await
        .unwrap();
    }

    let notation_id = notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(owner),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set("END".into()),
        ..Default::default()
    }
    .insert(&f.db)
    .await
    .unwrap()
    .id;

    // Materialize the rendered + signed PDFs in storage.
    for key in [
        web::retainer_walk::document_pdf_storage_key(notation_id),
        web::retainer_walk::signed_document_storage_key(notation_id),
    ] {
        f.storage
            .put(&key, b"%PDF-1.7 fake", "application/pdf")
            .await
            .unwrap();
    }

    let doc_uri = format!("/portal/admin/notations/{notation_id}/documents/retainer");
    let signed_uri = format!("/portal/admin/notations/{notation_id}/documents/signed");

    // (1) The spouse (participant, NOT the owner) can download both PDFs.
    let spouse_cookie = cookie_for(&f.sessions, Role::Client, Some(spouse));
    assert_eq!(
        get(&f.app, &doc_uri, &spouse_cookie).await.status(),
        StatusCode::OK,
        "a co-client participant can fetch the rendered PDF",
    );
    assert_eq!(
        get(&f.app, &signed_uri, &spouse_cookie).await.status(),
        StatusCode::OK,
        "a co-client participant can fetch the signed PDF",
    );

    // (2) The outsider (no participation) gets 404 — not 403, no leakage.
    let outsider_cookie = cookie_for(&f.sessions, Role::Client, Some(outsider));
    assert_eq!(
        get(&f.app, &doc_uri, &outsider_cookie).await.status(),
        StatusCode::NOT_FOUND,
        "a non-participant must get 404, not the document",
    );

    // (3) Admin bypasses participation.
    let admin_cookie = cookie_for(&f.sessions, Role::Admin, None);
    assert_eq!(
        get(&f.app, &doc_uri, &admin_cookie).await.status(),
        StatusCode::OK,
        "admin bypasses project scoping",
    );

    // (4) The client project page lists the notation under "Your
    // agreements" with a working signed-copy link.
    let page = get(
        &f.app,
        &format!("/portal/projects/{project_id}"),
        &spouse_cookie,
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let html = String::from_utf8(
        page.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        html.contains("Your agreements"),
        "agreements section missing"
    );
    assert!(
        html.contains(&format!(
            "/portal/admin/notations/{notation_id}/documents/signed"
        )),
        "signed-copy download link missing from the project page",
    );
}
