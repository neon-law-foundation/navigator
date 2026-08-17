//! Repo-hygiene guard: planning lives in GitHub issues, not a local
//! drafts directory. The old convention — a gitignored top-level
//! folder of kickoff briefs — is deprecated and removed; this test
//! fails if any tracked file reintroduces a path reference to it, so
//! the convention cannot silently creep back. See AGENTS.md,
//! "Planning lives in GitHub issues".

use std::process::Command;

/// The deprecated directory's path token, assembled at runtime so this
/// test file does not itself contain the literal string and self-match
/// under `git grep`.
fn deprecated_dir_token() -> String {
    format!("{}{}", "prompts", "/")
}

#[test]
fn no_tracked_file_references_the_deprecated_planning_drafts_dir() {
    let token = deprecated_dir_token();
    let workspace_root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

    // Scan every tracked file in the working tree for the path token.
    let out = Command::new("git")
        .arg("grep")
        .arg("--no-color")
        .arg("-n")
        .arg("-I")
        .arg("-F")
        .arg(&token)
        .current_dir(workspace_root)
        .output()
        .expect("run `git grep` to scan tracked files");

    match out.status.code() {
        // `git grep` exits 1 with no output when there are no matches —
        // the invariant holds.
        Some(1) => {}
        // Exit 0 means matches were found — the invariant is violated.
        Some(0) => {
            let hits = String::from_utf8_lossy(&out.stdout);
            panic!(
                "Planning now lives in GitHub issues; the `{token}` drafts directory is deprecated \
                 and removed. Update these tracked references to point at the relevant GitHub issue \
                 (or drop them):\n{hits}"
            );
        }
        other => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!("`git grep` failed (exit {other:?}): {stderr}");
        }
    }
}
