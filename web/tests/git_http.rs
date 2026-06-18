//! End-to-end smart-HTTP transport test: a real `git` client clones and
//! pushes against `web` over a live socket.
//!
//! This exercises the whole path — PAT auth (HTTP Basic), the project
//! ACL, lazy bare-repo creation, the ref advertisement, and the
//! upload-pack / receive-pack RPCs — with git's own binary as the
//! client, so the protocol bytes are validated by the reference
//! implementation rather than by us.
//!
//! One test function covers the happy round-trip plus the two rejection
//! paths, because the repo root is a process-global env var
//! (`NAVIGATOR_GIT_REPO_ROOT`) and parallel tests would race on it.

use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;

use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::person::Role;
use store::entity::{git_access_token, person, person_project_role, project};
use store::test_support::pg;
use store::Db;
use uuid::Uuid;
use web::AppState;

async fn state(db: Db) -> AppState {
    AppState {
        storage: std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-git-http-storage"))
                .await
                .unwrap(),
        ),
        ..web::test_support::app_state(db).await
    }
}

async fn a_person(db: &Db, name: &str, email: &str, role: Role) -> Uuid {
    person::ActiveModel {
        name: ActiveValue::Set(name.into()),
        email: ActiveValue::Set(email.into()),
        role: ActiveValue::Set(role),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn a_project(db: &Db) -> Uuid {
    project::ActiveModel {
        name: ActiveValue::Set("Matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(db).await),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn mint(db: &Db, person_id: Uuid, project_id: Option<Uuid>, scope: &str, plaintext: &str) {
    store::git_access_tokens::mint(
        db,
        person_id,
        project_id,
        scope,
        plaintext,
        Utc::now() + Duration::hours(1),
    )
    .await
    .unwrap();
}

/// Run git in `dir`, isolated from the developer's config and never
/// prompting. Returns (success, combined stderr+stdout).
fn git(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Libra")
        .env("GIT_AUTHOR_EMAIL", "libra@example.com")
        .env("GIT_COMMITTER_NAME", "Libra")
        .env("GIT_COMMITTER_EMAIL", "libra@example.com")
        .args(args)
        .output()
        .expect("run git");
    let mut log = String::from_utf8_lossy(&out.stderr).into_owned();
    log.push_str(&String::from_utf8_lossy(&out.stdout));
    (out.status.success(), log)
}

fn repo_url(addr: SocketAddr, pat: &str, project: Uuid) -> String {
    // Username is ignored; the PAT is the password.
    format!("http://git:{pat}@{addr}/projects/{project}.git")
}

// Multi-thread runtime: the test drives a blocking `git` subprocess
// while the spawned `axum::serve` task must keep running on another
// thread. A single-threaded runtime would deadlock — the blocking git
// call would starve the server task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)]
async fn clone_and_push_round_trip_with_pat() {
    let repo_root = tempfile::tempdir().unwrap();
    std::env::set_var("NAVIGATOR_GIT_REPO_ROOT", repo_root.path());
    let public_dir = tempfile::tempdir().unwrap();

    let db = pg().await;

    // An admin (bypasses project-scoping), a project, a write PAT.
    let admin = a_person(&db, "Nick", "nick@neonlaw.com", Role::Admin).await;
    let project = a_project(&db).await;
    mint(
        &db,
        admin,
        None,
        git_access_token::SCOPE_WRITE,
        "admin-write-pat",
    )
    .await;

    // A staff member WITH a participation row + a read PAT: the ACL must
    // grant them read (this exercises participation, not admin-bypass).
    let staff = a_person(&db, "Virgo", "virgo@example.com", Role::Staff).await;
    person_project_role::ActiveModel {
        person_id: ActiveValue::Set(staff),
        project_id: ActiveValue::Set(project),
        participation: ActiveValue::Set("paralegal".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    mint(
        &db,
        staff,
        None,
        git_access_token::SCOPE_READ,
        "staff-read-pat",
    )
    .await;

    // A client with NO participation on the project, plus a read PAT —
    // a valid identity that the ACL must still refuse.
    let outsider = a_person(&db, "Aries", "aries@example.com", Role::Client).await;
    mint(
        &db,
        outsider,
        None,
        git_access_token::SCOPE_READ,
        "outsider-pat",
    )
    .await;

    // Serve on an ephemeral port.
    let app = web::build_router(state(db).await, public_dir.path());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let work = tempfile::tempdir().unwrap();

    // 1. Clone the (lazily created, empty) repo with the write PAT.
    let dir1 = work.path().join("clone1");
    let (ok, log) = git(
        work.path(),
        &[
            "clone",
            &repo_url(addr, "admin-write-pat", project),
            dir1.to_str().unwrap(),
        ],
    );
    assert!(ok, "clone with valid PAT must succeed:\n{log}");

    // 2. Commit a document and push it to main.
    std::fs::write(dir1.join("will.txt"), "the last will").unwrap();
    assert!(git(&dir1, &["add", "will.txt"]).0);
    assert!(git(&dir1, &["commit", "-m", "add will"]).0);
    // The empty-repo clone's local branch is the client's default
    // (`master` here); push it explicitly onto the repo's single `main`.
    let (ok, log) = git(&dir1, &["push", "origin", "HEAD:main"]);
    assert!(ok, "push to main must succeed:\n{log}");

    // 3. A fresh clone sees the pushed document — the repo persisted it.
    let dir2 = work.path().join("clone2");
    let (ok, log) = git(
        work.path(),
        &[
            "clone",
            &repo_url(addr, "admin-write-pat", project),
            dir2.to_str().unwrap(),
        ],
    );
    assert!(ok, "re-clone must succeed:\n{log}");
    assert_eq!(
        std::fs::read_to_string(dir2.join("will.txt")).unwrap(),
        "the last will"
    );

    // 3b. The staff participant (read PAT, not admin) can clone.
    let (ok, log) = git(
        work.path(),
        &[
            "clone",
            &repo_url(addr, "staff-read-pat", project),
            work.path().join("clone-staff").to_str().unwrap(),
        ],
    );
    assert!(ok, "a participant with a read PAT must clone:\n{log}");

    // 4. A bad PAT is rejected (401 → git clone fails).
    let (ok, _) = git(
        work.path(),
        &[
            "clone",
            &repo_url(addr, "not-a-real-pat", project),
            work.path().join("clone-bad").to_str().unwrap(),
        ],
    );
    assert!(!ok, "clone with an invalid PAT must fail");

    // 5. A valid identity with no participation is refused (403).
    let (ok, _) = git(
        work.path(),
        &[
            "clone",
            &repo_url(addr, "outsider-pat", project),
            work.path().join("clone-outsider").to_str().unwrap(),
        ],
    );
    assert!(!ok, "clone by a non-participant must be refused");

    std::env::remove_var("NAVIGATOR_GIT_REPO_ROOT");
}
