//! End-to-end tests for the `compass validate <dir>` binary.

use std::fs;
use std::path::Path;

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

fn compass() -> Command {
    Command::cargo_bin("compass").unwrap()
}

#[test]
fn compass_validate_passes_on_file_satisfying_all_markdown_rules() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "Ok.md", "Body paragraph.\n");
    compass()
        .args(["validate", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(str::contains("Scanned 1 file(s), found 0 violation(s)"));
}

#[test]
fn compass_validate_still_runs_navigator_rules() {
    let dir = TempDir::new().unwrap();
    // Missing respondent_type triggers F102 (navigator rule, inherited).
    write(
        dir.path(),
        "Bad.md",
        "---\ntitle: Trust\n---\n\nBody paragraph.\n",
    );
    compass()
        .args(["validate"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(str::contains("F102"));
}

#[test]
fn compass_validate_flags_c001_when_body_is_empty() {
    let dir = TempDir::new().unwrap();
    // No frontmatter, empty body — under `--markdown-only` the F-family
    // is skipped, so C001 is the only thing that should fire.
    write(dir.path(), "Empty.md", "");
    compass()
        .args(["validate", "--markdown-only"])
        .arg(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(str::contains("C001"));
}
