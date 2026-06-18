//! Cucumber runner for `features/git_repo_collaboration.feature`.
//!
//! The git project-repo journey: a document makes the round trip through a
//! Project's append-only repo. It crosses the `repos` engine (commit,
//! HEAD listing, governed-expunge) and the live `web::git_http` smart-HTTP
//! surface, which serves the repo only to a holder of a valid Personal
//! Access Token scoped to the Project (`store::git_access_tokens` +
//! `can_see_project`). The repo root is a per-runner temp dir wired
//! through `NAVIGATOR_GIT_REPO_ROOT`, the same env `web` reads.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use cucumber::{given, then, when, World};
use features::{body_string, journey::client, journey::matter, journey::Journey};
use store::entity::git_access_token::SCOPE_READ;
use tower::ServiceExt;
use uuid::Uuid;

const PAT: &str = "navpat-git-journey-secret-0001";

#[derive(Default, World)]
#[world(init = Self::default)]
struct GitWorld {
    journey: Option<Journey>,
    person_id: Option<Uuid>,
    project_id: Option<Uuid>,
}

impl std::fmt::Debug for GitWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitWorld")
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl GitWorld {
    fn journey(&self) -> &Journey {
        self.journey.as_ref().expect("journey not built")
    }

    fn project_id(&self) -> Uuid {
        self.project_id.expect("project not built")
    }

    fn lists(&self, path: &str) -> bool {
        repo_store()
            .read_head_tree(self.project_id())
            .expect("read head tree")
            .iter()
            .any(|(p, _)| p == path)
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with a matter and a repo access token$"#)]
async fn seed(world: &mut GitWorld, name: String, email: String) {
    // Point `web` (and this runner) at one per-runner temp repo root.
    // Set once for the process; each scenario uses a unique project id, so
    // repos never collide even under cucumber's concurrent scenarios.
    let root = std::env::temp_dir().join("navigator-features-git-journey");
    std::env::set_var(repos::REPO_ROOT_ENV, &root);

    let journey = Journey::open("git-repo").await;
    let person = client(&journey.db, &name, &email).await;
    let project_id = matter(&journey.db, person.id, "Estate matter with a repo").await;
    // A read-scoped PAT for this person over this Project — the credential
    // the client's git client will carry.
    store::git_access_tokens::mint(
        &journey.db,
        person.id,
        Some(project_id),
        SCOPE_READ,
        PAT,
        chrono::Utc::now() + chrono::Duration::days(1),
    )
    .await
    .expect("mint PAT");
    world.person_id = Some(person.id);
    world.project_id = Some(project_id);
    world.journey = Some(journey);
}

#[when(regex = r#"^the firm commits "([^"]+)" to the Project repo$"#)]
async fn commit(world: &mut GitWorld, path: String) {
    let store = repo_store();
    let project_id = world.project_id();
    store.ensure(project_id).expect("ensure repo");
    store
        .commit_as(
            project_id,
            repos::Author {
                name: "Neon Law",
                email: "support@neonlaw.com",
            },
            &format!("Add {path}"),
            &[(
                path.as_str(),
                b"# Draft will\n\nThe estate plan in progress.\n",
            )],
        )
        .expect("commit document");
}

#[then(regex = r#"^"([^"]+)" appears in the Project repo listing$"#)]
async fn assert_listed(world: &mut GitWorld, path: String) {
    assert!(
        world.lists(&path),
        "{path} should be in the repo HEAD listing"
    );
}

#[then(regex = r#"^"([^"]+)" is gone from the Project repo listing$"#)]
async fn assert_not_listed(world: &mut GitWorld, path: String) {
    assert!(
        !world.lists(&path),
        "{path} should have been expunged from the repo HEAD listing",
    );
}

fn repo_store() -> repos::RepoStore {
    repos::RepoStore::from_env().expect("NAVIGATOR_GIT_REPO_ROOT set in build_app")
}

fn info_refs_uri(project_id: Uuid) -> String {
    format!("/projects/{project_id}.git/info/refs?service=git-upload-pack")
}

#[then("the repo refuses an anonymous git fetch")]
async fn assert_anon_refused(world: &mut GitWorld) {
    let resp = world
        .journey()
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(info_refs_uri(world.project_id()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "an anonymous git fetch must be challenged",
    );
}

#[then("the repo serves a git fetch to the token holder")]
async fn assert_pat_fetch(world: &mut GitWorld) {
    // Git sends the PAT as the HTTP Basic password; the username is
    // ignored (GitHub convention).
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("git:{PAT}"));
    let resp = world
        .journey()
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(info_refs_uri(world.project_id()))
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the PAT holder should be served"
    );
    let body = body_string(resp).await;
    assert!(
        body.contains("git-upload-pack"),
        "the smart-HTTP advertisement should name the upload-pack service",
    );
}

#[when(regex = r#"^an admin governed-expunges "([^"]+)" from the repo$"#)]
async fn expunge(world: &mut GitWorld, path: String) {
    let outcome = repo_store()
        .expunge_path(world.project_id(), &path)
        .expect("governed expunge");
    assert_ne!(
        outcome.head_before, outcome.head_after,
        "expunge must rewrite history to a new HEAD",
    );
}

#[tokio::main]
async fn main() {
    GitWorld::cucumber()
        .run("tests/features/git_repo_collaboration.feature")
        .await;
}
