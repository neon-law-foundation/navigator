//! `navigator dev sample-project` — clone, build, and stage the reference
//! project application.
//!
//! The `simpsons` demo matter carries a client portal at
//! `/app/projects/simpsons/portal/`. By default `web` boot publishes a stub
//! compiled into the binary, which needs no network and no Node. This command
//! is the opt-in upgrade: it clones
//! [navigator-sample-project](https://github.com/neon-law-foundation/navigator-sample-project),
//! builds it with `pnpm`, and stages the resulting `dist/` where the next boot
//! will publish it instead.
//!
//! The clone and the build happen in a **temporary directory** that is removed
//! when the command returns — a build tree is derived, so keeping it in the
//! worktree would only invite editing the wrong copy. Two things survive into
//! `.devx/sample-project/`: the built `dist/`, and the `navigator.yml` that
//! declares which Project the bundle mounts on. Boot re-reads that manifest
//! rather than trusting whoever set the environment variable, so the pair
//! travels together. `--keep` retains the temp tree for debugging a failed
//! build.
//!
//! Nothing here runs in production: the seed reads
//! [`store::sample_project::STAGE_ENV`], and only this command sets it.
//!
//! ## Testing
//!
//! Cloning and `pnpm` shell out to the network and to Node, so the
//! orchestration is not unit-tested. Everything that can silently drift — the
//! git arguments, the staged path, and the tree copy — is pure and covered
//! below.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The canonical upstream. Overridable so a contributor can point at a fork
/// without editing source.
pub const DEFAULT_REPO: &str =
    "https://github.com/neon-law-foundation/navigator-sample-project.git";

/// Where the project is staged, relative to the workspace root. Inside
/// `.devx/` because it is generated, per-checkout, and already ignored.
const STAGE_RELATIVE: [&str; 1] = ["sample-project"];

/// Build the `git clone` arguments. Always a shallow, single-branch clone: the
/// history is not wanted, only the tree that builds.
///
/// A pinned `git_ref` still clones shallow — `--branch` accepts a tag or a
/// branch name — so the common case stays one round trip.
fn clone_args(repo: &str, git_ref: Option<&str>, dest: &Path) -> Vec<String> {
    let mut args = vec![
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--single-branch".to_string(),
    ];
    if let Some(reference) = git_ref {
        args.push("--branch".to_string());
        args.push(reference.to_string());
    }
    args.push(repo.to_string());
    args.push(dest.display().to_string());
    args
}

/// Where the project is staged for the next `web` boot — the manifest and the
/// built bundle together.
fn staged_root(workspace_root: &Path) -> PathBuf {
    let mut path = workspace_root.join(".devx");
    for segment in STAGE_RELATIVE {
        path.push(segment);
    }
    path
}

/// Copy a directory tree, replacing `dst` wholesale.
///
/// Replacing rather than merging is deliberate: a merge would leave assets
/// from a previous build in the staged tree, and boot publishes everything it
/// finds, so stale files would be republished forever.
fn copy_tree(src: &Path, dst: &Path) -> Result<usize> {
    if dst.exists() {
        std::fs::remove_dir_all(dst)
            .with_context(|| format!("clearing the staged bundle at {}", dst.display()))?;
    }
    let mut copied = 0;
    copy_into(src, dst, &mut copied)?;
    Ok(copied)
}

fn copy_into(src: &Path, dst: &Path, copied: &mut usize) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("creating {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let metadata = std::fs::metadata(&from)?;
        if metadata.is_dir() {
            copy_into(&from, &to, copied)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
            *copied += 1;
        }
    }
    Ok(())
}

/// Run one command in `dir`, failing loudly. Output is inherited so a `pnpm`
/// build's own diagnostics reach the operator instead of being swallowed into
/// a captured buffer nobody prints.
fn run_in(dir: &Path, program: &str, args: &[String]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("running `{program}` — is it installed and on PATH?"))?;
    if !status.success() {
        bail!(
            "`{program} {}` failed in {} ({status})",
            args.join(" "),
            dir.display()
        );
    }
    Ok(())
}

/// `navigator dev sample-project`: clone, build, stage.
pub fn run(repo: &str, git_ref: Option<&str>, keep: bool) -> Result<()> {
    super::require_tools(&["git", "pnpm"])?;
    let workspace_root = super::orchestrate::workspace_root()?;

    // The checkout and the build live in a temp tree; only `dist/` survives.
    let temp = tempfile::Builder::new()
        .prefix("navigator-sample-project-")
        .tempdir()
        .context("creating a temporary build directory")?;
    let checkout = temp.path().join("checkout");

    println!("navigator: cloning {repo}");
    let args = clone_args(repo, git_ref, &checkout);
    run_in(temp.path(), "git", &args)?;

    // Read the Project *before* spending a build on it: a bundle that declares
    // the wrong Project would be refused at boot anyway.
    let manifest_path = checkout.join(store::sample_project::MANIFEST_FILE);
    let manifest = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — a project application declares its Project there",
            manifest_path.display()
        )
    })?;
    let code = store::sample_project::project_code_from_manifest(&manifest)?;
    println!(
        "navigator: {} declares Project `{code}`",
        repo_basename(repo)
    );

    // `--frozen-lockfile` keeps the build reproducible, so say plainly what is
    // wrong rather than letting pnpm fail with its own less specific message.
    if !checkout.join("pnpm-lock.yaml").is_file() {
        bail!(
            "{repo} has no pnpm-lock.yaml, so its dependencies cannot be resolved \
             reproducibly. Commit a lockfile there (which needs every dependency \
             to be resolvable — see its README) and re-run."
        );
    }

    println!("navigator: installing dependencies (pnpm)");
    run_in(
        &checkout,
        "pnpm",
        &["install".to_string(), "--frozen-lockfile".to_string()],
    )?;

    println!("navigator: building the bundle (pnpm build)");
    run_in(&checkout, "pnpm", &["build".to_string()])?;

    let built = checkout.join(store::sample_project::DIST_DIR);
    if !built.is_dir() {
        bail!(
            "the build produced no `dist/` at {} — check the repository's build script",
            built.display()
        );
    }
    if !built.join(store::sample_project::ENTRY_DOCUMENT).is_file() {
        bail!(
            "the build produced no `{}` — Navigator publishes nothing without an entry document",
            store::sample_project::ENTRY_DOCUMENT
        );
    }

    // Stage the manifest beside the bundle: boot re-reads the declared Project
    // rather than trusting whoever set the environment variable.
    let stage = staged_root(&workspace_root);
    let copied = copy_tree(&built, &stage.join(store::sample_project::DIST_DIR))?;
    std::fs::write(stage.join(store::sample_project::MANIFEST_FILE), &manifest)
        .with_context(|| format!("staging the manifest in {}", stage.display()))?;

    if keep {
        // Leak the TempDir so the tree survives for inspection.
        let path = temp.keep();
        println!("navigator: kept the build tree at {}", path.display());
    }

    println!();
    println!("navigator: staged {copied} file(s) to {}", stage.display());
    println!();
    println!("Point the next `web` boot at it, then restart `web`:");
    println!();
    println!(
        "    export {}={}",
        store::sample_project::STAGE_ENV,
        stage.display()
    );
    println!();
    println!("Unset it to go back to the compiled stub.");
    Ok(())
}

/// The repository's last path segment, for a readable progress line.
fn repo_basename(repo: &str) -> &str {
    repo.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repo)
        .trim_end_matches(".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_is_shallow_and_single_branch() {
        let args = clone_args("https://example.com/r.git", None, Path::new("/tmp/x"));
        assert_eq!(
            args,
            vec![
                "clone",
                "--depth",
                "1",
                "--single-branch",
                "https://example.com/r.git",
                "/tmp/x"
            ]
        );
    }

    #[test]
    fn a_pinned_ref_becomes_branch_and_stays_shallow() {
        let args = clone_args("r.git", Some("v1.2.3"), Path::new("/tmp/x"));
        assert!(args.contains(&"--branch".to_string()));
        assert!(args.contains(&"v1.2.3".to_string()));
        assert_eq!(
            args.iter().filter(|a| *a == "--depth").count(),
            1,
            "pinning a ref must not cost a full history"
        );
    }

    #[test]
    fn staged_root_lives_under_devx() {
        assert_eq!(
            staged_root(Path::new("/w")),
            PathBuf::from("/w/.devx/sample-project")
        );
    }

    #[test]
    fn repo_basename_reads_through_the_git_suffix_and_trailing_slash() {
        assert_eq!(
            repo_basename("https://github.com/o/navigator-sample-project.git"),
            "navigator-sample-project"
        );
        // SCP-style remotes split on the same `/` as a URL path.
        assert_eq!(repo_basename("git@github.com:o/r.git"), "r");
        assert_eq!(repo_basename("https://x/y/"), "y");
    }

    #[test]
    fn copy_tree_replaces_rather_than_merges() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(src.join("assets")).expect("mkdir");
        std::fs::write(src.join("index.html"), b"new").expect("write");
        std::fs::write(src.join("assets/app-new.js"), b"new").expect("write");

        // A previous build left an asset that the new one does not have.
        std::fs::create_dir_all(dst.join("assets")).expect("mkdir");
        std::fs::write(dst.join("assets/app-old.js"), b"old").expect("write");

        let copied = copy_tree(&src, &dst).expect("copy");

        assert_eq!(copied, 2);
        assert!(dst.join("assets/app-new.js").is_file());
        assert!(
            !dst.join("assets/app-old.js").exists(),
            "a stale asset would be republished on every boot"
        );
    }

    #[test]
    fn copy_tree_preserves_nested_structure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(src.join("assets/fonts")).expect("mkdir");
        std::fs::write(src.join("index.html"), b"x").expect("write");
        std::fs::write(src.join("assets/fonts/gorp.woff2"), b"x").expect("write");

        copy_tree(&src, &dst).expect("copy");

        assert!(dst.join("assets/fonts/gorp.woff2").is_file());
    }
}
