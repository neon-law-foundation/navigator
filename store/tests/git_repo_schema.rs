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

use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity::{git_access_token, person, project};
use store::test_support::pg;
use uuid::Uuid;

#[tokio::test]
async fn new_project_starts_with_uninitialized_repo() {
    let db = pg().await;

    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert project");

    // Every Project is a single-branch (`main`) repo identity; the bare
    // repo itself is created lazily, so it starts uninitialized.
    assert_eq!(proj.git_initialized_at, None);
}

#[tokio::test]
async fn git_access_token_round_trips_scoped_to_a_project() {
    let db = pg().await;

    let owner = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert person");

    let __dri = store::test_support::dri_person(&db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert project");

    let token_id = Uuid::now_v7();
    git_access_token::ActiveModel {
        id: ActiveValue::Set(token_id),
        person_id: ActiveValue::Set(owner.id),
        project_id: ActiveValue::Set(Some(proj.id)),
        token_hash: ActiveValue::Set("0".repeat(64)),
        scope: ActiveValue::Set(git_access_token::SCOPE_WRITE.into()),
        expires_at: ActiveValue::Set("2099-01-01T00:00:00Z".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert git access token");

    let fetched = git_access_token::Entity::find_by_id(token_id)
        .one(&db)
        .await
        .expect("query token")
        .expect("token exists");

    assert_eq!(fetched.person_id, owner.id);
    assert_eq!(fetched.project_id, Some(proj.id));
    assert_eq!(fetched.scope, git_access_token::SCOPE_WRITE);
    assert_eq!(fetched.token_hash.len(), 64);
}
