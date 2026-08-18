//! `navigator ops release-provenance` proves that a release tag resolves to a
//! commit reachable from `origin/main`. The tests use a real temporary Git
//! repository because annotated-tag peeling and ancestry are Git behavior, not
//! string parsing.

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as GitCommand;

struct Repository {
    _root: tempfile::TempDir,
    checkout: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let checkout = root.path().join("checkout");
        let remote = root.path().join("origin.git");

        git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            root.path(),
            &["init", "--initial-branch=main", checkout.to_str().unwrap()],
        );
        git(&checkout, &["config", "user.name", "Navigator Test"]);
        git(
            &checkout,
            &["config", "user.email", "navigator@example.com"],
        );
        git(&checkout, &["config", "commit.gpgsign", "false"]);
        git(&checkout, &["config", "tag.gpgsign", "false"]);
        std::fs::write(checkout.join("release.txt"), "main\n").expect("write fixture");
        git(&checkout, &["add", "release.txt"]);
        git(&checkout, &["commit", "-m", "main release"]);
        git(
            &checkout,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&checkout, &["push", "-u", "origin", "main"]);

        Self {
            _root: root,
            checkout,
        }
    }

    fn tag(&self, tag: &str, annotated: bool) {
        if annotated {
            git(&self.checkout, &["tag", "-a", tag, "-m", tag]);
        } else {
            git(&self.checkout, &["tag", tag]);
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = GitCommand::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn verify(repo: &Repository, tag: &str) -> assert_cmd::assert::Assert {
    Command::cargo_bin("navigator")
        .expect("navigator binary")
        .args([
            "ops",
            "release-provenance",
            "--tag",
            tag,
            "--repo",
            repo.checkout.to_str().unwrap(),
        ])
        .assert()
}

#[test]
fn lightweight_and_annotated_tags_on_main_are_accepted() {
    let repo = Repository::new();
    repo.tag("26.8.19-hotfix.14", false);
    verify(&repo, "26.8.19-hotfix.14").success();

    repo.tag("26.8.19-hotfix.15", true);
    verify(&repo, "26.8.19-hotfix.15").success();
}

#[test]
fn a_tag_on_an_unmerged_side_branch_is_rejected() {
    let repo = Repository::new();
    git(&repo.checkout, &["switch", "-c", "unmerged-release"]);
    std::fs::write(repo.checkout.join("release.txt"), "side branch\n").expect("write fixture");
    git(&repo.checkout, &["add", "release.txt"]);
    git(&repo.checkout, &["commit", "-m", "unmerged release"]);
    repo.tag("26.8.19-hotfix.14", true);

    verify(&repo, "26.8.19-hotfix.14")
        .failure()
        .stderr(predicates::str::contains("not reachable from origin/main"));
}
