//! End-to-end tests for `cli project create`. The subcommand runs
//! migrate + seed against the target Postgres so the canonical
//! `shook.family` entity is in place by the time we look it up.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use store::test_support::schema;

#[tokio::test]
async fn create_project_inserts_row_linked_to_seeded_entity() {
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Shook Estate",
            "--entity-name",
            "shook.family",
            "--database-url",
        ])
        .arg(&s.url)
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
async fn create_project_without_entity_link_is_rejected() {
    // A matter always opens against a pre-existing entity, so `project
    // create` requires `--entity-name`.
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Orphan Matter",
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
    let prime = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Prime Project",
            "--entity-name",
            "shook.family",
            "--database-url",
        ])
        .arg(&s.url)
        .output()
        .expect("prime run");
    assert!(
        prime.status.success(),
        "prime failed: stderr=\n{}",
        String::from_utf8_lossy(&prime.stderr)
    );

    // Second pass: --skip-migrate-and-seed against the same schema.
    // No migrate, no seed — must still succeed because the schema
    // and `shook.family` row already exist from the first pass.
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Shook Estate Production",
            "--entity-name",
            "shook.family",
            "--skip-migrate-and-seed",
            "--database-url",
        ])
        .arg(&s.url)
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
async fn create_project_rejects_unknown_entity_name() {
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args([
            "project",
            "create",
            "--name",
            "Bad Link",
            "--entity-name",
            "definitely.not.a.real.entity",
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
