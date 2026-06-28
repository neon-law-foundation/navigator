//! Integration tests for the top-level `navigator --help` output.

use assert_cmd::Command;
use predicates::str;

#[test]
fn top_level_help_disclaims_legal_advice() {
    Command::cargo_bin("navigator")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(str::contains("Nothing here is legal advice"))
        .stdout(str::contains("an attorney"))
        .stdout(str::contains("remains responsible"))
        .stdout(str::contains("responsible for legal advice and judgment"));
}

#[test]
fn notation_create_is_canonical_and_matter_open_is_deprecated_alias() {
    Command::cargo_bin("navigator")
        .unwrap()
        .args(["notation", "create", "--help"])
        .assert()
        .success()
        .stdout(str::contains("Usage: navigator notation create"))
        .stdout(str::contains("<TEMPLATE_CODE>"))
        .stdout(str::contains("--client-email"))
        .stdout(str::contains("--project"));

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["matter", "open", "--help"])
        .assert()
        .success()
        .stdout(str::contains("Deprecated alias"))
        .stdout(str::contains("notation create <template-code>"));
}
