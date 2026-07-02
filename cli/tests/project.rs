//! End-to-end tests for `cli project create`. The subcommand runs
//! migrate + seed against the target Postgres so the canonical
//! `shook.family` entity is in place by the time we look it up. The
//! canonical seed only seeds Nick as ADMIN, so each test that needs a
//! client DRI seeds a `role = client` person explicitly first — the
//! row survives the (idempotent) seed the subcommand runs.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::entity::{person, project};
use store::test_support::{schema, Schema};

/// Insert a `role = client` person with `email` into the per-test
/// schema so `project create --client-email <email>` can resolve it.
async fn seed_client(s: &Schema, name: &str, email: &str) {
    person::ActiveModel {
        name: ActiveValue::Set(name.to_string()),
        email: ActiveValue::Set(email.to_string()),
        role: ActiveValue::Set(person::Role::Client),
        ..Default::default()
    }
    .insert(&s.db)
    .await
    .expect("seed client person");
}

#[tokio::test]
async fn create_project_inserts_row_linked_to_seeded_entity() {
    let s = schema().await;
    seed_client(&s, "Estate Client", "estate.client@example.com").await;
    let repo_root = tempfile::tempdir().expect("repo root tempdir");
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Shook Estate",
            "--entity-name",
            "shook.family",
            "--client-email",
            "estate.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .env("NAVIGATOR_GIT_REPO_ROOT", repo_root.path())
        .output()
        .expect("run navigator project create");
    assert!(
        out.status.success(),
        "project create failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Shook Estate"),
        "expected name in stdout: {stdout}"
    );

    let list = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("projects")
        .output()
        .expect("run navigator list projects");
    assert!(list.status.success(), "list projects failed");
    let listed = String::from_utf8_lossy(&list.stdout);
    assert!(
        listed.contains("Shook Estate"),
        "expected the new row in list projects: {listed}"
    );
}

#[tokio::test]
async fn create_project_eagerly_provisions_the_git_repo() {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let s = schema().await;
    seed_client(&s, "Repo Client", "repo.client@example.com").await;
    // The child process gets an isolated repo volume via its own env — no
    // process-global `set_var`, so parallel tests are unaffected.
    let repo_root = tempfile::tempdir().expect("repo root tempdir");
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Repo Matter",
            "--entity-name",
            "shook.family",
            "--client-email",
            "repo.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .env("NAVIGATOR_GIT_REPO_ROOT", repo_root.path())
        .output()
        .expect("run navigator project create");
    assert!(
        out.status.success(),
        "project create failed: stderr=\n{}",
        String::from_utf8_lossy(&out.stderr),
    );

    // The row is stamped, and the bare repo exists on the volume — eager
    // provisioning, not lazy-on-first-clone.
    let row = project::Entity::find()
        .filter(project::Column::Name.eq("Repo Matter"))
        .one(&s.db)
        .await
        .expect("query project")
        .expect("project row exists");
    assert!(
        row.git_initialized_at.is_some(),
        "git_initialized_at must be stamped on eager provision",
    );
    let repo_dir = repo_root.path().join(format!("{}.git", row.id));
    assert!(
        repo_dir.join("HEAD").is_file(),
        "bare repo must exist on the volume at {repo_dir:?}",
    );
}

#[tokio::test]
async fn create_project_without_entity_link_is_rejected() {
    // A matter always opens against a pre-existing entity, so `project
    // create` requires `--entity-name`. The entity is resolved before
    // the client, so this fails on the missing entity even though a
    // valid `--client-email` is supplied.
    let s = schema().await;
    seed_client(&s, "Orphan Client", "orphan.client@example.com").await;
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Orphan Matter",
            "--client-email",
            "orphan.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .output()
        .expect("run navigator project create");
    assert!(
        !out.status.success(),
        "create without --entity-name should be rejected",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("entity"),
        "error should name the missing entity: {stderr}"
    );
}

#[tokio::test]
async fn create_project_with_skip_migrate_and_seed_uses_existing_schema() {
    // First pass: prime the schema with migrate+seed via the default mode.
    let s = schema().await;
    seed_client(&s, "Prime Client", "prime.client@example.com").await;
    let repo_root = tempfile::tempdir().expect("repo root tempdir");
    let prime = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Prime Project",
            "--entity-name",
            "shook.family",
            "--client-email",
            "prime.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .env("NAVIGATOR_GIT_REPO_ROOT", repo_root.path())
        .output()
        .expect("prime run");
    assert!(
        prime.status.success(),
        "prime failed: stderr=\n{}",
        String::from_utf8_lossy(&prime.stderr)
    );

    // Second pass: --skip-migrate-and-seed against the same schema.
    // No migrate, no seed — must still succeed because the schema,
    // the `shook.family` row, and the seeded client already exist
    // from the first pass.
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Shook Estate Production",
            "--entity-name",
            "shook.family",
            "--client-email",
            "prime.client@example.com",
            "--skip-migrate-and-seed",
            "--database-url",
        ])
        .arg(&s.url)
        .env("NAVIGATOR_GIT_REPO_ROOT", repo_root.path())
        .output()
        .expect("run navigator project create --skip-migrate-and-seed");
    assert!(
        out.status.success(),
        "--skip-migrate-and-seed create failed: stderr=\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Shook Estate Production"));
}

#[tokio::test]
async fn create_project_rolls_back_when_repo_provisioning_fails() {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let s = schema().await;
    seed_client(&s, "Rollback Client", "rollback.client@example.com").await;
    let file_root = tempfile::NamedTempFile::new().expect("repo root file");
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Rollback Matter",
            "--entity-name",
            "shook.family",
            "--client-email",
            "rollback.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .env("NAVIGATOR_GIT_REPO_ROOT", file_root.path())
        .output()
        .expect("run navigator project create");
    assert!(
        !out.status.success(),
        "create should fail when repo provisioning fails",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("secure document workspace was not ready"),
        "error should use the client-facing workspace message: {stderr}",
    );
    let row = project::Entity::find()
        .filter(project::Column::Name.eq("Rollback Matter"))
        .one(&s.db)
        .await
        .expect("query project");
    assert!(row.is_none(), "project row must roll back on repo failure");
}

#[tokio::test]
async fn create_project_rejects_unknown_entity_name() {
    // The entity is resolved before the client, so an unknown
    // `--entity-name` fails even with a valid `--client-email`.
    let s = schema().await;
    seed_client(&s, "Bad Link Client", "badlink.client@example.com").await;
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Bad Link",
            "--entity-name",
            "definitely.not.a.real.entity",
            "--client-email",
            "badlink.client@example.com",
            "--database-url",
        ])
        .arg(&s.url)
        .output()
        .expect("run navigator project create");
    assert!(
        !out.status.success(),
        "expected nonzero exit for unknown entity"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no entity named"),
        "expected explanatory error on stderr: {stderr}"
    );
}
