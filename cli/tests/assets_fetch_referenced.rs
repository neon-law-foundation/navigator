//! Integration coverage for `navigator ops assets fetch-referenced` — drives
//! the real binary on the no-origin guard and a wiremock-backed happy
//! path. The live HTTP download logic is unit-tested in `assets.rs`.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A blank origin hits the shared no-origin guard before any network call.
#[test]
fn fetch_referenced_fails_when_no_origin_is_set() {
    let content = TempDir::new().unwrap();
    let blog = content.path().join("blog");
    fs::create_dir_all(&blog).unwrap();
    fs::write(blog.join("post.md"), "![hero](img/demo/hero.png)\n").unwrap();
    let out = TempDir::new().unwrap();

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["ops", "assets", "fetch-referenced"])
        .arg("--content")
        .arg(content.path())
        .arg("--out")
        .arg(out.path())
        .args(["--base-url", "   "])
        .env_remove("NAVIGATOR_ASSET_BASE_URL")
        .assert()
        .code(2)
        .stderr(predicates::str::contains("no public origin"));
}

/// An empty content tree short-circuits to success without fetching.
#[test]
fn fetch_referenced_succeeds_when_the_content_tree_has_no_image_references() {
    let content = TempDir::new().unwrap();
    fs::create_dir_all(content.path().join("blog")).unwrap();
    let out = TempDir::new().unwrap();

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["ops", "assets", "fetch-referenced"])
        .arg("--content")
        .arg(content.path())
        .arg("--out")
        .arg(out.path())
        .args(["--base-url", "https://cdn.example"])
        .env_remove("NAVIGATOR_ASSET_BASE_URL")
        .assert()
        .success()
        .stdout(predicates::str::contains("no content image references"));
}

/// Downloads a referenced image over public HTTP into `<out>/img/…`.
#[tokio::test]
async fn fetch_referenced_writes_bytes_from_the_public_origin() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/img/demo/hero.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"png-bytes"))
        .mount(&server)
        .await;

    let content = TempDir::new().unwrap();
    let blog = content.path().join("blog");
    fs::create_dir_all(&blog).unwrap();
    fs::write(blog.join("post.md"), "![hero](img/demo/hero.png)\n").unwrap();
    let out = TempDir::new().unwrap();

    Command::cargo_bin("navigator")
        .unwrap()
        .args(["ops", "assets", "fetch-referenced"])
        .arg("--content")
        .arg(content.path())
        .arg("--out")
        .arg(out.path())
        .args(["--base-url", &server.uri()])
        .env_remove("NAVIGATOR_ASSET_BASE_URL")
        .assert()
        .success()
        .stdout(predicates::str::contains("fetched 1 content image"));

    assert_eq!(
        fs::read(out.path().join("img/demo/hero.png")).unwrap(),
        b"png-bytes"
    );
}
