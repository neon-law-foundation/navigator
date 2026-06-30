//! End-to-end tests for `navigator validate-i18n <root>` — confirms the
//! subcommand wires the catalog↔call-site audit into the CLI dispatch and
//! reports drift on stdout with the right exit code.

use std::fs;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

/// Write `<root>/<rel>` (creating parents).
fn write_source(root: &std::path::Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

#[test]
fn passes_when_every_catalog_key_is_referenced() {
    // A temp tree whose source names every En catalog key leaves nothing
    // missing and nothing unused, so the gate exits 0.
    let work = TempDir::new().unwrap();
    let body = views::i18n::en_catalog_keys()
        .iter()
        .map(|k| format!("let _ = i18n::t(locale, \"{k}\");"))
        .collect::<Vec<_>>()
        .join("\n");
    write_source(
        work.path(),
        "views/src/all.rs",
        &format!("fn r() {{ {body} }}"),
    );

    let out = Command::new(cargo_bin("navigator"))
        .arg("validate-i18n")
        .arg(work.path())
        .output()
        .expect("run navigator validate-i18n");

    assert!(
        out.status.success(),
        "exit: {:?}\nstdout: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn fails_and_reports_a_missing_key() {
    let work = TempDir::new().unwrap();
    write_source(
        work.path(),
        "web/src/x.rs",
        "fn r() { let _ = i18n::t(locale, \"nope.not.real\"); }",
    );

    let out = Command::new(cargo_bin("navigator"))
        .arg("validate-i18n")
        .arg(work.path())
        .output()
        .expect("run navigator validate-i18n");

    assert!(!out.status.success(), "drift must exit non-zero");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("I18N-MISSING") && stdout.contains("nope.not.real"),
        "stdout: {stdout}"
    );
}
