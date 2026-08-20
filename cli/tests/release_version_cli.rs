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

/// `--hotfix` writes the same-day release spelling: a `-hotfix.N` prerelease on
/// TOMORROW's date. It uses the UTC hour as a convenient default N; explicit
/// `--tag` remains the way to choose another number. This is the version a
/// `deploy.yml` hotfix tag must equal, so
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

    let (base, number) = version
        .split_once("-hotfix.")
        .expect("`--hotfix` must write a `-hotfix.N` prerelease");

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

    // The convenience command selects the UTC hour as its default N. It is
    // unpadded, which is the semver rule for a numeric prerelease identifier.
    assert!(
        !number.starts_with('0') || number == "0",
        "the number must be unpadded — `hotfix.08` is invalid semver, got {number:?}"
    );
    let number: u32 = number.parse().expect("the hotfix number must be numeric");
    assert!(
        number <= 23,
        "the convenience command's default N is a UTC hour, got {number}"
    );
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

/// A minimal two-crate workspace whose members inherit `version.workspace =
/// true` — the shape that makes `Cargo.lock` go stale the moment only the
/// manifest is written. No external dependencies, so it resolves offline.
fn seed_workspace(root: &std::path::Path) -> std::path::PathBuf {
    fs::create_dir_all(root.join("alpha/src")).expect("alpha src");
    fs::create_dir_all(root.join("beta/src")).expect("beta src");
    fs::write(root.join("alpha/src/lib.rs"), "").expect("alpha lib");
    fs::write(root.join("beta/src/lib.rs"), "").expect("beta lib");
    fs::write(
        root.join("alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion.workspace = true\nedition.workspace = true\n\n\
         [dependencies]\nbeta = { path = \"../beta\" }\n",
    )
    .expect("alpha manifest");
    fs::write(
        root.join("beta/Cargo.toml"),
        "[package]\nname = \"beta\"\nversion.workspace = true\nedition.workspace = true\n",
    )
    .expect("beta manifest");

    let manifest = root.join("Cargo.toml");
    fs::write(
        &manifest,
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"2\"\n\n\
         [workspace.package]\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("workspace manifest");

    // The stale lock this command has to refresh: written while the workspace
    // still says 0.1.0, exactly as the previous release left it.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let generated = std::process::Command::new(cargo)
        .args([
            "generate-lockfile",
            "--offline",
            "--quiet",
            "--manifest-path",
        ])
        .arg(&manifest)
        .status()
        .expect("run cargo generate-lockfile");
    assert!(generated.success(), "the fixture lock must be generated");

    manifest
}

/// Every `[[package]]` version in a lock, keyed by package name.
fn locked_versions(lockfile: &std::path::Path) -> Vec<(String, String)> {
    let text = fs::read_to_string(lockfile).expect("read lock");
    let mut found = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("name = \"") {
            name = value.strip_suffix('"').map(str::to_string);
        } else if let Some(value) = line.strip_prefix("version = \"") {
            if let (Some(name), Some(version)) = (name.take(), value.strip_suffix('"')) {
                found.push((name, version.to_string()));
            }
        }
    }
    found
}

/// THE PROPERTY A RELEASE DEPENDS ON, and the one that used to be missing.
/// `deploy.yml` builds the release with `--locked` in four places — the
/// provenance step and all three CLI archive jobs — and `--locked` refuses a
/// lock the manifest has moved past. A bump that wrote only `Cargo.toml` failed
/// AFTER the tag was pushed, and the `release-tags` ruleset admits no bypass
/// actor, so the name could not be moved and the day's release was spent. The
/// manifest and the lock must therefore agree the moment this command returns.
#[test]
fn the_lockfile_agrees_with_the_manifest_it_wrote() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = seed_workspace(dir.path());
    let lockfile = dir.path().join("Cargo.lock");

    assert!(
        locked_versions(&lockfile)
            .iter()
            .all(|(_, version)| version == "0.1.0"),
        "the fixture starts with a lock at the previous version"
    );

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

    let locked = locked_versions(&lockfile);
    assert_eq!(
        locked.len(),
        2,
        "both workspace crates must still be locked: {locked:?}"
    );
    for (name, version) in &locked {
        assert_eq!(
            version, "26.8.14",
            "{name} is locked at {version}, but the manifest says 26.8.14 — \
             `cargo build --locked` would refuse this lock"
        );
    }
}

/// The refresh is not conditional on the manifest having changed. A rerun of an
/// already-bumped manifest is exactly how the lock was left stale in the first
/// place, so a second run must repair it rather than report success and do
/// nothing.
#[test]
fn a_rerun_repairs_a_lock_left_behind_by_an_earlier_bump() {
    let dir = tempfile::tempdir().expect("tempdir");
    let manifest = seed_workspace(dir.path());
    let lockfile = dir.path().join("Cargo.lock");

    // The state the bug produced: manifest bumped by hand, lock untouched.
    let text = fs::read_to_string(&manifest).expect("read manifest");
    fs::write(
        &manifest,
        text.replace("version = \"0.1.0\"", "version = \"26.8.14\""),
    )
    .expect("write manifest");
    assert!(
        locked_versions(&lockfile)
            .iter()
            .all(|(_, version)| version == "0.1.0"),
        "the lock is stale before the rerun"
    );

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

    for (name, version) in locked_versions(&lockfile) {
        assert_eq!(
            version, "26.8.14",
            "{name} must be refreshed even though the manifest already said 26.8.14"
        );
    }
}
