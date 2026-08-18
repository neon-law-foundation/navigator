//! Prove that a release tag targets a commit already reachable from `main`.
//!
//! A Git commit carries no branch name, and Git permits a person to tag an
//! unmerged side-branch commit. Release publication therefore cannot infer
//! provenance from the pushed tag event. This command refreshes `origin/main`,
//! peels either an annotated or lightweight tag to a commit, and asks Git's own
//! ancestry engine whether that commit has passed through `main`.

use std::path::Path;
use std::process::{Command, ExitCode, Output};

use anyhow::{bail, Context, Result};

/// Entry point for `ops release-provenance`.
pub fn run(repo: &Path, tag: &str) -> ExitCode {
    match verify(repo, tag) {
        Ok(commit) => {
            println!(
                "navigator: release tag {tag} resolves to {commit} and is reachable from origin/main"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("navigator: release-provenance: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn verify(repo: &Path, tag: &str) -> Result<String> {
    crate::devx::registry::validate_release_tag(tag)?;

    // Write the fetched main tip to the canonical remote-tracking ref. The
    // force marker handles a rewritten test remote and is harmless for the
    // protected production branch; this command changes no local branch.
    git_checked(
        repo,
        &[
            "fetch",
            "--no-tags",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
        ],
        "fetch origin/main",
    )?;

    // `^{commit}` peels an annotated tag and leaves a lightweight tag's commit
    // unchanged. Prefixing the full tag ref avoids treating a tag-like option
    // or another ref namespace as the release source.
    let revision = format!("refs/tags/{tag}^{{commit}}");
    let resolved = git_output(repo, &["rev-parse", "--verify", "--quiet", &revision])
        .with_context(|| format!("resolve release tag `{tag}` to a commit"))?;
    if !resolved.status.success() {
        bail!("release tag `{tag}` does not resolve to a commit");
    }
    let commit = String::from_utf8(resolved.stdout)
        .context("release commit is not UTF-8")?
        .trim()
        .to_string();
    if commit.is_empty() {
        bail!("release tag `{tag}` resolved to an empty commit id");
    }

    let ancestry = git_output(
        repo,
        &["merge-base", "--is-ancestor", &commit, "origin/main"],
    )
    .context("test release commit reachability from origin/main")?;
    match ancestry.status.code() {
        Some(0) => Ok(commit),
        Some(1) => bail!(
            "release tag `{tag}` targets commit {commit}, which is not reachable from origin/main; merge its PR before creating or publishing the release"
        ),
        Some(code) => bail!(
            "git merge-base could not test release provenance (exit {code}): {}",
            String::from_utf8_lossy(&ancestry.stderr).trim()
        ),
        None => bail!("git merge-base was terminated before it tested release provenance"),
    }
}

fn git_checked(repo: &Path, args: &[&str], action: &str) -> Result<()> {
    let output = git_output(repo, args).with_context(|| action.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "{action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn git_output(repo: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git").arg("-C").arg(repo).args(args).output()
}
