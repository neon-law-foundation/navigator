#![allow(clippy::doc_markdown, clippy::too_many_lines)]
//! Integration test for the Northstar estate admin matter page (seam 2).
//!
//! `GET /portal/projects/:id` renders the staff matter page. For a
//! transcript-driven estate matter parked at `BEGIN`, that page must
//! carry the phone-friendly transcript-upload form pointing at the
//! shipped handler — but only for staff **disclosed to the matter**
//! (a `person_project_roles` row). A staff member who is not on the
//! matter gets `404`, never a peek: the matter does not exist for them.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::person::Role;
use store::entity::{notation, person, person_project_role, project, template};
use tower::ServiceExt;
use uuid::Uuid;
use web::session::{SessionData, SESSION_COOKIE_NAME};
use web::{AppState, SessionStore};

const KEY: &str = "test-session-key-not-for-production";

struct Fixture {
    app: axum::Router,
    project_id: Uuid,
    notation_id: Uuid,
    /// A staff member disclosed to the matter (has a person_project_roles
    /// row) — sees the matter page and the transcript form.
    disclosed_cookie: String,
    /// A staff member NOT on the matter — gets 404.
    outsider_cookie: String,
}

async fn build_fixture() -> Fixture {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-estate-admin-ui-test"))
            .await
            .unwrap(),
    );

    let tmpl = template::ActiveModel {
        code: ActiveValue::Set("onboarding__estate".into()),
        title: ActiveValue::Set("Northstar Estate Plan".into()),
        respondent_type: ActiveValue::Set("person".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let client = person::ActiveModel {
        name: ActiveValue::Set("Capricorn".into()),
        email: ActiveValue::Set("capricorn@example.com".into()),
        role: ActiveValue::Set(Role::Client),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Capricorn estate plan".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation_id = notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(client.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;

    // A staff member disclosed to the matter: a person_project_roles row.
    let staff = person::ActiveModel {
        name: ActiveValue::Set("Staff Member".into()),
        email: ActiveValue::Set("staff@example.com".into()),
        role: ActiveValue::Set(Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    person_project_role::ActiveModel {
        person_id: ActiveValue::Set(staff.id),
        project_id: ActiveValue::Set(proj.id),
        participation: ActiveValue::Set("staff".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // A second staff member with NO row on this matter — the outsider.
    let outsider = person::ActiveModel {
        name: ActiveValue::Set("Other Staff".into()),
        email: ActiveValue::Set("other@example.com".into()),
        role: ActiveValue::Set(Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let sessions = SessionStore::new(KEY);
    let mut disclosed = SessionData::fresh("staff-sub", Role::Staff);
    disclosed.person_id = Some(staff.id);
    let disclosed_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&disclosed));
    let mut out = SessionData::fresh("outsider-sub", Role::Staff);
    out.person_id = Some(outsider.id);
    let outsider_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&out));

    let email: Arc<dyn web::email::EmailService> = Arc::new(web::email::CapturingEmail::new());
    let runtime = Arc::new(workflows::InMemoryRuntime::new());
    let state = AppState {
        sessions: SessionStore::new(KEY),
        storage: storage.clone(),
        workflow_runtime: runtime.clone(),
        questionnaire_runtime: runtime,
        email,
        ..web::test_support::app_state(db).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    Fixture {
        app,
        project_id: proj.id,
        notation_id,
        disclosed_cookie,
        outsider_cookie,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn disclosed_staff_sees_the_transcript_upload_form_at_begin() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.disclosed_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Estate plan — Northstar"), "html: {html}");
    assert!(html.contains("File the sitting transcript"));
    assert!(html.contains(&format!(
        "action=\"/portal/projects/{}/notations/{}/transcript\"",
        f.project_id, f.notation_id
    )));
    assert!(html.contains("enctype=\"multipart/form-data\""));
}

#[tokio::test]
async fn staff_not_disclosed_to_the_matter_gets_404() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.outsider_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
