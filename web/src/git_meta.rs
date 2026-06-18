//! Git-derived per-file metadata. Today: one helper that asks `git`
//! when a file last changed.
//!
//! Used by long-lived content pages (mission, future blog/help/about
//! signals) to render a "Last edited in main: YYYY-MM-DD" footer line.
//!
//! # Production caveat
//!
//! The production Dockerfile bundles `.git/` via `.dockerignore`-NOT,
//! and the runtime image is `gcr.io/distroless/static`, which carries
//! no `git` binary. So at request time in production this helper
//! returns `None`. That is intentional: callers must treat the date as
//! optional and omit the "Last edited" line when unknown.
//!
//! A follow-up commit will compute the index at build time (so the
//! production binary ships with the dates baked in). Until then the
//! footer line is a local-dev / KIND-only nicety.

use std::path::Path;
use std::process::Command;

use chrono::NaiveDate;

/// Return the date of the most recent commit that touched `path`, or
/// `None` if git can't answer (no `.git`, no `git` binary, file not
/// tracked, command failure, malformed output, etc.). Never panics.
///
/// Uses `git -C <parent> log -1 --format=%cI -- <name>` which yields a
/// strict ISO-8601 committer date like `2026-05-22T10:14:33-07:00`.
/// We keep just the `YYYY-MM-DD` head — visitors don't need seconds,
/// and the timezone offset isn't meaningful to render as wall time.
///
/// Running git from the file's *parent* directory (`-C`) is what lets
/// callers pass an absolute path to a file in a repo other than the
/// process's current working directory — git walks up from `-C` to
/// find the right `.git`. Without this, a call from the workspace root
/// targeting a file in a separate repo (or a tempdir test repo) would
/// silently consult the wrong history.
#[must_use]
pub fn last_touched(path: &Path) -> Option<NaiveDate> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(parent)
        .args(["log", "-1", "--format=%cI", "--no-show-signature", "--"])
        .arg(name)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let iso = stdout.trim();
    if iso.is_empty() {
        return None;
    }
    // Keep the leading YYYY-MM-DD. `parse_from_str` with "%Y-%m-%d"
    // refuses trailing characters, so split on 'T' first.
    let date_head = iso.split('T').next()?;
    NaiveDate::parse_from_str(date_head, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::last_touched;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    /// `git init` + minimal identity so commit works without depending
    /// on the host's `~/.gitconfig`.
    fn init_repo(dir: &Path) {
        Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .current_dir(dir)
            .status()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .status()
            .expect("git config name");
        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(dir)
            .status()
            .expect("git config gpgsign");
    }

    fn commit(dir: &Path, rel: &str, contents: &str, msg: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        Command::new("git")
            .args(["add", rel])
            .current_dir(dir)
            .status()
            .expect("git add");
        Command::new("git")
            .args(["commit", "--quiet", "-m", msg])
            .current_dir(dir)
            .status()
            .expect("git commit");
    }

    #[test]
    fn returns_some_for_a_committed_file_in_a_repo() {
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        commit(tmp.path(), "page.md", "hello", "add page");
        let date = last_touched(&tmp.path().join("page.md"));
        let today = chrono::Utc::now().date_naive();
        // `git log %cI` uses the committer date; on a just-made commit
        // this is now() rounded to a whole second, so it must equal
        // today within ±1 day to handle the rare UTC-midnight crossing.
        let date = date.expect("git log returned a date for the committed file");
        let diff = (today - date).num_days().abs();
        assert!(diff <= 1, "date {date} not within 1 day of today {today}");
    }

    #[test]
    fn returns_none_for_a_path_with_no_git_history() {
        // File exists on disk but was never committed → git log produces
        // no output → helper returns None.
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        let path = tmp.path().join("uncommitted.md");
        fs::write(&path, "draft").unwrap();
        assert_eq!(last_touched(&path), None);
    }

    #[test]
    fn returns_none_for_a_path_outside_any_repo() {
        // tempdir is not a git repo and not inside one.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("orphan.md");
        fs::write(&path, "no repo here").unwrap();
        assert_eq!(last_touched(&path), None);
    }

    #[test]
    fn updates_when_the_file_is_recommitted() {
        // Two commits to the same path: the helper returns the second
        // commit's date (it walks `git log -1`, newest first).
        let tmp = TempDir::new().unwrap();
        init_repo(tmp.path());
        commit(tmp.path(), "p.md", "v1", "initial");
        let first = last_touched(&tmp.path().join("p.md")).expect("first commit dated");
        commit(tmp.path(), "p.md", "v2", "edit");
        let second = last_touched(&tmp.path().join("p.md")).expect("second commit dated");
        assert!(
            second >= first,
            "second commit date {second} should be >= first {first}",
        );
    }
}
