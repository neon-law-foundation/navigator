//! The single mounted writer's `/git-writer/ensure` endpoint — contract
//! tests (auth, idempotent ensure, absent-on-stateless-pods), plus the
//! #279 covering test: a matter-creation surface with **no**
//! `NAVIGATOR_GIT_REPO_ROOT` still opens a matter when the writer owns
//! the volume and the surface routes provisioning through it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::projects::{EnsureRepoResponse, GIT_WRITER_ENSURE_PATH};
use store::{entity, seed};
use tower::ServiceExt;
use web::git_writer::{routes_with, WriterConfig};
use web::signature::StubSignatureProvider;
use web::AppState;
use workflows::{DispatchingRuntime, InMemoryRuntime, StateMachineRuntime};

fn temp_repo_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "navigator-git-writer-{tag}-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn writer_router(root: &std::path::Path, token: &str) -> axum::Router {
    routes_with::<()>(Some(WriterConfig {
        token: token.into(),
        store: repos::RepoStore::new(root),
    }))
}

fn ensure_request(project_id: uuid::Uuid, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(GIT_WRITER_ENSURE_PATH)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder
        .body(Body::from(format!("{{\"project_id\":\"{project_id}\"}}")))
        .unwrap()
}

#[tokio::test]
async fn ensure_requires_the_writer_bearer() {
    let root = temp_repo_root("auth");
    let app = writer_router(&root, "s3cret");
    let project_id = uuid::Uuid::now_v7();

    let missing = app
        .clone()
        .oneshot(ensure_request(project_id, None))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let wrong = app
        .oneshot(ensure_request(project_id, Some("wrong")))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let store = repos::RepoStore::new(&root);
    assert!(
        !store.exists(project_id),
        "an unauthorized call must not create a repo"
    );
}

#[tokio::test]
async fn ensure_creates_the_bare_repo_and_is_idempotent() {
    let root = temp_repo_root("ensure");
    let app = writer_router(&root, "s3cret");
    let project_id = uuid::Uuid::now_v7();

    for _ in 0..2 {
        let resp = app
            .clone()
            .oneshot(ensure_request(project_id, Some("s3cret")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: EnsureRepoResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(
            std::path::Path::new(&body.path).join("HEAD").is_file(),
            "response path should be the bare repo: {}",
            body.path
        );
    }
}

#[tokio::test]
async fn a_pod_without_the_writer_role_serves_404() {
    // `routes_with(None)` is what the stateless tier mounts.
    let app = routes_with::<()>(None);
    let resp = app
        .oneshot(ensure_request(uuid::Uuid::now_v7(), Some("s3cret")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// #279's covering test: the matter-open form on a `web` process with **no**
/// repo volume (`NAVIGATOR_GIT_REPO_ROOT` unset) still opens the matter,
/// because provisioning routes through the single mounted writer. The row
/// commits with `git_initialized_at` stamped, and the bare repo lands on the
/// *writer's* volume.
///
/// Env vars are process-global, so this is the only test in this binary
/// that touches them; the contract tests above use the explicit-config seam.
#[tokio::test]
async fn matter_opens_through_the_remote_writer_without_a_local_volume() {
    // The writer: its own volume + token, served on an ephemeral port.
    let writer_root = temp_repo_root("remote-writer");
    let writer = writer_router(&writer_root, "t0ken");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let writer_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, writer).await.unwrap();
    });

    // The create surface: no volume, writer wiring only.
    std::env::remove_var("NAVIGATOR_GIT_REPO_ROOT");
    std::env::set_var("NAVIGATOR_GIT_WRITER_URL", &writer_url);
    std::env::set_var("NAVIGATOR_GIT_WRITER_TOKEN", "t0ken");

    let db = store::test_support::pg().await;
    let storage: Arc<dyn cloud::StorageService> = Arc::new(
        cloud::FsStorage::new(std::env::temp_dir().join(format!(
            "navigator-git-writer-storage-{}",
            uuid::Uuid::now_v7()
        )))
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
    let state = AppState {
        storage,
        workflow_runtime,
        questionnaire_runtime: runtime,
        signature_provider: Arc::new(StubSignatureProvider::new()),
        email,
        ..web::test_support::app_state(db.clone()).await
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let entity_id = store::test_support::seed_entity(&db).await;
    let client_id = {
        use sea_orm::{ActiveModelTrait, ActiveValue};
        entity::person::ActiveModel {
            name: ActiveValue::Set("Libra Client".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            role: ActiveValue::Set(entity::person::Role::Client),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id
    };

    let body = format!(
        "name=Remote%20writer%20matter&status=open&entity_id={entity_id}\
         &client_dri_person_id={client_id}\
         &retainer_template_code=onboarding__retainer\
         &scope_of_services=Flat-fee%20estate%20planning",
    );
    let resp = app
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
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "the matter must open even though this process has no repo volume"
    );

    let project = entity::project::Entity::find()
        .filter(entity::project::Column::Name.eq("Remote writer matter"))
        .one(&db)
        .await
        .unwrap()
        .expect("project row committed");
    assert!(
        project.git_initialized_at.is_some(),
        "the caller-side transaction stamps git_initialized_at"
    );

    // The bare repo landed on the WRITER's volume.
    let writer_store = repos::RepoStore::new(&writer_root);
    assert!(
        writer_store.exists(project.id),
        "the bare repo must exist on the single mounted writer's volume"
    );
}
