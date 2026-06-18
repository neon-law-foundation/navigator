//! End-to-end tests for `navigator glossary [term]`. The command
//! does no I/O beyond stdout, so the tests just spawn the binary
//! and assert on its captured output.

use std::process::Command;

use assert_cmd::cargo::cargo_bin;

#[test]
fn glossary_without_argument_lists_every_term() {
    let out = Command::new(cargo_bin("navigator"))
        .arg("glossary")
        .output()
        .expect("run navigator glossary");
    assert!(out.status.success(), "exit status: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Template — "));
    assert!(stdout.contains("Notation — "));
    assert!(stdout.contains("Jurisdiction — "));
    // Sanity: the full dump is at least the count of terms ported.
    let line_count = stdout.lines().filter(|l| !l.is_empty()).count();
    assert!(
        line_count >= 25,
        "expected >= 25 term lines, got {line_count}"
    );
}

#[test]
fn glossary_with_known_term_prints_just_that_term() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["glossary", "Notation"])
        .output()
        .expect("run navigator glossary Notation");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Notation — "));
    // One term only — no "Template — " line.
    assert!(!stdout.contains("Template — "));
}

#[test]
fn glossary_term_lookup_is_case_insensitive() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["glossary", "template"])
        .output()
        .expect("run navigator glossary template (lower-case)");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Template — "));
}

#[test]
fn glossary_unknown_term_exits_non_zero_with_helpful_stderr() {
    let out = Command::new(cargo_bin("navigator"))
        .args(["glossary", "not-a-real-term"])
        .output()
        .expect("run navigator glossary on unknown term");
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown term"),
        "expected `unknown term` in stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("Run `navigator glossary`"),
        "expected hint in stderr, got: {stderr}",
    );
}
