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
