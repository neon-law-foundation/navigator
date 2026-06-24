//! Integration tests for the `navigator validate <dir>` subcommand.
//!
//! These drive the compiled binary through `assert_cmd` so the test
//! exercises the real argv parsing, exit codes, and stdout the user
//! will see — not just the library it wraps.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str;
use tempfile::TempDir;

fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn navigator() -> Command {
    Command::cargo_bin("navigator").unwrap()
}

#[test]
fn validate_succeeds_on_clean_directory() {
    let dir = TempDir::new().unwrap();
    // Use markdown-only mode so the test doesn't need to satisfy the
    // full N-family notation-template expectations (questionnaire/workflow
    // maps, confidential, staff_review). Those rules have dedicated
    // unit tests in the rules crate.
    write(dir.path(), "Notes.md", "Plain body line.\n");
    navigator()
        .args(["validate", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("Scanned 1 file(s), found 0 violation(s)"));
}

#[test]
fn validate_exits_nonzero_on_violations_and_prints_each_one() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "Bad.md",
        &format!("Intro.\n\n{}\n", "x".repeat(121)),
    );
    navigator()
        .args(["validate", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(str::contains("S101"))
        .stdout(str::contains("Scanned 1 file(s), found"));
}

#[test]
fn validate_default_rule_set_flags_missing_frontmatter() {
    let dir = TempDir::new().unwrap();
    // Files under `notation_templates/` are notation templates even before
    // they have enough frontmatter to self-identify.
    write(
        dir.path(),
        "notation_templates/notes.md",
        "Just a body line.\n",
    );
    navigator()
        .args(["validate"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(str::contains("N101"))
        .stdout(str::contains("N102"));
}

#[test]
fn validate_default_treats_code_only_frontmatter_as_markdown() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "web/content/marketing/service.md",
        "---\ntitle: Service\ncode: northstar\n---\n\nBody.\n",
    );
    navigator()
        .args(["validate", "--no-default-excludes"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("Scanned 1 file(s), found 0 violation(s)"));
}

#[test]
fn validate_returns_exit_code_2_when_directory_does_not_exist() {
    navigator()
        .args(["validate", "/definitely/does/not/exist/12345"])
        .assert()
        .failure()
        .code(2)
        .stderr(str::contains("navigator:"));
}

#[test]
fn validate_skips_readme_and_claude_files() {
    let dir = TempDir::new().unwrap();
    // Both files would violate S101 — but they should be skipped.
    write(dir.path(), "README.md", &"x".repeat(200));
    write(dir.path(), "CLAUDE.md", &"x".repeat(200));
    write(dir.path(), "Ok.md", "Plain body line.\n");
    navigator()
        .args(["validate", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("Scanned 1 file(s)"));
}

/// The repository root, derived from this crate's manifest dir
/// (`CARGO_MANIFEST_DIR` points at `cli/`; the workspace is one up).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("workspace root exists")
}

/// CI guard: every shipped example notation under `notation_templates/` must pass
/// the *classified* (default-mode) validator with zero violations.
///
/// Files under `notation_templates/` are always classified as notation templates,
/// so this runs the full N-family (N101–N108) plus the markdown rules
/// against each one. It is the enforcement the prompt asks for — running
/// inside `cargo test --workspace`, it fails CI the moment a template (or
/// a newly added one) drifts out of conformance. Keep the example
/// notations conforming; do not loosen this test to make a bad template
/// pass.
#[test]
fn every_template_notation_passes_classified_validation() {
    let templates = workspace_root().join("notation_templates");
    assert!(
        templates.is_dir(),
        "notation_templates/ directory must exist at {}",
        templates.display(),
    );
    navigator()
        .arg("validate")
        .arg(&templates)
        .assert()
        .success()
        .stdout(str::contains("found 0 violation(s)"));
}

/// Companion guard: the same notations must also pass under
/// `--markdown-only`, which forces the markdown rules (M-family + S101 +
/// S102) onto every file regardless of classification. This catches
/// prose-level regressions (long lines, hard tabs, trailing whitespace)
/// that the notation-only path would not surface.
#[test]
fn every_template_notation_passes_markdown_only_validation() {
    let templates = workspace_root().join("notation_templates");
    navigator()
        .args(["validate", "--markdown-only"])
        .arg(&templates)
        .assert()
        .success()
        .stdout(str::contains("found 0 violation(s)"));
}

#[test]
fn validate_events_accepts_bundled_event_markdown() {
    let events = workspace_root().join("web/content/events");
    navigator()
        .arg("validate-events")
        .arg(&events)
        .assert()
        .success()
        .stdout(str::contains("Validated 1 event markdown file(s)"));
}

#[test]
fn validate_events_rejects_missing_required_frontmatter() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "20260702_bad.md",
        "---\ntitle: Bad Event\n---\n\nBody.\n",
    );
    navigator()
        .arg("validate-events")
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(str::contains("invalid event front matter"));
}

#[test]
fn missing_subcommand_prints_usage_and_fails() {
    navigator()
        .assert()
        .failure()
        .stderr(str::contains("Usage:"));
}

#[test]
fn validate_fix_writes_back_autofixable_edits_and_reports_remaining() {
    let dir = TempDir::new().unwrap();
    // Three trailing spaces (M009 violates — two-space hard break is
    // exempt, three is not) + a hard tab (M010). Both autofixable.
    write(
        dir.path(),
        "Mixed.md",
        "Body line with trailing spaces   \nTabbed\there\n",
    );
    navigator()
        .args(["validate", "--fix", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .stdout(str::contains("fixed"))
        .stdout(str::contains("Fixed 1 file(s)"));
    let after = fs::read_to_string(dir.path().join("Mixed.md")).unwrap();
    assert_eq!(
        after, "Body line with trailing spaces\nTabbed  here\n",
        "expected M009 + M010 autofixes; got: {after:?}",
    );
}

#[test]
fn validate_fix_leaves_diagnostic_only_violations_for_human() {
    let dir = TempDir::new().unwrap();
    // M010 (autofixable) + N101 (diagnostic-only) in the same
    // notation-template file.
    write(
        dir.path(),
        "notation_templates/needs.md",
        "---\nrespondent_type: entity\n---\n\n\tTabbed\n",
    );
    navigator()
        .args(["validate", "--fix"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(str::contains("N101"))
        .stdout(str::contains("remaining violation"));
    // The autofixable tab is gone.
    let after = fs::read_to_string(dir.path().join("notation_templates/needs.md")).unwrap();
    assert!(
        !after.contains('\t'),
        "tab should be replaced; got: {after:?}"
    );
}

#[test]
fn validate_fix_is_idempotent() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "OnlyFixable.md", "Body  \n\tIndent\n");
    navigator()
        .args(["validate", "--fix", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .success();
    // Second run finds nothing to fix.
    navigator()
        .args(["validate", "--fix", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("Fixed 0 file(s)"));
}
