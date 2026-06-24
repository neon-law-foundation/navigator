#![allow(clippy::doc_markdown)]
//! Matter-lifecycle warning flags on the admin Projects list.
//!
//! The firm's lifecycle invariant: every matter opens on an onboarding
//! (`onboarding__*`) notation — the client's retainer — and a *closed*
//! matter carries a `closing__letter`. Neither is schema-enforced, so the
//! Projects list surfaces the gaps with a warning badge. These tests pin
//! both the pure rule (`web::admin::matter_flags`) and the rendered list.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity::person::Role;
use store::{entity, seed};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};
use workflows::{InMemoryRuntime, StateMachineRuntime};

const SESSION_KEY: &str = "test-session-key-not-for-production";

// ---- the pure rule ----

#[test]
fn flags_an_open_matter_with_no_onboarding_as_missing_retainer() {
    assert_eq!(
        web::admin::matter_flags(false, "open", false),
        (true, false)
    );
}

#[test]
fn a_matter_with_an_onboarding_notation_is_not_missing_its_retainer() {
    assert_eq!(
        web::admin::matter_flags(true, "open", false),
        (false, false)
    );
}

#[test]
fn flags_a_closed_matter_with_no_closing_letter() {
    // Has its retainer, but closed without a closing letter.
    assert_eq!(
        web::admin::matter_flags(true, "closed", false),
        (false, true)
    );
}

#[test]
fn a_closed_matter_with_a_closing_letter_is_clean() {
    assert_eq!(
        web::admin::matter_flags(true, "closed", true),
        (false, false)
    );
}

#[test]
fn an_open_matter_never_owes_a_closing_letter() {
    // No closing letter on an open matter is fine — it is only owed at close.
    assert_eq!(
        web::admin::matter_flags(true, "open", false),
        (false, false)
    );
}

// ---- the rendered list ----

async fn build_app() -> (axum::Router, store::Db) {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-matter-flags-storage"))
            .await
            .unwrap(),
    );
    seed::seed_canonical(&db, &storage).await.unwrap();
    let runtime = Arc::new(InMemoryRuntime::new());
    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let workflow_runtime: Arc<dyn StateMachineRuntime> = Arc::new(
        workflows::DispatchingRuntime::new(runtime.clone(), email.clone(), storage.clone()),
    );
    let state = AppState {
        sessions: SessionStore::new(SESSION_KEY),
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    (
        web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR)),
        db,
    )
}

async fn project(db: &store::Db, name: &str, status: &str) -> Uuid {
    let __dri = store::test_support::dri_person(db).await;
    entity::project::ActiveModel {
        name: ActiveValue::Set(name.into()),
        status: ActiveValue::Set(status.into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn notation(db: &store::Db, project_id: Uuid, person_id: Uuid, template_code: &str) {
    use sea_orm::{ColumnTrait, QueryFilter};
    let tmpl = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq(template_code))
        .one(db)
        .await
        .unwrap()
        .expect("template seeded");
    entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person_id),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn projects_list_flags_the_lifecycle_gaps_and_nothing_else() {
    let (app, db) = build_app().await;
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set("Aries".into()),
        email: ActiveValue::Set("aries@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // A: open, has its retainer → clean.
    let a = project(&db, "Has retainer open", "open").await;
    notation(&db, a, person.id, "onboarding__retainer").await;
    // B: open, no onboarding notation → missing retainer.
    project(&db, "Bare open matter", "open").await;
    // C: closed, has its retainer but no closing letter → missing closing letter.
    let c = project(&db, "Closed no letter", "closed").await;
    notation(&db, c, person.id, "onboarding__estate").await;
    // D: closed, has both → clean.
    let d = project(&db, "Closed with letter", "closed").await;
    notation(&db, d, person.id, "onboarding__retainer").await;
    notation(&db, d, person.id, "closing__letter").await;

    // The matters list filters by the caller's role; forge an admin
    // session so every project is visible.
    let admin = SessionData::fresh("admin-sub", Role::Admin);
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}",
        SessionStore::new(SESSION_KEY).encode(&admin)
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/projects")
                .header("authorization", "Bearer dev")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;

    // Scope each check to the matter's own table row (the canonical seed
    // also carries projects without onboarding notations, so a global
    // count would be polluted).
    let row_for = |name: &str| -> String {
        html.split("<tr")
            .find(|frag| frag.contains(name))
            .unwrap_or("")
            .to_string()
    };

    // B — bare open matter — is flagged as missing its retainer only.
    let b = row_for("Bare open matter");
    assert!(
        b.contains("no retainer"),
        "bare matter should flag the retainer gap"
    );
    assert!(
        !b.contains("no closing letter"),
        "an open matter owes no closing letter"
    );

    // C — closed without a letter — is flagged for the closing letter only
    // (it has its onboarding__estate retainer).
    let c = row_for("Closed no letter");
    assert!(
        c.contains("no closing letter"),
        "closed matter should flag the closing-letter gap"
    );
    assert!(
        !c.contains("no retainer"),
        "C has its retainer (onboarding__estate)"
    );

    // A and D are clean — no badge either way.
    let a = row_for("Has retainer open");
    assert!(!a.contains("no retainer") && !a.contains("no closing letter"));
    let d = row_for("Closed with letter");
    assert!(!d.contains("no retainer") && !d.contains("no closing letter"));
}
