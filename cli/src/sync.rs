//! `navigator site sync` — mirror the matters this login participates in
//! into a folder tree on disk, one folder per matter code.
//!
//! The tree is the surface for people who work a matter in an editor
//! rather than in the browser: `~/Projects/<project-code>/` is a real
//! directory you can open in Claude Code, and `~/Projects/CLAUDE.md`
//! (mirrored as `AGENTS.md`) is the standing instruction every agent
//! session inherits when it opens one of those folders, because agents
//! read guidance files up the directory tree.
//!
//! Sync is a **read** of `GET /app/projects.csv` — the same
//! participation-scoped list `site projects list` prints, resolved by
//! `store::access::visible_projects_as_lawyer`. There is no parallel API
//! and no client-side authorization: the server decides what you can see,
//! and sync writes exactly that.
//!
//! Two rules keep the tree safe to re-run:
//!
//! - **Sync owns three paths and nothing else.** `CLAUDE.md` and
//!   `AGENTS.md` at the root, and `README.md` inside each matter folder.
//!   Everything else in a matter folder is yours and is never read,
//!   moved, or rewritten.
//! - **Sync never deletes.** A folder whose matter is no longer visible —
//!   the matter closed, or your participation ended — is reported and
//!   left alone. Removing a folder that may hold your local work is a
//!   decision for you, not for a sync command.
//!
//! Every managed file is a pure function of the matter row, with no
//! timestamp in the body, so re-running against an unchanged matter
//! rewrites nothing and the report reads honestly.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The banner every managed file opens with, so a reader (human or agent)
/// knows the file is derived and hand edits will not survive.
const MANAGED_BANNER: &str =
    "<!-- Managed by `navigator site sync`. Edits are overwritten on the next run. -->";

/// One matter, as `GET /app/projects.csv` renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matter {
    pub id: String,
    pub code: String,
    pub name: String,
    pub status: String,
    pub entity_name: String,
}

/// What one `sync` run did, so the command can report it and a test can
/// assert on it without re-reading the tree.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Matter codes whose folder did not exist before this run.
    pub created: Vec<String>,
    /// Matter codes whose folder existed and whose managed files changed.
    pub refreshed: Vec<String>,
    /// Matter codes that were already exactly right.
    pub unchanged: Vec<String>,
    /// Root-level managed files (`CLAUDE.md`, `AGENTS.md`) this run wrote.
    pub guides_written: Vec<String>,
    /// Directory names under the root that match no visible matter. Left
    /// on disk, never deleted.
    pub unmatched: Vec<String>,
}

impl SyncReport {
    /// Total matters the run accounted for.
    pub fn matters(&self) -> usize {
        self.created.len() + self.refreshed.len() + self.unchanged.len()
    }
}

/// `~/Projects` — the default tree root.
pub fn default_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("cannot locate your home directory (HOME is unset) — pass --root")?;
    Ok(home.join("Projects"))
}

/// Read `projects.csv` rows into matters, resolving columns by header
/// name so a future column added server-side does not shift the parse.
pub fn matters_from_csv(rows: &[Vec<String>]) -> Result<Vec<Matter>> {
    let Some((header, data)) = rows.split_first() else {
        return Ok(Vec::new());
    };
    let col = |want: &str| -> Result<usize> {
        header
            .iter()
            .position(|h| h == want)
            .with_context(|| format!("projects.csv is missing a `{want}` column"))
    };
    let (id, code, name, status, entity) = (
        col("id")?,
        col("code")?,
        col("name")?,
        col("status")?,
        col("entity_name")?,
    );
    let at = |row: &[String], i: usize| row.get(i).cloned().unwrap_or_default();
    Ok(data
        .iter()
        // A trailing newline yields one empty record; it is not a matter.
        .filter(|row| row.iter().any(|f| !f.is_empty()))
        .map(|row| Matter {
            id: at(row, id),
            code: at(row, code),
            name: at(row, name),
            status: at(row, status),
            entity_name: at(row, entity),
        })
        .collect())
}

/// Write the whole tree under `root` for `matters` fetched from `base`.
pub fn sync_tree(root: &Path, base: &str, matters: &[Matter]) -> Result<SyncReport> {
    fs::create_dir_all(root)
        .with_context(|| format!("create the projects root at {}", root.display()))?;

    let mut report = SyncReport::default();

    // The root guide, written to both filenames. Two real files rather
    // than a symlink: some tools skip symlinks when walking a tree, and a
    // guide an agent silently never reads is worse than a duplicated one.
    let guide = root_guide(base);
    for filename in ["CLAUDE.md", "AGENTS.md"] {
        if write_if_changed(&root.join(filename), &guide)? {
            report.guides_written.push(filename.to_string());
        }
    }

    let mut managed_dirs = BTreeSet::new();
    for matter in matters {
        let dir_name = vfs::name::sanitize(&matter.code);
        managed_dirs.insert(dir_name.clone());
        let dir = root.join(&dir_name);
        let is_new = !dir.exists();
        fs::create_dir_all(&dir)
            .with_context(|| format!("create the matter folder at {}", dir.display()))?;
        let changed = write_if_changed(&dir.join("README.md"), &matter_readme(base, matter))?;
        if is_new {
            report.created.push(matter.code.clone());
        } else if changed {
            report.refreshed.push(matter.code.clone());
        } else {
            report.unchanged.push(matter.code.clone());
        }
    }

    report.unmatched = unmatched_dirs(root, &managed_dirs)?;
    Ok(report)
}

/// Directory names directly under `root` that no visible matter claims.
/// Reported so you can see what the server no longer shows you; never
/// removed, because the folder may hold local work sync did not put there.
fn unmatched_dirs(root: &Path, managed: &BTreeSet<String>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in
        fs::read_dir(root).with_context(|| format!("read the tree at {}", root.display()))?
    {
        let entry = entry.context("read a directory entry")?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // Dotfiles are editor and tooling state, not matters.
        if name.starts_with('.') || managed.contains(&name) {
            continue;
        }
        out.push(name);
    }
    out.sort();
    Ok(out)
}

/// Write `body` to `path` only when it differs, returning whether it did.
/// Re-running sync on an unchanged matter must not churn mtimes — editors
/// and file watchers treat a rewrite as a real edit.
fn write_if_changed(path: &Path, body: &str) -> Result<bool> {
    if fs::read_to_string(path).is_ok_and(|existing| existing == body) {
        return Ok(false);
    }
    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

/// The standing instruction at the root of the tree. Agent tools read
/// guidance files up the directory tree, so this one applies to every
/// matter folder opened beneath it — which is the whole reason it lives at
/// the root rather than being copied into each folder.
pub fn root_guide(base: &str) -> String {
    format!(
        r"{MANAGED_BANNER}

# Matters

This tree mirrors the matters you participate in on {base}. One folder per matter, named by its
matter code, created by `navigator site sync`.

## What is confidential here

Everything in a matter folder is client material: confidential under Rule 1.6 and, for most of it,
privileged. Treat the whole tree that way.

- Do not copy matter content into any tool, model, or service the firm has not approved for client
  data, and do not paste it into a public issue, gist, chat, or search box.
- Do not commit these folders to a repository. Matter content lives in Navigator and in the firm's
  Workspace drive; the copy on your laptop is a working copy, not a system of record.
- Do not move content between matter folders. Each folder is one client's matter, and the wall
  between them is the point.

## What is authoritative

This tree is a convenience surface, not the record. The matter's authoritative state — its workflow
step, its notations, its filed and client-visible documents, its audit trail — lives on {base}. Each
folder's `README.md` links to that matter's workbench. When the folder and the site disagree, the
site is right and the folder is stale; re-run sync.

Navigator does not sign, file, or send anything on its own, and neither does an agent working in
this tree. Those are acts a licensed attorney takes deliberately, through Navigator.

## What sync owns

`navigator site sync` writes exactly three kinds of file and overwrites them on every run:

- `CLAUDE.md` and `AGENTS.md` at this root (identical content — this file).
- `README.md` inside each matter folder.

Everything else in a matter folder is yours. Sync never deletes anything: if a folder here no longer
matches a matter you can see, sync reports it and leaves it in place for you to decide about.

Refresh the tree with:

```bash
navigator site sync
```
"
    )
}

/// The matter card written into each folder — the fields the site's own
/// project list carries, plus the link back to the authoritative surface.
pub fn matter_readme(base: &str, matter: &Matter) -> String {
    let Matter {
        id,
        code,
        name,
        status,
        entity_name,
    } = matter;
    let entity = if entity_name.is_empty() {
        "(none recorded)"
    } else {
        entity_name.as_str()
    };
    format!(
        r"{MANAGED_BANNER}

# {name}

| | |
| --- | --- |
| Matter code | `{code}` |
| Status | {status} |
| Entity | {entity} |
| Workbench | {base}/app/projects/{id} |

Client material — confidential, and mostly privileged. The confidentiality and handling rules for
this whole tree are in the `CLAUDE.md` beside this folder; they apply here.

The workbench link above is this matter's authoritative state: its workflow step, notations,
documents, and audit trail. This file is a derived card, refreshed by `navigator site sync`.
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn henderson() -> Matter {
        Matter {
            id: "3f2a1c88-0000-4000-8000-000000000001".into(),
            code: "henderson-bungalow".into(),
            name: "Henderson Bungalow Purchase".into(),
            status: "open".into(),
            entity_name: "Henderson Holdings LLC".into(),
        }
    }

    fn deed() -> Matter {
        Matter {
            id: "3f2a1c88-0000-4000-8000-000000000002".into(),
            code: "virgo-deed".into(),
            name: "Virgo Deed of Sale".into(),
            status: "open".into(),
            entity_name: String::new(),
        }
    }

    #[test]
    fn csv_is_read_by_header_name_not_position() {
        let rows = vec![
            vec![
                "status".to_string(),
                "id".to_string(),
                "entity_name".to_string(),
                "code".to_string(),
                "name".to_string(),
            ],
            vec![
                "open".to_string(),
                "the-id".to_string(),
                "Acme LLC".to_string(),
                "the-code".to_string(),
                "The Matter".to_string(),
            ],
        ];
        let matters = matters_from_csv(&rows).unwrap();
        assert_eq!(
            matters,
            vec![Matter {
                id: "the-id".into(),
                code: "the-code".into(),
                name: "The Matter".into(),
                status: "open".into(),
                entity_name: "Acme LLC".into(),
            }]
        );
    }

    #[test]
    fn csv_with_only_a_header_yields_no_matters() {
        let rows = vec![vec![
            "id".to_string(),
            "code".to_string(),
            "name".to_string(),
            "status".to_string(),
            "entity_name".to_string(),
        ]];
        assert!(matters_from_csv(&rows).unwrap().is_empty());
    }

    #[test]
    fn csv_missing_a_column_is_an_error_not_a_silent_blank() {
        let rows = vec![vec!["id".to_string(), "name".to_string()]];
        let err = matters_from_csv(&rows).unwrap_err();
        assert!(format!("{err:#}").contains("`code`"));
    }

    #[test]
    fn sync_creates_a_folder_and_readme_per_matter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        let report = sync_tree(&root, "https://example.test", &[henderson(), deed()]).unwrap();

        assert_eq!(report.created, vec!["henderson-bungalow", "virgo-deed"]);
        assert!(report.refreshed.is_empty());
        assert_eq!(report.matters(), 2);
        assert!(root.join("henderson-bungalow/README.md").is_file());
        assert!(root.join("virgo-deed/README.md").is_file());

        let card = fs::read_to_string(root.join("henderson-bungalow/README.md")).unwrap();
        assert!(card.contains("# Henderson Bungalow Purchase"));
        assert!(card.contains("`henderson-bungalow`"));
        assert!(
            card.contains("https://example.test/app/projects/3f2a1c88-0000-4000-8000-000000000001")
        );
    }

    #[test]
    fn sync_writes_both_root_guides_with_identical_content() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        let report = sync_tree(&root, "https://example.test", &[henderson()]).unwrap();

        assert_eq!(report.guides_written, vec!["CLAUDE.md", "AGENTS.md"]);
        let claude = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert_eq!(claude, agents);
        assert!(claude.contains("https://example.test"));
        assert!(claude.contains("navigator site sync"));
    }

    #[test]
    fn a_second_run_over_unchanged_matters_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        sync_tree(&root, "https://example.test", &[henderson()]).unwrap();

        let again = sync_tree(&root, "https://example.test", &[henderson()]).unwrap();
        assert!(again.created.is_empty());
        assert!(again.refreshed.is_empty());
        assert!(again.guides_written.is_empty());
        assert_eq!(again.unchanged, vec!["henderson-bungalow"]);
    }

    #[test]
    fn a_renamed_matter_refreshes_its_card_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        sync_tree(&root, "https://example.test", &[henderson()]).unwrap();

        let mut renamed = henderson();
        renamed.name = "Henderson Bungalow Purchase (amended)".into();
        let report = sync_tree(&root, "https://example.test", &[renamed]).unwrap();

        assert_eq!(report.refreshed, vec!["henderson-bungalow"]);
        assert!(report.created.is_empty());
        let card = fs::read_to_string(root.join("henderson-bungalow/README.md")).unwrap();
        assert!(card.contains("(amended)"));
    }

    #[test]
    fn sync_never_deletes_a_folder_it_no_longer_sees() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        sync_tree(&root, "https://example.test", &[henderson(), deed()]).unwrap();
        // Local work the user put in the folder themselves.
        fs::write(root.join("virgo-deed/notes.md"), "my working notes").unwrap();

        // Participation on the deed matter ends: it drops off the list.
        let report = sync_tree(&root, "https://example.test", &[henderson()]).unwrap();

        assert_eq!(report.unmatched, vec!["virgo-deed"]);
        assert!(root.join("virgo-deed/README.md").is_file());
        assert_eq!(
            fs::read_to_string(root.join("virgo-deed/notes.md")).unwrap(),
            "my working notes"
        );
    }

    #[test]
    fn unrelated_files_in_a_matter_folder_survive_a_resync() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        sync_tree(&root, "https://example.test", &[henderson()]).unwrap();
        let mine = root.join("henderson-bungalow/draft.md");
        fs::write(&mine, "# my draft").unwrap();

        sync_tree(&root, "https://example.test", &[henderson()]).unwrap();

        assert_eq!(fs::read_to_string(&mine).unwrap(), "# my draft");
    }

    #[test]
    fn a_matter_code_with_path_characters_stays_one_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        let mut sneaky = henderson();
        sneaky.code = "../escaped/nul".into();

        sync_tree(&root, "https://example.test", &[sneaky]).unwrap();

        // One directory directly under the root, and nothing written above it.
        let dirs: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            dirs.len(),
            1,
            "expected exactly one matter folder: {dirs:?}"
        );
        assert!(!tmp.path().join("escaped").exists());
    }

    #[test]
    fn dotfiles_at_the_root_are_not_reported_as_unmatched() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Projects");
        sync_tree(&root, "https://example.test", &[henderson()]).unwrap();
        fs::create_dir_all(root.join(".vscode")).unwrap();

        let report = sync_tree(&root, "https://example.test", &[henderson()]).unwrap();
        assert!(report.unmatched.is_empty());
    }

    #[test]
    fn a_matter_with_no_entity_still_renders_a_card() {
        let card = matter_readme("https://example.test", &deed());
        assert!(card.contains("(none recorded)"));
        assert!(card.contains(MANAGED_BANNER));
    }
}
