//! `navigator ops release-version` end-to-end: the command rewrites the one
//! workspace-version line and leaves everything else — dependency pins
//! especially — untouched. `--no-commit` keeps these hermetic: no git repo is
//! required and nothing is committed, so the assertion is purely on the file the
//! command wrote.

use assert_cmd::Command;
use std::fs;

/// A minimal workspace manifest with a dependency `version =` that MUST survive,
/// so a regression that widens the rewrite to the whole file is caught here.
const MANIFEST: &str = "\
[workspace.package]
version = \"0.1.0\"
edition = \"2021\"
license = \"MIT OR Apache-2.0\"

[workspace.dependencies]
serde = { version = \"1\" }
";

fn run(args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("navigator").unwrap().args(args).assert()
}

#[test]
fn writes_the_explicit_version_and_preserves_dependency_pins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("Cargo.toml");
    fs::write(&manifest, MANIFEST).expect("write manifest");

    run(&[
        "ops",
        "release-version",
        "--tag",
        "26.8.14",
        "--no-commit",
        "--manifest-path",
        manifest.to_str().unwrap(),
    ])
    .success();

    let written = fs::read_to_string(&manifest).expect("read manifest");
    assert!(
        written.contains("version = \"26.8.14\""),
        "the workspace version must be bumped"
    );
    assert!(
        !written.contains("0.1.0"),
        "the old workspace version must be gone"
    );
    assert!(
        written.contains("serde = { version = \"1\" }"),
        "a dependency pin must never be rewritten"
    );
}

#[test]
fn an_empty_tag_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("Cargo.toml");
    fs::write(&manifest, MANIFEST).expect("write manifest");

    run(&[
        "ops",
        "release-version",
        "--tag",
        "   ",
        "--no-commit",
        "--manifest-path",
        manifest.to_str().unwrap(),
    ])
    .failure();

    assert!(
        fs::read_to_string(&manifest)
            .expect("read manifest")
            .contains("0.1.0"),
        "a rejected run must not touch the manifest"
    );
}

#[test]
fn a_manifest_without_a_workspace_version_fails_loudly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("Cargo.toml");
    fs::write(&manifest, "[workspace.package]\nedition = \"2021\"\n").expect("write manifest");

    run(&[
        "ops",
        "release-version",
        "--tag",
        "26.8.14",
        "--no-commit",
        "--manifest-path",
        manifest.to_str().unwrap(),
    ])
    .failure();
}
