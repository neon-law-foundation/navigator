//! Integration test: the matter's git clone URL on `GET /portal/projects/:id`.
//!
//! The per-Project git repo is internal staff workspace — the append-only
//! document system of record (`docs/git-project-repos.md`). Staff and admin
//! reach the admin matter page and see the clone URL so they can clone the
//! repo with a PAT; a client reaches the portal view and must **never** see
//! the clone URL (the client sees a "Documents" view, never the word "git").

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
    /// A staff member disclosed to the matter — sees the admin page.
    staff_cookie: String,
    /// The matter's client — reaches the portal view, never the git URL.
    client_cookie: String,
}

async fn build_fixture() -> Fixture {
    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join("navigator-project-clone-url-test"))
            .await
            .unwrap(),
    );

    let tmpl = template::ActiveModel {
        code: ActiveValue::Set("onboarding__retainer".into()),
        title: ActiveValue::Set("Retainer".into()),
        respondent_type: ActiveValue::Set("person_and_entity".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    let __dri = store::test_support::dri_person(&db).await;
    let client = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        role: ActiveValue::Set(Role::Client),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Libra formation".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(client.id)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(client.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    // Staff disclosed to the matter (a person_project_roles row).
    let staff = person::ActiveModel {
        name: ActiveValue::Set("Staff Member".into()),
        email: ActiveValue::Set("staff@example.com".into()),
        role: ActiveValue::Set(Role::Staff),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    for (pid, participation) in [(staff.id, "staff"), (client.id, "client")] {
        person_project_role::ActiveModel {
            person_id: ActiveValue::Set(pid),
            project_id: ActiveValue::Set(proj.id),
            participation: ActiveValue::Set(participation.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let sessions = SessionStore::new(KEY);
    let mut staff_session = SessionData::fresh("staff-sub", Role::Staff);
    staff_session.person_id = Some(staff.id);
    let staff_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&staff_session));
    let mut client_session = SessionData::fresh("client-sub", Role::Client);
    client_session.person_id = Some(client.id);
    let client_cookie = format!("{SESSION_COOKIE_NAME}={}", sessions.encode(&client_session));

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
        staff_cookie,
        client_cookie,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn staff_sees_the_repository_clone_url() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.staff_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(
        html.contains("Repository"),
        "staff page must have the repo section"
    );
    // The clone URL's stable tail is the smart-HTTP path the git route serves.
    assert!(
        html.contains(&format!("/projects/{}.git", f.project_id)),
        "staff must see the matter's git clone URL"
    );
}

#[tokio::test]
async fn client_never_sees_the_clone_url() {
    let f = build_fixture().await;
    let resp = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}", f.project_id))
                .header("authorization", "Bearer dev")
                .header("cookie", &f.client_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // The client reaches their *rendered* portal view of the matter — a 200,
    // not a redirect or 404. Assert that first, and that the page actually
    // renders the matter it names, so the `.git`-absence check below is proven
    // against a real portal page and can't pass vacuously on an empty body.
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "client must reach their rendered portal view of the matter"
    );
    let html = body_string(resp).await;
    assert!(
        html.contains("Libra formation"),
        "the client portal view must render the matter it names"
    );
    // The git clone URL must not appear anywhere in that rendered view.
    assert!(
        !html.contains(".git"),
        "the client portal view must never expose the git clone URL"
    );
}
