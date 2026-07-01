//! Per-Project git repositories — the bare-repo store.
//!
//! Every Project is one **append-only, single-branch (`main`)** bare git
//! repository, served Rust-native from `web`. This crate owns where
//! those repos physically live on the volume and how they are created
//! with the append-only invariant baked in, so a misconfigured client
//! cannot violate it. See [the design](../../docs/git-project-repos.md).
//!
//! The store shells to the `git` binary (`feedback_infra_kind_gke`: lean
//! on the mature upstream rather than reimplement pack negotiation). The
//! same binary backs the smart-HTTP transport in `web`.
//!
//! ## The append-only invariant
//!
//! Each bare repo is created with three guards, two via config and one
//! via a hook, so every write path is covered:
//!
//! - `receive.denyNonFastForwards = true` — no history rewrite / force push.
//! - `receive.denyDeletes = true` — no ref deletion.
//! - a `pre-receive` hook that rejects any ref other than
//!   `refs/heads/main` — no second branch, no tags.
//!
//! The only operation that bypasses this is the admin **governed
//! expunge** (privilege clawback / sealing / lawful deletion), which
//! acts on the bare repo directly and is never a push.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use uuid::Uuid;

/// The single ref every Project repo carries.
pub const DEFAULT_BRANCH: &str = "main";

/// Who a server-side commit is attributed to. The commit log *is* the
/// matter's audit trail (design §7), so a commit made on a person's
/// behalf — a portal upload, an inbound-email attachment, an e-sign
/// completion — is authored as *that* `persons` identity, not a generic
/// service account.
#[derive(Clone, Copy, Debug)]
pub struct Author<'a> {
    /// The person's display name (`persons.name`).
    pub name: &'a str,
    /// The person's email (`persons.email`).
    pub email: &'a str,
}

/// Result of a governed expunge: the head commit before and after the
/// history rewrite (both `None` only if the repo was empty), and the
/// path that was removed.
#[derive(Clone, Debug)]
pub struct ExpungeOutcome {
    /// `refs/heads/main` before the rewrite.
    pub head_before: Option<String>,
    /// `refs/heads/main` after the rewrite (a new oid — history changed).
    pub head_after: Option<String>,
    /// The repo-relative path that was removed from all of history.
    pub path: String,
}

/// Env var naming the POSIX directory that holds every Project's bare
/// repo. The volume path is deploy-specific (an RWO PVC mount in prod,
/// a tmp/hostPath dir in KIND) and never hard-coded.
pub const REPO_ROOT_ENV: &str = "NAVIGATOR_GIT_REPO_ROOT";

/// Git LFS routing seeded into every repo's `info/attributes` at
/// creation (design §5). PDFs, docx, and images go through Git LFS —
/// their bytes live in `cloud::StorageService` (see `web::git_lfs`)
/// while only a small pointer rides the pack history. Writing this
/// server-side means `git check-attr` (and diff/archive) route binaries
/// to LFS on a fresh, empty repo, before any commit exists.
const LFS_ATTRIBUTES: &str = "\
*.pdf filter=lfs diff=lfs merge=lfs -text\n\
*.docx filter=lfs diff=lfs merge=lfs -text\n\
*.png filter=lfs diff=lfs merge=lfs -text\n\
*.jpg filter=lfs diff=lfs merge=lfs -text\n\
*.jpeg filter=lfs diff=lfs merge=lfs -text\n";

/// The append-only `pre-receive` hook. Rejects any pushed ref that is
/// not `refs/heads/main`; the config guards (`denyNonFastForwards`,
/// `denyDeletes`) cover force-push and deletion of `main` itself.
const PRE_RECEIVE_HOOK: &str = "#!/bin/sh\n\
# Neon Law Navigator: matter repos are append-only and single-branch (main).\n\
# Only fast-forward additions to refs/heads/main are accepted; the\n\
# repo config rejects non-fast-forward updates and deletions.\n\
while read -r _old _new ref; do\n\
\tif [ \"$ref\" != \"refs/heads/main\" ]; then\n\
\t\techo \"navigator: this matter repo is single-branch; only refs/heads/main may be pushed (got $ref)\" >&2\n\
\t\texit 1\n\
\tfi\n\
done\n\
exit 0\n";

/// Errors creating or locating a Project's bare repo.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// [`REPO_ROOT_ENV`] is unset, so we don't know where repos live.
    #[error("git repo root env {REPO_ROOT_ENV} is not set")]
    RootUnset,
    /// A `git` subprocess exited non-zero. Carries the command and its
    /// stderr so the caller can log a precise failure.
    #[error("git {command} failed: {stderr}")]
    Git { command: String, stderr: String },
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The public clone URL for a Project: `<base>/projects/<id>.git`.
///
/// `base` is the deployment's public origin (`https://www.your-domain.example`)
/// — never hard-coded. The path matches the smart-HTTP route `web` serves,
/// so the URL is valid whether the bare repo was provisioned eagerly at
/// matter open or lazily on first access.
#[must_use]
pub fn clone_url(base: &str, project_id: Uuid) -> String {
    format!("{}/projects/{project_id}.git", base.trim_end_matches('/'))
}

/// Where every Project's bare repo lives, and how they are created.
///
/// Cheap to clone (`root` is a `PathBuf`); construct once at boot and
/// share. Methods are synchronous — repo creation is rare and fast — so
/// the async `web` layer wraps the occasional call in `spawn_blocking`.
#[derive(Clone, Debug)]
pub struct RepoStore {
    root: PathBuf,
}

impl RepoStore {
    /// Construct rooted at an explicit directory (tests, or a caller
    /// that already resolved the volume path).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Construct from [`REPO_ROOT_ENV`].
    ///
    /// # Errors
    /// [`RepoError::RootUnset`] when the env var is absent.
    pub fn from_env() -> Result<Self, RepoError> {
        let root = std::env::var(REPO_ROOT_ENV).map_err(|_| RepoError::RootUnset)?;
        Ok(Self::new(root))
    }

    /// The directory holding every Project's bare repo.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Filesystem path of a Project's bare repo: `<root>/<id>.git`. The
    /// `.git` suffix matches the public URL shape
    /// (`/projects/<id>.git`).
    #[must_use]
    pub fn path_for(&self, project_id: Uuid) -> PathBuf {
        self.root.join(format!("{project_id}.git"))
    }

    /// Whether the Project's bare repo has been created.
    #[must_use]
    pub fn exists(&self, project_id: Uuid) -> bool {
        self.path_for(project_id).join("HEAD").is_file()
    }

    /// Create the Project's append-only single-branch bare repo if it is
    /// absent, and return its path. Idempotent: an existing repo is
    /// returned untouched, so this is safe to call on every git request.
    ///
    /// # Errors
    /// [`RepoError::Git`] if a `git` invocation fails, or
    /// [`RepoError::Io`] on a filesystem error.
    pub fn ensure(&self, project_id: Uuid) -> Result<PathBuf, RepoError> {
        let path = self.path_for(project_id);
        if path.join("HEAD").is_file() {
            // Self-heal the LFS routing on every access: the guard above
            // keys on `HEAD`, which `git init --bare` writes *before* the
            // routing is seeded, so a repo whose creation failed after
            // `HEAD` (or one predating LFS seeding) would otherwise be
            // served forever without routing. `ensure_lfs_attributes`
            // only writes when the file is absent or wrong.
            ensure_lfs_attributes(&path)?;
            return Ok(path);
        }
        std::fs::create_dir_all(&self.root)?;
        let path_str = path.to_string_lossy().into_owned();

        run_git(&[
            "init",
            "--bare",
            "--initial-branch",
            DEFAULT_BRANCH,
            &path_str,
        ])?;
        // Append-only guards (force-push + deletion).
        run_git(&[
            "-C",
            &path_str,
            "config",
            "receive.denyNonFastForwards",
            "true",
        ])?;
        run_git(&["-C", &path_str, "config", "receive.denyDeletes", "true"])?;
        // Let the smart-HTTP transport accept pushes to this repo.
        run_git(&["-C", &path_str, "config", "http.receivepack", "true"])?;
        install_pre_receive_hook(&path)?;
        ensure_lfs_attributes(&path)?;

        tracing::info!(%project_id, path = %path_str, "created append-only bare repo");
        Ok(path)
    }

    /// The commit `main` currently points at, or `None` if the repo has
    /// no commits yet (unborn branch).
    ///
    /// # Errors
    /// [`RepoError::Git`] / [`RepoError::Io`] on failure.
    pub fn head_oid(&self, project_id: Uuid) -> Result<Option<String>, RepoError> {
        let repo = self.ensure(project_id)?;
        let repo_str = repo.to_string_lossy().into_owned();
        // `rev-parse --verify` exits non-zero on an unborn branch, which
        // we map to `None` rather than an error.
        let out = Command::new("git")
            .args([
                "-C",
                &repo_str,
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/heads/main",
            ])
            .output()?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ))
    }

    /// Every file at `main`'s tip as `(path, bytes)`, recursively.
    ///
    /// This is the "give the client their current files" read path: it
    /// returns the working-tree contents of HEAD with their human,
    /// `/`-separated paths — never packfiles, bundles, or history. An
    /// unborn `main` (no commits yet) yields an empty list. Paths are
    /// read NUL-delimited so spaces or newlines in a filename are safe.
    ///
    /// # Errors
    /// [`RepoError::Git`] / [`RepoError::Io`] on failure.
    pub fn read_head_tree(&self, project_id: Uuid) -> Result<Vec<(String, Vec<u8>)>, RepoError> {
        if self.head_oid(project_id)?.is_none() {
            return Ok(Vec::new());
        }
        let repo = self.ensure(project_id)?;
        let repo_str = repo.to_string_lossy().into_owned();

        let listing = capture(
            &[
                "-C",
                &repo_str,
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                "refs/heads/main",
            ],
            &[],
            None,
        )?;

        let mut files = Vec::new();
        for raw in listing.split(|b| *b == 0) {
            if raw.is_empty() {
                continue;
            }
            let path = String::from_utf8_lossy(raw).into_owned();
            let spec = format!("refs/heads/main:{path}");
            let bytes = capture(&["-C", &repo_str, "cat-file", "-p", &spec], &[], None)?;
            files.push((path, bytes));
        }
        Ok(files)
    }

    /// Append a commit to `main`, authored as `author`, that adds or
    /// updates `files` (each `(path, bytes)`), and return the new commit
    /// oid. This is the server-side write path — portal upload, inbound
    /// email, e-sign completion — so `git log` faithfully records who
    /// did what.
    ///
    /// Uses git plumbing against the bare repo (no worktree) and only
    /// ever fast-forwards `main`, so it never violates the append-only
    /// invariant. Existing files not named in `files` are carried
    /// forward from the parent commit.
    ///
    /// # Errors
    /// [`RepoError::Git`] / [`RepoError::Io`] on failure.
    pub fn commit_as(
        &self,
        project_id: Uuid,
        author: Author<'_>,
        message: &str,
        files: &[(&str, &[u8])],
    ) -> Result<String, RepoError> {
        let repo = self.ensure(project_id)?;
        let repo_str = repo.to_string_lossy().into_owned();

        // A private index file scoped to this commit, so concurrent
        // writers don't share the bare repo's default index.
        let index = repo.join(format!("index-commit-{}", Uuid::now_v7()));
        let index_str = index.to_string_lossy().into_owned();
        let index_env = [("GIT_INDEX_FILE", index_str.as_str())];

        let parent = self.head_oid(project_id)?;

        // Seed the index from the parent tree so unchanged files survive.
        if let Some(ref p) = parent {
            capture(&["-C", &repo_str, "read-tree", p], &index_env, None)?;
        }

        for (path, bytes) in files {
            let oid_raw = capture(
                &[
                    "-C",
                    &repo_str,
                    "hash-object",
                    "-w",
                    "--path",
                    path,
                    "--stdin",
                ],
                &[],
                Some(bytes),
            )?;
            let oid = String::from_utf8_lossy(&oid_raw).trim().to_string();
            let cacheinfo = format!("100644,{oid},{path}");
            capture(
                &[
                    "-C",
                    &repo_str,
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &cacheinfo,
                ],
                &index_env,
                None,
            )?;
        }

        let tree_raw = capture(&["-C", &repo_str, "write-tree"], &index_env, None)?;
        let tree = String::from_utf8_lossy(&tree_raw).trim().to_string();

        // Author *and* committer are the acting person, so both fields
        // of `git log` name them. Global/system config is neutralized.
        let commit_env = [
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
            ("GIT_AUTHOR_NAME", author.name),
            ("GIT_AUTHOR_EMAIL", author.email),
            ("GIT_COMMITTER_NAME", author.name),
            ("GIT_COMMITTER_EMAIL", author.email),
        ];
        let mut commit_args = vec!["-C", &repo_str, "commit-tree", &tree];
        if let Some(ref p) = parent {
            commit_args.push("-p");
            commit_args.push(p);
        }
        commit_args.push("-m");
        commit_args.push(message);
        let commit_raw = capture(&commit_args, &commit_env, None)?;
        let commit = String::from_utf8_lossy(&commit_raw).trim().to_string();

        // Fast-forward main to the new commit.
        capture(
            &["-C", &repo_str, "update-ref", "refs/heads/main", &commit],
            &[],
            None,
        )?;

        let _ = std::fs::remove_file(&index);
        tracing::info!(%project_id, author = author.email, commit = %commit, "server-side commit");
        Ok(commit)
    }

    /// **Governed expunge** — the one operation that is *not*
    /// append-only. Permanently removes `path` from every commit in the
    /// repo's history, drops the backup refs, expires reflogs, and
    /// `gc`s so the blob becomes unreachable and is pruned. Returns the
    /// head before/after the rewrite.
    ///
    /// Reserved for a privilege clawback, a sealing order, or a client's
    /// lawful deletion request, and only ever invoked by an admin (the
    /// caller enforces that — see `web::expunge`). It does not delete any
    /// Git LFS object the path may point at; the orchestrating layer
    /// removes that from `cloud::StorageService` separately.
    ///
    /// Rewriting history changes every commit oid from the point the
    /// file was introduced, so **existing clones are invalidated** and a
    /// later push from one would be rejected as non-fast-forward — a
    /// deliberate, documented consequence of a lawful expunge.
    ///
    /// # Errors
    /// [`RepoError::Git`] / [`RepoError::Io`] on failure.
    pub fn expunge_path(&self, project_id: Uuid, path: &str) -> Result<ExpungeOutcome, RepoError> {
        let repo = self.ensure(project_id)?;
        let repo_str = repo.to_string_lossy().into_owned();
        let head_before = self.head_oid(project_id)?;

        // `git rm` the path out of every tree across all refs. The
        // index-filter runs in a shell inside filter-branch; single-quote
        // the path (escaping embedded quotes) so spaces are safe.
        let escaped = path.replace('\'', "'\\''");
        let index_filter = format!("git rm --cached --ignore-unmatch -- '{escaped}'");
        capture(
            &[
                "-C",
                &repo_str,
                "filter-branch",
                "--force",
                "--prune-empty",
                "--index-filter",
                &index_filter,
                "--",
                "--all",
            ],
            &[("FILTER_BRANCH_SQUELCH_WARNING", "1")],
            None,
        )?;

        // filter-branch leaves the pre-rewrite refs under refs/original/;
        // delete them, expire reflogs, and gc so the old blobs are gone.
        let originals = capture(
            &[
                "-C",
                &repo_str,
                "for-each-ref",
                "--format=%(refname)",
                "refs/original/",
            ],
            &[],
            None,
        )?;
        for refname in String::from_utf8_lossy(&originals).lines() {
            if !refname.is_empty() {
                capture(&["-C", &repo_str, "update-ref", "-d", refname], &[], None)?;
            }
        }
        // Bare repos usually keep no reflog; ignore failure here.
        let _ = capture(
            &["-C", &repo_str, "reflog", "expire", "--expire=now", "--all"],
            &[],
            None,
        );
        capture(&["-C", &repo_str, "gc", "--prune=now"], &[], None)?;

        let head_after = self.head_oid(project_id)?;
        tracing::warn!(%project_id, path, "governed expunge: rewrote history to remove a path");
        Ok(ExpungeOutcome {
            head_before,
            head_after,
            path: path.to_string(),
        })
    }
}

/// Write the append-only `pre-receive` hook and mark it executable.
fn install_pre_receive_hook(repo: &Path) -> Result<(), RepoError> {
    let hooks = repo.join("hooks");
    std::fs::create_dir_all(&hooks)?;
    let hook = hooks.join("pre-receive");
    std::fs::write(&hook, PRE_RECEIVE_HOOK)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Seed the repo's server-side `info/attributes` with the Git LFS
/// routing rules ([`LFS_ATTRIBUTES`]) unless they are already present and
/// current. Idempotent and self-healing: it restores routing for a repo
/// created before LFS seeding, or one whose creation failed after `HEAD`
/// was written (a truncated or stale file is rewritten too). `git init
/// --bare` already creates the `info/` directory, but `create_dir_all`
/// keeps this robust.
fn ensure_lfs_attributes(repo: &Path) -> Result<(), RepoError> {
    let info = repo.join("info");
    let attributes = info.join("attributes");
    if std::fs::read_to_string(&attributes).is_ok_and(|c| c == LFS_ATTRIBUTES) {
        return Ok(());
    }
    std::fs::create_dir_all(&info)?;
    std::fs::write(&attributes, LFS_ATTRIBUTES)?;
    Ok(())
}

/// Run `git <args>`, mapping a non-zero exit to [`RepoError::Git`].
fn run_git(args: &[&str]) -> Result<(), RepoError> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(RepoError::Git {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

/// Run `git <args>` with extra env vars and optional stdin, returning
/// stdout on success. The plumbing path ([`RepoStore::commit_as`]) needs
/// the captured oids and a private `GIT_INDEX_FILE`.
fn capture(
    args: &[&str],
    envs: &[(&str, &str)],
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, RepoError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cmd.spawn()?;
    if let Some(data) = stdin {
        child.stdin.take().expect("stdin piped").write_all(data)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(RepoError::Git {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn clone_url_joins_without_double_slash() {
        let id = Uuid::nil();
        // A trailing slash on `base` must not produce `//projects`.
        assert_eq!(
            clone_url("https://www.example.test/", id),
            "https://www.example.test/projects/00000000-0000-0000-0000-000000000000.git"
        );
        assert_eq!(
            clone_url("https://www.example.test", id),
            "https://www.example.test/projects/00000000-0000-0000-0000-000000000000.git"
        );
    }

    /// Run a git command in `dir`, isolated from the developer's global
    /// / system config and with a deterministic identity, so the test is
    /// hermetic. Returns the exit status plus captured stderr.
    fn git(dir: &Path, args: &[&str]) -> (bool, String) {
        let out = Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Libra")
            .env("GIT_AUTHOR_EMAIL", "libra@example.com")
            .env("GIT_COMMITTER_NAME", "Libra")
            .env("GIT_COMMITTER_EMAIL", "libra@example.com")
            .args(args)
            .output()
            .expect("run git");
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// Clone the bare repo, make one commit on `main`, push it. Returns
    /// the working-clone dir so the test can attempt further pushes.
    fn clone_commit_push(store: &RepoStore, project: Uuid, work_parent: &Path) -> TempDir {
        let bare = store.path_for(project);
        let work = TempDir::new().unwrap();
        let (ok, err) = git(
            work_parent,
            &[
                "clone",
                bare.to_str().unwrap(),
                work.path().to_str().unwrap(),
            ],
        );
        assert!(ok, "clone failed: {err}");
        std::fs::write(work.path().join("hello.txt"), "hi").unwrap();
        assert!(git(work.path(), &["add", "hello.txt"]).0);
        assert!(git(work.path(), &["commit", "-m", "first"]).0);
        let (ok, err) = git(work.path(), &["push", "origin", "main"]);
        assert!(ok, "push main failed: {err}");
        work
    }

    #[test]
    fn ensure_creates_single_branch_bare_repo_idempotently() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();

        assert!(!store.exists(project));
        let path = store.ensure(project).unwrap();
        assert!(store.exists(project));
        assert!(path.join("HEAD").is_file());

        // HEAD points at the one branch we allow.
        let head = std::fs::read_to_string(path.join("HEAD")).unwrap();
        assert_eq!(head.trim(), "ref: refs/heads/main");

        // Idempotent: a second call returns the same path, no error.
        let again = store.ensure(project).unwrap();
        assert_eq!(again, path);
    }

    /// Resolve the `filter` attribute git applies to `path` in the bare
    /// repo — `git check-attr` reads `info/attributes` server-side, so
    /// this answers "is this path routed through LFS?" on a fresh, empty
    /// repo without git-lfs installed.
    fn check_filter(repo: &Path, path: &str) -> String {
        let out = Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .args([
                "-C",
                repo.to_str().unwrap(),
                "check-attr",
                "filter",
                "--",
                path,
            ])
            .output()
            .expect("run git check-attr");
        assert!(
            out.status.success(),
            "git check-attr failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Output shape: `<path>: filter: <value>`; take the value.
        String::from_utf8_lossy(&out.stdout)
            .rsplit(": ")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    #[test]
    fn ensure_seeds_lfs_routing_for_binaries() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();
        let repo = store.ensure(project).unwrap();

        // Every binary type §5 names routes to the lfs filter on a fresh,
        // empty repo — no commit, no git-lfs binary required.
        for name in [
            "deed.pdf",
            "brief.docx",
            "seal.png",
            "scan.jpg",
            "photo.jpeg",
        ] {
            assert_eq!(
                check_filter(&repo, name),
                "lfs",
                "{name} should route through Git LFS"
            );
        }

        // Text and other files are untouched — LFS is for binaries only.
        assert_eq!(check_filter(&repo, "notes.txt"), "unspecified");

        // Idempotent: a second ensure leaves routing in place.
        store.ensure(project).unwrap();
        assert_eq!(check_filter(&repo, "deed.pdf"), "lfs");

        // Self-heal: `ensure`'s idempotency guard keys on `HEAD`, which
        // exists before routing is seeded — so a repo left without
        // routing (a creation that failed after `HEAD`, or one predating
        // LFS seeding) must have it restored on the next access rather
        // than served unrouted forever.
        std::fs::remove_file(repo.join("info").join("attributes")).unwrap();
        assert_eq!(check_filter(&repo, "deed.pdf"), "unspecified");
        store.ensure(project).unwrap();
        assert_eq!(check_filter(&repo, "deed.pdf"), "lfs");
    }

    #[test]
    fn accepts_a_fast_forward_commit_to_main() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();
        store.ensure(project).unwrap();

        let parent = TempDir::new().unwrap();
        let _work = clone_commit_push(&store, project, parent.path());
        // A push that succeeded is the assertion (inside the helper).
    }

    #[test]
    fn rejects_a_second_branch() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();
        store.ensure(project).unwrap();

        let parent = TempDir::new().unwrap();
        let work = clone_commit_push(&store, project, parent.path());

        // Pushing any ref other than main is rejected by the hook.
        let (ok, err) = git(work.path(), &["push", "origin", "HEAD:refs/heads/feature"]);
        assert!(!ok, "a second branch must be rejected");
        assert!(
            err.contains("single-branch"),
            "expected hook message, got: {err}"
        );
    }

    /// Read `git log`/`show` output from the bare repo with isolated
    /// config.
    fn show(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .args([&["-C", repo.to_str().unwrap()], args].concat())
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn commit_as_attributes_to_the_person_and_chains() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();

        // Unborn branch → None.
        assert_eq!(store.head_oid(project).unwrap(), None);

        // First commit, authored as Libra.
        let c1 = store
            .commit_as(
                project,
                Author {
                    name: "Libra",
                    email: "libra@example.com",
                },
                "add will",
                &[("will.txt", b"the last will")],
            )
            .unwrap();
        let repo = store.path_for(project);
        assert_eq!(
            store.head_oid(project).unwrap().as_deref(),
            Some(c1.as_str())
        );
        assert_eq!(
            show(&repo, &["log", "-1", "--format=%an <%ae>"]),
            "Libra <libra@example.com>"
        );
        assert_eq!(show(&repo, &["show", "main:will.txt"]), "the last will");

        // Second commit by a different person adds a file; the first
        // file survives and the history chains onto c1.
        let c2 = store
            .commit_as(
                project,
                Author {
                    name: "Nick",
                    email: "nick@neonlaw.com",
                },
                "add trust",
                &[("trust.txt", b"the trust")],
            )
            .unwrap();
        assert_eq!(show(&repo, &["log", "-1", "--format=%an"]), "Nick");
        assert_eq!(show(&repo, &["show", "main:will.txt"]), "the last will");
        assert_eq!(show(&repo, &["show", "main:trust.txt"]), "the trust");
        assert_eq!(show(&repo, &["rev-parse", "main~1"]), c1);
        assert_ne!(c1, c2);
    }

    #[test]
    fn read_head_tree_returns_every_current_file_and_empty_when_unborn() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();

        // Unborn branch → no files.
        assert!(store.read_head_tree(project).unwrap().is_empty());

        store
            .commit_as(
                project,
                Author {
                    name: "Libra",
                    email: "libra@example.com",
                },
                "file two docs",
                &[
                    ("will.txt", b"the last will"),
                    ("folder/trust.pdf", b"the trust bytes"),
                ],
            )
            .unwrap();

        let mut files = store.read_head_tree(project).unwrap();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "folder/trust.pdf");
        assert_eq!(files[0].1, b"the trust bytes");
        assert_eq!(files[1].0, "will.txt");
        assert_eq!(files[1].1, b"the last will");
    }

    #[test]
    fn governed_expunge_removes_a_path_from_all_history() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();

        // One commit holding a privileged doc + a doc we keep.
        store
            .commit_as(
                project,
                Author {
                    name: "Libra",
                    email: "libra@example.com",
                },
                "file two docs",
                &[
                    ("privileged.txt", b"attorney-client privileged"),
                    ("keep.txt", b"ordinary matter doc"),
                ],
            )
            .unwrap();
        let repo = store.path_for(project);
        let before = store.head_oid(project).unwrap();

        let outcome = store.expunge_path(project, "privileged.txt").unwrap();

        // The kept doc survives; the privileged one is gone from HEAD,
        // from all history, and as a reachable object.
        assert_eq!(
            show(&repo, &["show", "main:keep.txt"]),
            "ordinary matter doc"
        );
        let head_files = show(&repo, &["ls-tree", "-r", "--name-only", "main"]);
        assert!(
            !head_files.contains("privileged.txt"),
            "expunged file still in HEAD tree: {head_files}"
        );
        let history = show(
            &repo,
            &["log", "--all", "--oneline", "--", "privileged.txt"],
        );
        assert!(
            history.is_empty(),
            "expunged file still appears in history: {history}"
        );
        let objects = show(&repo, &["rev-list", "--all", "--objects"]);
        assert!(
            !objects.contains("privileged.txt"),
            "expunged blob still reachable"
        );

        // History was rewritten — the head moved.
        assert_eq!(outcome.head_before, before);
        assert_ne!(outcome.head_after, before);
        assert!(outcome.head_after.is_some());
    }

    #[test]
    fn rejects_force_push_and_branch_deletion() {
        let root = TempDir::new().unwrap();
        let store = RepoStore::new(root.path());
        let project = Uuid::now_v7();
        store.ensure(project).unwrap();

        let parent = TempDir::new().unwrap();
        let work = clone_commit_push(&store, project, parent.path());

        // Rewrite history and force-push: denyNonFastForwards rejects it.
        std::fs::write(work.path().join("hello.txt"), "rewritten").unwrap();
        assert!(git(work.path(), &["commit", "--amend", "-m", "rewrite"]).0);
        let (ok, _) = git(work.path(), &["push", "--force", "origin", "main"]);
        assert!(!ok, "force push (non-fast-forward) must be rejected");

        // Deleting main: denyDeletes rejects it.
        let (ok, _) = git(work.path(), &["push", "origin", "--delete", "main"]);
        assert!(!ok, "deleting main must be rejected");
    }
}
