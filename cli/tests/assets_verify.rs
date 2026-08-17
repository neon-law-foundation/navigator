//! Integration coverage for `navigator ops assets verify` — drives the real
//! binary (clap parsing + dispatch + the tokio-runtime glue in
//! `run_verify`) on the two branches that need no live origin: an empty
//! content tree, which still probes the licensed faces (→ exit 2 against
//! an origin that refuses the connection), and a referenced image with no
//! configured origin (→ exit 2). The live HTTP path is unit-tested against
//! `wiremock` in `assets.rs`.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

/// The GORP faces are probed on every run, not just when content happens to
/// reference an image — they are published by `assets fonts upload` alone, so
/// an empty tree is exactly when an unpublished typeface would otherwise slip
/// through. Points at a closed port so the probe is attempted and reported
/// without a live origin.
#[test]
fn verify_probes_the_licensed_fonts_on_a_content_tree_with_no_image_references() {
    let content = TempDir::new().unwrap();
    fs::create_dir_all(content.path().join("blog")).unwrap();

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["ops", "assets", "verify"])
        .arg("--content")
        .arg(content.path())
        // Nothing listens on port 1: each font probe fails to connect, which
        // is itself the proof that verify reached for them.
        .args(["--base-url", "http://127.0.0.1:1"])
        // Isolate from any ambient origin in the test environment.
        .env_remove("NAVIGATOR_ASSET_BASE_URL")
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "fonts/gorp-serif/GORPSerif-Regular.woff2",
        ))
        .stderr(predicates::str::contains(
            "fonts/gorp-serif/GORPSerif-Bold.woff2",
        ));
}

/// A real `img/…` reference with a blank origin hits the "no public
/// origin" guard and exits non-zero before any network call.
#[test]
fn verify_fails_when_an_image_is_referenced_but_no_origin_is_set() {
    let content = TempDir::new().unwrap();
    let blog = content.path().join("blog");
    fs::create_dir_all(&blog).unwrap();
    fs::write(blog.join("post.md"), "![hero](img/demo/hero.png)\n").unwrap();

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["ops", "assets", "verify"])
        .arg("--content")
        .arg(content.path())
        .args(["--base-url", "   "])
        .env_remove("NAVIGATOR_ASSET_BASE_URL")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("no public origin"));
}
