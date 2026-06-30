//! Integration tests for `navigator --version` / `-V`.
//!
//! The published binary must self-report the `YY.M.D` release tag (no
//! leading zeros on any component — June 5 is `26.6.5`) that
//! `deploy.yml` built it from — not the placeholder `0.1.0` workspace crate
//! version. `deploy.yml` exposes that tag to `cargo build` as
//! `NAVIGATOR_RELEASE_TAG`; the CLI honors the same env var at *runtime* (the
//! codebase-wide convention `web`/`lsp` already follow) and bakes it at build
//! time via `build.rs` so the downloaded binary keeps reporting it with no env
//! var set. These tests pin both ends of that contract.

use assert_cmd::Command;
use predicates::str;

/// A runtime `NAVIGATOR_RELEASE_TAG` wins — this is what the baked build-time
/// value rides on, and the seam that lets us assert the format without a
/// rebuild.
#[test]
fn version_reports_the_release_tag_when_set() {
    Command::cargo_bin("navigator")
        .unwrap()
        .env("NAVIGATOR_RELEASE_TAG", "26.6.5")
        .arg("--version")
        .assert()
        .success()
        .stdout(str::contains("navigator 26.6.5"));
}

/// `-V` is the short alias for the same flag and must agree.
#[test]
fn short_version_flag_matches() {
    Command::cargo_bin("navigator")
        .unwrap()
        .env("NAVIGATOR_RELEASE_TAG", "26.6.5")
        .arg("-V")
        .assert()
        .success()
        .stdout(str::contains("navigator 26.6.5"));
}

/// An empty/whitespace tag is ignored — it must never surface as the version.
/// With no usable tag the binary falls back to the build-time baked value (the
/// crate version `0.1.0` on a plain local build), never an empty string.
#[test]
fn blank_release_tag_falls_back_and_is_never_blank() {
    Command::cargo_bin("navigator")
        .unwrap()
        .env("NAVIGATOR_RELEASE_TAG", "   ")
        .arg("--version")
        .assert()
        .success()
        .stdout(str::contains("navigator "))
        .stdout(str::is_match(r"navigator \S").unwrap());
}
