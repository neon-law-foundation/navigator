//! `navigator ops release-version` end-to-end: the command rewrites the one
//! workspace-version line and leaves everything else — dependency pins
//! especially — untouched. `--no-commit` keeps these hermetic: no git repo is
//! required and nothing is committed, so the assertion is purely on the file the
//! command wrote.

use assert_cmd::Command;
use chrono::Datelike;
use std::fs;

/// A minimal workspace manifest with a dependency `version =` that MUST survive,
/// so a regression that widens the rewrite to the whole file is caught here.
const MANIFEST: &str = "\
[workspace.package]
version = \"0.1.0\"
edition = \"2021\"
license = \"AGPL-3.0-only\"

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

/// `--hotfix` writes the same-day release spelling: a `-hotfix.H` prerelease on
/// TOMORROW's date. This is the version a `deploy.yml` hotfix tag must equal, so
/// the shape is asserted against the workflow's own regex rather than eyeballed.
#[test]
fn hotfix_writes_a_prerelease_on_tomorrows_date() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("Cargo.toml");
    fs::write(&manifest, MANIFEST).expect("write manifest");

    run(&[
        "ops",
        "release-version",
        "--hotfix",
        "--no-commit",
        "--manifest-path",
        manifest.to_str().unwrap(),
    ])
    .success();

    let written = fs::read_to_string(&manifest).expect("read manifest");
    let version = written
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("the workspace version line must still be present");

    let (base, hour) = version
        .split_once("-hotfix.")
        .expect("`--hotfix` must write a `-hotfix.H` prerelease");

    // Tomorrow's base, derived independently of the command under test. Semver
    // ranks a prerelease BELOW its own base, so today's base would sort the fix
    // as older than the release it fixes — the next day is what makes it correct.
    let tomorrow = chrono::Utc::now()
        .date_naive()
        .checked_add_days(chrono::Days::new(1))
        .expect("valid date");
    assert_eq!(
        base,
        format!(
            "{}.{}.{}",
            tomorrow.year() % 100,
            tomorrow.month(),
            tomorrow.day()
        ),
        "a hotfix hangs off TOMORROW's base"
    );

    // The hour is an unpadded 0-23, which is both `deploy.yml`'s regex and the
    // semver rule that a numeric prerelease identifier carries no leading zero.
    assert!(
        !hour.starts_with('0') || hour == "0",
        "the hour must be unpadded — `hotfix.08` is invalid semver, got {hour:?}"
    );
    let hour: u32 = hour.parse().expect("the hour must be numeric");
    assert!(hour <= 23, "the hour must be a real UTC hour, got {hour}");
    assert!(
        written.contains("serde = { version = \"1\" }"),
        "a dependency pin must never be rewritten"
    );
}

/// `--hotfix` and `--tag` are mutually exclusive: one derives the version and the
/// other dictates it, so accepting both would silently honour one and ignore the
/// other.
#[test]
fn hotfix_and_tag_cannot_be_combined() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = dir.path().join("Cargo.toml");
    fs::write(&manifest, MANIFEST).expect("write manifest");

    run(&[
        "ops",
        "release-version",
        "--hotfix",
        "--tag",
        "26.8.14",
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
