//! End-to-end tests for `navigator docs ...`.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;

#[test]
fn docs_requires_a_subcommand() {
    let out = Command::new(cargo_bin("navigator"))
        .arg("docs")
        .output()
        .expect("run navigator docs");
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Usage: navigator docs <COMMAND>"),
        "expected docs subcommand usage, got: {stderr}",
    );
}

#[test]
fn docs_list_includes_erd_and_glossary_term_pages() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["docs", "list"])
        .output()
        .expect("run navigator docs list");
    assert!(out.status.success(), "exit status: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("/docs/erd\t"));
    assert!(stdout.contains("/docs/glossary#staff-review\tGlossary: Staff Review"));
    assert!(stdout.contains("/docs/glossary#workflow-runtime\tGlossary: Workflow Runtime"));
}

#[test]
fn docs_glossary_with_known_term_prints_just_that_term() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["docs", "glossary", "Staff Review"])
        .output()
        .expect("run navigator docs glossary Staff Review");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("## Staff Review"));
    assert!(stdout.contains("`staff_review`"));
    assert!(stdout.contains("notation-authoring.md"));
    assert!(!stdout.contains("## Workflow Runtime"));
}

#[test]
fn docs_glossary_term_lookup_accepts_anchor_slug() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["docs", "glossary", "staff-review"])
        .output()
        .expect("run navigator docs glossary staff-review");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("## Staff Review"));
}

#[test]
fn docs_glossary_unknown_term_exits_non_zero_with_helpful_stderr() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["docs", "glossary", "not-a-real-term"])
        .output()
        .expect("run navigator docs glossary on unknown term");
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown term"),
        "expected `unknown term` in stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("Run `navigator docs list`"),
        "expected hint in stderr, got: {stderr}",
    );
}
