//! End-to-end tests for `navigator list <subject>`. Every list call
//! runs the full canonical seed pass first (idempotent), so a fresh
//! database is enough to see the canonical rows. Imported templates
//! remain on top of the seeded data.

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use store::test_support::{schema, Schema};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("repo root exists")
}

/// Spin up a fresh per-test schema, then import the workspace
/// templates into it. Returns the [`Schema`] handle (DB + URL) so
/// the caller can both query the DB directly and pass the URL to
/// subprocess invocations of `cli list`.
async fn populated_schema() -> Schema {
    let s = schema().await;
    let bin = cargo_bin("navigator");
    let out = Command::new(&bin)
        .args(["import", "--database-url"])
        .arg(&s.url)
        .arg(repo_root().join("templates"))
        .output()
        .expect("run navigator import");
    assert!(
        out.status.success(),
        "import failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    s
}

#[tokio::test]
async fn list_questions_prints_every_canonical_code() {
    let s = populated_schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("questions")
        .output()
        .expect("run navigator list questions");
    assert!(
        out.status.success(),
        "list questions failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    for code in [
        "person",
        "people",
        "entity",
        "custom_text",
        "custom_single_choice",
        "staff_review",
        "testator_signature", // only added by the notation import (will workflow state)
    ] {
        assert!(
            stdout.contains(code),
            "expected `{code}` in `list questions` output:\n{stdout}",
        );
    }
}

#[tokio::test]
async fn list_against_fresh_db_auto_seeds() {
    // No prior seed/import — `list` must still produce the full
    // canonical question set on its own.
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("questions")
        .output()
        .expect("run navigator list questions");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("custom_text"),
        "fresh DB list must contain canonical questions:\n{stdout}"
    );
}

#[tokio::test]
async fn list_templates_prints_imported_titles() {
    let s = populated_schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("templates")
        .output()
        .expect("run navigator list templates");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "trusts__nevada",
        "Nevada Trust",
        "ca__llc_operating_agreement",
        "California LLC Operating Agreement",
        "will__simple",
        "Simple Last Will and Testament",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in `list templates` output:\n{stdout}",
        );
    }
}

#[tokio::test]
async fn list_jurisdictions_prints_full_state_set() {
    let s = populated_schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("jurisdictions")
        .output()
        .expect("run navigator list jurisdictions");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for needle in ["NV", "Nevada", "CA", "California", "DC", "GMBH"] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in `list jurisdictions` output:\n{stdout}",
        );
    }
}

#[tokio::test]
async fn list_persons_includes_seeded_emails() {
    let s = populated_schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("persons")
        .output()
        .expect("run navigator list persons");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let needle = "nick@neonlaw.com";
    assert!(
        stdout.contains(needle),
        "expected `{needle}` in `list persons` output:\n{stdout}",
    );
}

#[tokio::test]
async fn list_entities_includes_seeded_org_names() {
    let s = populated_schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("entities")
        .output()
        .expect("run navigator list entities");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for needle in ["Shook Law PLLC", "Neon Law Foundation"] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in `list entities` output:\n{stdout}",
        );
    }
}

#[tokio::test]
async fn list_templates_against_a_seed_only_db_shows_the_bundled_retainer() {
    // The canonical seed pass now bundles the retainer notation
    // template (see `store::seed::seed_templates`), so a fresh
    // seed-only DB carries exactly one row — `onboarding__retainer`.
    let s = schema().await;
    let out = Command::new(cargo_bin("navigator"))
        .args(["list", "--database-url"])
        .arg(&s.url)
        .arg("templates")
        .output()
        .expect("run navigator list templates");
    assert!(out.status.success(), "fresh-DB list must still succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("onboarding__retainer"),
        "expected the seeded retainer template; got:\n{stdout}",
    );
    assert!(stdout.contains("Retainer Agreement"));
}
