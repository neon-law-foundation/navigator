//! `navigator dev sample-project` — clone, build, and stage the reference
//! project application.
//!
//! The `simpsons` demo matter carries the client portal at
//! `/app/projects/simpsons/portal/`. Development boot clones the repository
//! recorded on that Project, builds it with `pnpm`, and stages the resulting
//! `dist/` before writing the environment that `web` reads.
//!
//! The URL comes from the Project rather than a constant here, so pointing a
//! matter at a different forge — or standing up a second Project's application
//! — is a data change. `--repo` still overrides it for a fork or a local
//! mirror.
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
//! Nothing here runs in production: only the local `dev` boot path stages it.
//!
//! ## Testing
//!
//! Cloning and `pnpm` shell out to the network and to Node, so [`run`]'s
//! sequencing is not unit-tested. Everything that *decides* something is
//! extracted so it can be: which repository to clone ([`choose_repo`]), the git
//! arguments, the two preconditions a contributor actually trips
//! ([`require_lockfile`], [`built_bundle`]), the staged path, and the tree copy.
//! What is left in `run` is the order of the shell-outs.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The Project whose application this command stages.
///
/// One demo matter carries a portal locally, so the code is fixed here while
/// the *repository* is not: that is read from the Project row, which is what
/// makes a second Project's application a data change rather than a code
/// change.
const PROJECT_CODE: &str = "simpsons";

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

/// Whether the store was even consulted, so the "no URL" error can say which
/// of the two absences it is.
enum Lookup {
    /// No row for [`PROJECT_CODE`] at all.
    NoProject,
    /// The row exists and carries this `repository_url`.
    Project(Option<String>),
}

/// Choose the repository to clone, given the flag and what the store holds.
///
/// The pure half of [`resolve_repo`]: every branch a caller can land in is
/// decided here, so the IO wrapper stays a connect-and-read with no decisions
/// of its own. `--repo` wins without a lookup, and each absence names its own
/// fix rather than falling back to a compiled-in upstream.
fn choose_repo(explicit: Option<&str>, lookup: impl FnOnce() -> Result<Lookup>) -> Result<String> {
    if let Some(repo) = explicit {
        return Ok(repo.to_string());
    }
    match lookup()? {
        Lookup::NoProject => bail!(
            "no Project `{PROJECT_CODE}` in this store — start `web` once so the dev seed \
             runs, or pass `--repo`"
        ),
        Lookup::Project(None) => bail!(
            "Project `{PROJECT_CODE}` records no repository URL. Set one on the matter, or \
             pass `--repo`."
        ),
        Lookup::Project(Some(url)) => Ok(url),
    }
}

/// The repository to clone: `--repo` when given, else the URL recorded on the
/// Project.
///
/// Reading the Project is what keeps one source of truth. The decision lives in
/// [`choose_repo`]; this only supplies the store.
fn resolve_repo(explicit: Option<&str>) -> Result<String> {
    choose_repo(explicit, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create tokio runtime")?;
        runtime.block_on(async {
            let surreal = store::surreal::connect_from_env().await.context(
                "connect to SurrealDB to read the Project's repository URL — source \
                 this worktree's `.devx/env` first, or pass `--repo`",
            )?;
            Ok(
                match store::projects::find_by_code(&surreal, PROJECT_CODE)
                    .await
                    .with_context(|| format!("look up Project `{PROJECT_CODE}`"))?
                {
                    None => Lookup::NoProject,
                    Some(project) => Lookup::Project(project.repository_url),
                },
            )
        })
    })
}

/// The manifest text and the Project code it declares.
///
/// Read *before* a build is spent on the checkout: a bundle declaring the wrong
/// Project is refused at boot anyway, so finding out here saves an install. The
/// text is returned alongside the code because it is staged verbatim next to the
/// bundle — boot re-reads it rather than trusting whoever staged it.
fn declared_project(checkout: &Path) -> Result<(String, String)> {
    let manifest_path = checkout.join(store::sample_project::MANIFEST_FILE);
    let manifest = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — a project application declares its Project there",
            manifest_path.display()
        )
    })?;
    let code = store::sample_project::project_code_from_manifest(&manifest)?;
    Ok((manifest, code))
}

/// Refuse a checkout with no lockfile, before spending an install on it.
///
/// `--frozen-lockfile` is what keeps the build reproducible, so this says
/// plainly what is wrong rather than letting `pnpm` fail with its own less
/// specific message about a missing lockfile it was told not to write.
fn require_lockfile(checkout: &Path, repo: &str) -> Result<()> {
    if checkout.join("pnpm-lock.yaml").is_file() {
        return Ok(());
    }
    bail!(
        "{repo} has no pnpm-lock.yaml, so its dependencies cannot be resolved \
         reproducibly. Commit a lockfile there (which needs every dependency \
         to be resolvable — see its README) and re-run."
    )
}

/// The built bundle inside a checkout, proven to be one.
///
/// Both absences are a failed build rather than a partial one, and they are
/// reported separately because they have different causes: no `dist/` means the
/// build script did not run or writes elsewhere, while a `dist/` with no entry
/// document means it ran and produced assets nothing can point at. Publishing
/// the latter would strand the live bundle.
fn built_bundle(checkout: &Path) -> Result<PathBuf> {
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
    Ok(built)
}

/// Refresh the Simpsons application for a local boot.
pub fn run(repo: Option<&str>, git_ref: Option<&str>, keep: bool) -> Result<()> {
    super::require_tools(&["git", "pnpm"])?;
    let repo = &resolve_repo(repo)?;
    let workspace_root = super::orchestrate::workspace_root()?;

    let (stage, copied) = run_for_root(repo, git_ref, keep, &workspace_root)?;
    print!("{}", staging_instructions(copied, &stage));
    Ok(())
}

/// Clone, build, and stage the one local Simpsons application for `root`.
///
/// The development orchestrator calls this before it renders `.devx/env`, so
/// the following `web` process always reads the freshly staged bundle.
pub(super) fn run_for_root(
    repo: &str,
    git_ref: Option<&str>,
    keep: bool,
    workspace_root: &Path,
) -> Result<(PathBuf, usize)> {
    super::require_tools(&["git", "pnpm"])?;

    // The checkout and the build live in a temp tree; only `dist/` survives.
    let temp = tempfile::Builder::new()
        .prefix("navigator-sample-project-")
        .tempdir()
        .context("creating a temporary build directory")?;
    let checkout = temp.path().join("checkout");

    println!("navigator: cloning {repo}");
    let args = clone_args(repo, git_ref, &checkout);
    run_in(temp.path(), "git", &args)?;

    let (manifest, code) = declared_project(&checkout)?;
    println!(
        "navigator: {} declares Project `{code}`",
        repo_basename(repo)
    );

    require_lockfile(&checkout, repo)?;

    println!("navigator: installing dependencies (pnpm)");
    run_in(
        &checkout,
        "pnpm",
        &["install".to_string(), "--frozen-lockfile".to_string()],
    )?;

    println!("navigator: building the bundle (pnpm build)");
    run_in(&checkout, "pnpm", &["build".to_string()])?;

    let built = built_bundle(&checkout)?;

    // Stage the manifest beside the bundle: boot re-reads the declared Project
    // rather than trusting whoever set the environment variable.
    let stage = staged_root(workspace_root);
    let copied = copy_tree(&built, &stage.join(store::sample_project::DIST_DIR))?;
    std::fs::write(stage.join(store::sample_project::MANIFEST_FILE), &manifest)
        .with_context(|| format!("staging the manifest in {}", stage.display()))?;

    if keep {
        // Leak the TempDir so the tree survives for inspection.
        let path = temp.keep();
        println!("navigator: kept the build tree at {}", path.display());
    }

    Ok((stage, copied))
}

/// What to tell the operator once the bundle is staged.
///
/// Built as a string so the refresh output is covered by a focused test.
fn staging_instructions(copied: usize, stage: &Path) -> String {
    format!(
        "\nnavigator: staged {copied} file(s) to {}\n\n\
         The next `web` boot reads it from the generated `.devx/env`.\n",
        stage.display(),
    )
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

    /// `--repo` wins outright, and does not consult the store at all.
    ///
    /// The lookup panics if called: a flag that still needed a database would
    /// make the command unusable on a cold checkout, which is the one case the
    /// flag exists for.
    #[test]
    fn an_explicit_repo_wins_without_reading_the_store() {
        let chosen = choose_repo(Some("https://example.test/a-fork/x.git"), || {
            panic!("the store must not be consulted when --repo is given")
        })
        .expect("a repo");
        assert_eq!(chosen, "https://example.test/a-fork/x.git");
    }

    /// With no flag, the Project's own recorded URL is what gets cloned —
    /// whatever forge it names.
    #[test]
    fn the_projects_recorded_url_is_cloned_when_no_flag_is_given() {
        let chosen = choose_repo(None, || {
            Ok(Lookup::Project(Some(
                "https://gitlab.example/a-group/a-project.git".to_string(),
            )))
        })
        .expect("a repo");
        assert_eq!(chosen, "https://gitlab.example/a-group/a-project.git");
    }

    /// The two absences are different problems, so they get different messages.
    ///
    /// Neither falls back to a compiled-in upstream: this command carries no
    /// default, and a silent one would clone somebody else's repository onto a
    /// matter's portal.
    #[test]
    fn each_absence_names_its_own_fix_rather_than_falling_back() {
        let no_project = choose_repo(None, || Ok(Lookup::NoProject))
            .expect_err("a store with no such Project is an error");
        let message = no_project.to_string();
        assert!(
            message.contains("start `web` once") && message.contains("--repo"),
            "the no-Project error must name both fixes: {message}"
        );

        let no_url = choose_repo(None, || Ok(Lookup::Project(None)))
            .expect_err("a Project with no repository URL is an error");
        let message = no_url.to_string();
        assert!(
            message.contains("records no repository URL"),
            "the no-URL error must say the column is empty: {message}"
        );
        assert!(
            !message.contains("github.com"),
            "no default upstream may appear in the error: {message}"
        );
    }

    /// A failed lookup propagates rather than being read as "no URL recorded".
    ///
    /// Otherwise an unreachable database would produce the *set one on the
    /// matter* advice, sending the reader to fix a row that is probably fine.
    #[test]
    fn a_failed_lookup_propagates_instead_of_becoming_an_absence() {
        let error = choose_repo(None, || anyhow::bail!("connection refused"))
            .expect_err("a lookup failure is an error");
        assert!(
            error.to_string().contains("connection refused"),
            "the underlying failure must survive: {error}"
        );
    }

    /// A missing or unusable manifest is refused before a build is spent, and
    /// the text is returned verbatim for staging.
    #[test]
    fn the_declared_project_is_read_before_a_build_is_spent() {
        let dir = tempfile::tempdir().expect("tempdir");

        let missing =
            declared_project(dir.path()).expect_err("a checkout with no manifest is refused");
        assert!(
            missing
                .to_string()
                .contains(store::sample_project::MANIFEST_FILE),
            "the refusal must name the file a bundle declares its Project in: {missing}"
        );

        // Present but naming something that is not a Project code.
        std::fs::write(
            dir.path().join(store::sample_project::MANIFEST_FILE),
            b"name: \"../etc\"\n",
        )
        .expect("write");
        declared_project(dir.path()).expect_err("a manifest cannot smuggle a path segment");

        std::fs::write(
            dir.path().join(store::sample_project::MANIFEST_FILE),
            b"name: simpsons\n",
        )
        .expect("write");
        let (manifest, code) = declared_project(dir.path()).expect("a valid manifest");
        assert_eq!(code, "simpsons");
        assert_eq!(
            manifest, "name: simpsons\n",
            "the text is staged verbatim, so it must come back unaltered"
        );
    }

    /// A checkout with no lockfile is refused before an install is spent on it,
    /// and the message names the repository so a contributor knows where to
    /// commit one.
    #[test]
    fn a_checkout_without_a_lockfile_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = require_lockfile(dir.path(), "https://forge.example/o/r")
            .expect_err("a checkout with no lockfile must be refused");
        let message = error.to_string();
        assert!(
            message.contains("https://forge.example/o/r") && message.contains("pnpm-lock.yaml"),
            "the refusal must name the repository and the file: {message}"
        );

        std::fs::write(dir.path().join("pnpm-lock.yaml"), b"lockfileVersion: '9.0'")
            .expect("write");
        require_lockfile(dir.path(), "https://forge.example/o/r").expect("a lockfile is enough");
    }

    /// The two failed-build shapes are reported separately, because they have
    /// different causes and different fixes.
    #[test]
    fn a_failed_build_names_which_half_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");

        let no_dist = built_bundle(dir.path()).expect_err("no dist/ is a failed build");
        assert!(
            no_dist.to_string().contains("no `dist/`"),
            "{no_dist}: a missing dist must point at the build script"
        );

        // A `dist/` full of assets but with no entry document: the build ran and
        // produced files nothing can point at.
        let dist = dir.path().join(store::sample_project::DIST_DIR);
        std::fs::create_dir_all(dist.join("assets")).expect("mkdir");
        std::fs::write(dist.join("assets/app-abc123.js"), b"x").expect("write");
        let no_entry = built_bundle(dir.path()).expect_err("no index.html is a failed build");
        assert!(
            no_entry
                .to_string()
                .contains(store::sample_project::ENTRY_DOCUMENT),
            "{no_entry}: a missing entry document must be named"
        );

        std::fs::write(
            dist.join(store::sample_project::ENTRY_DOCUMENT),
            b"<!doctype html>",
        )
        .expect("write");
        assert_eq!(
            built_bundle(dir.path()).expect("a complete build"),
            dist,
            "a dist/ with an entry document is the bundle to stage"
        );
    }

    /// The refresh output names the staged path and the generated environment.
    #[test]
    fn the_instructions_name_the_key_boot_reads_and_the_staged_path() {
        let text = staging_instructions(5, Path::new("/w/.devx/sample-project"));
        assert!(
            text.contains("The next `web` boot reads it from the generated `.devx/env`."),
            "{text}"
        );
        assert!(text.contains("staged 5 file(s)"), "{text}");
    }

    /// `run_in` reports a failing command and a missing one differently.
    ///
    /// These are the two failures an operator hits — a `pnpm build` that exits
    /// nonzero, and a `pnpm` that is not installed — and they need different
    /// fixes, so the missing-program case carries the "is it installed" hint
    /// rather than an exit status.
    #[test]
    fn a_failing_command_and_a_missing_one_are_reported_differently() {
        let dir = tempfile::tempdir().expect("tempdir");

        run_in(dir.path(), "true", &[]).expect("a succeeding command is Ok");

        let failed = run_in(dir.path(), "false", &[]).expect_err("a nonzero exit must fail");
        assert!(
            failed.to_string().contains("failed in"),
            "a nonzero exit must name where it ran: {failed}"
        );

        let missing = run_in(dir.path(), "navigator-no-such-program", &[])
            .expect_err("a missing program must fail");
        assert!(
            missing.to_string().contains("is it installed and on PATH?"),
            "a missing program must say so rather than report an exit code: {missing}"
        );
    }

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
