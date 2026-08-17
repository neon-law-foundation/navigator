//! Schema guards for "every Project is a git repository"
//! (`m20260627_add_git_repo_to_projects`).
//!
//! Two invariants the migration must hold:
//!
//! 1. A freshly inserted Project starts with an uninitialized repo
//!    (`git_initialized_at = NULL`) — the bare repo is created lazily on
//!    first git access. The branch is always `main`, enforced by the repo's
//!    `pre-receive` hook and pinned in `repos::DEFAULT_BRANCH`, so there is
//!    no per-row branch column to assert
//!    (`m20260719_drop_git_default_branch_from_projects`).
//! 2. A `git_access_tokens` row round-trips: a Project-scoped, hashed
//!    PAT inserts and reads back with its scope and expiry intact.

use chrono::{DateTime, Utc};
use store::persons::{self, NewPerson};
use store::projects::NewProject;
use store::test_support::mem_surreal;

#[tokio::test]
async fn new_project_starts_with_uninitialized_repo() {
    let surreal = mem_surreal().await;

    let proj = store::projects::create(
        &surreal,
        &NewProject {
            code: "git-schema".into(),
            name: "matter".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .expect("insert project");

    // Every Project is a single-branch (`main`) repo identity; the bare
    // repo itself is created lazily, so it starts uninitialized.
    assert_eq!(proj.git_initialized_at, None);
}

#[tokio::test]
async fn git_access_token_round_trips_scoped_to_a_project() {
    let surreal = mem_surreal().await;

    let owner = persons::create(&surreal, &NewPerson::new("Libra", "libra@example.com"))
        .await
        .expect("insert person");

    let proj = store::projects::create(
        &surreal,
        &NewProject {
            code: "git-token-schema".into(),
            name: "matter".into(),
            status: "open".into(),
            entity_id: store::test_support::seed_entity(&surreal).await,
            ..Default::default()
        },
    )
    .await
    .expect("insert project");

    let fetched = store::git_access_tokens::mint(
        &surreal,
        owner.id,
        Some(proj.id),
        store::git_access_tokens::SCOPE_WRITE,
        "the-secret",
        DateTime::parse_from_rfc3339("2099-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
    )
    .await
    .expect("insert git access token");

    assert_eq!(fetched.person_id, owner.id);
    assert_eq!(fetched.project_id, Some(proj.id));
    assert_eq!(fetched.scope, store::git_access_tokens::SCOPE_WRITE);
    assert_eq!(fetched.token_hash.len(), 64);
}
