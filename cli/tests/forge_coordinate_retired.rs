//! No forge coordinate lives in Navigator's source.
//!
//! One invariant, two mechanical halves.
//!
//! **The six-organization vocabulary is retired.** Six deployment
//! organizations — `production-templates`, `staging-templates`, `nlf-templates`,
//! `production-applications`, `staging-applications`, `nlf-applications` —
//! collapsed to three, named for the entities they serve. Twelve files named at
//! least one of the six, and issues 02 through 11 removed them as a side effect
//! of their own work. A rename that wide is easy to half-finish and easy to
//! reintroduce: a doc paragraph pasted from an old one, a test fixture naming
//! the old organization. So the invariant is asserted rather than remembered,
//! the same way `brand_identifier_is_neon.rs` asserts that the brand identifier
//! is `neon`.
//!
//! **No forge host is a literal in the files that read forge configuration.**
//! This is the sharper half, and it is the defect the collapse removed:
//! `portal::config` read `NAVIGATOR_GIT_HOST` with a **public forge as the
//! default**, so an unset variable silently pointed every Project's clone URL at
//! a namespace the Firm does not control — while `ops github setup` deliberately
//! had no such fallback and documented why. Two crates, opposite rules, and the
//! permissive one was the one serving users.
//!
//! **No Project repository URL is composed at all.** A Project's source is a
//! whole URL stored on the row (`store::projects::Project::repository_url`), on
//! whatever forge hosts it, so the derivation those two halves used to police is
//! gone rather than merely configured. The one surviving host in configuration is
//! `ops github setup`'s authorization boundary, which governs *this* tenant's own
//! repositories and never names a client matter's source.
//!
//! # Why the second half is scoped rather than tree-wide
//!
//! `github.com` appears legitimately all over this tree: dependency URLs in
//! `Cargo.lock`, generated third-party notices, `api.github.com` in the webhook
//! receiver, documentation links. A tree-wide refusal would be unsatisfiable and
//! would therefore be switched off. So the refusal is scoped to the files that
//! actually *compose a Project repository coordinate* — the place a stray host
//! literal becomes a wrong URL in front of a user — and [`COORDINATE_SOURCES`]
//! is asserted to exist so the scoping cannot go stale silently.
//!
//! # Scope is this repository alone
//!
//! Project repositories are separate repositories with their own CI. Nothing
//! here polices their contents; this guard walks this tree and nothing else.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// The six retiring organization names. No allowlist: not one of them has a
/// legitimate surviving use, in source or in prose.
const RETIRED_ORGANIZATIONS: &[&str] = &[
    "production-templates",
    "staging-templates",
    "nlf-templates",
    "production-applications",
    "staging-applications",
    "nlf-applications",
];

/// Forge hosts that may not be spelled where a Project coordinate is composed.
const FORGE_HOSTS: &[&str] = &["github.com", "github.com"];

/// The files that compose, render, or verify a Project repository coordinate.
///
/// Every one of these took a host or an organization from a literal before the
/// collapse. Each must now read configuration instead, so none may spell a forge
/// host at all. The list is explicit rather than a glob because that is the
/// claim: *these* are the coordinate-composing surfaces, and adding one is a
/// deliberate act.
const COORDINATE_SOURCES: &[&str] = &[
    "cloud/src/workspace.rs",
    "portal/src/config.rs",
    "portal/src/project_portal.rs",
    "cli/src/projects/doctor.rs",
    "cli/src/projects/repository.rs",
    "cli/src/devx/github_setup.rs",
    ".github/actions/validate/action.yml",
];

/// Files exempt by provenance rather than by name: only this test, whose own
/// refusal lists are written in the things it forbids.
const SKIPPED_FILES: &[&str] = &["forge_coordinate_retired.rs"];

/// The workspace root (this test crate is `cli`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every file Git tracks, as a repo-relative path.
///
/// Asking Git rather than walking the filesystem answers all three traps a
/// tree-walking guard hits here at once:
///
/// - **`.git` is a *file*** inside a linked worktree checkout, not a directory,
///   so a walk that skips directories by name reads a path carrying the branch
///   name and fails on the reviewer's own branch.
/// - **`.claude/worktrees/` holds whole second copies of this tree.** One such
///   checkout contributed 1640 false hits to the previous guard. A guard that
///   fails only on the machine where someone is verifying a change is the worst
///   possible shape.
/// - **Scratch files in the working tree are not source.** A `/tmp`-style note
///   left in the checkout must not fail the run.
///
/// Git's index answers all three: it knows nothing about `target`,
/// `node_modules`, `.devx`, another worktree's files, or untracked scratch.
fn tracked_files() -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files", "-z"])
        .output()
        .expect("run `git ls-files`");
    assert!(
        output.status.success(),
        "`git ls-files` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    assert!(
        files.len() > 100,
        "expected a tracked file list, got {} entries — this guard would pass vacuously",
        files.len()
    );
    files
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether `haystack` references `retired` as a whole hyphen-delimited slug,
/// rather than embedding it inside a longer identifier.
///
/// The retired names are complete organization slugs (`staging-applications`).
/// The mandatory `<deployment>-applications` bucket lane (ENG-126) resolves to
/// names like `neon-law-stg-applications` that carry the substring while
/// being a different identifier, so a bare `contains` false-positives on the
/// bucket. Matching whole `[a-z0-9-]+` runs keeps the true invariant — no
/// retired ORGANIZATION survives — without flagging a bucket that merely shares
/// the `-applications` suffix.
fn names_retired_org(haystack: &str, retired: &str) -> bool {
    haystack
        .to_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .any(|run| run == retired)
}

/// Not one of the six organization names survives, anywhere Git tracks.
///
/// Prose counts. Two of the twelve files that named them were workshop content
/// rather than source, and a code-only grep misses exactly those.
#[test]
fn no_retired_organization_name_survives() {
    let mut hits = Vec::new();
    for path in tracked_files() {
        if SKIPPED_FILES.contains(&basename(&path)) {
            continue;
        }
        // A path can carry the name without any line doing so.
        for retired in RETIRED_ORGANIZATIONS {
            if names_retired_org(&path, retired) {
                hits.push(format!("{path}: path names a retired organization"));
            }
        }
        let Ok(body) = std::fs::read_to_string(repo_root().join(&path)) else {
            continue;
        };
        for (index, line) in body.lines().enumerate() {
            for retired in RETIRED_ORGANIZATIONS {
                if names_retired_org(line, retired) {
                    hits.push(format!("{path}:{}: {}", index + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "the six deployment organizations collapsed to three, and the surviving three are \
         configuration (NAVIGATOR_GITHUB_ORG) rather than names in source. Found {} \
         occurrence(s):\n  {}",
        hits.len(),
        hits.join("\n  ")
    );
}

/// Whether a line is a comment rather than code.
///
/// Comments are excluded on purpose, and the exclusion is narrow rather than
/// convenient: prose *about* a forge — that the Actions App ID differs per host,
/// that a handle from one host resolves nowhere on another — is exactly the
/// context a reader needs, and a comment cannot compose a URL. What this test is
/// for is the *executable* literal: `.unwrap_or_else(|| "github.com".into())`
/// was a real default serving real users. Organization names are held to the
/// stricter rule and refused in prose too, by the test above.
fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("--")
}

/// No forge host is a literal in the code that composes a Project coordinate.
///
/// The host is `NAVIGATOR_GIT_HOST`, read with no default. A literal in one of
/// these files is either a fallback nobody chose or a fixture asserting one
/// deployment's spelling, and both are how the permissive default got there the
/// first time.
#[test]
fn no_forge_host_is_a_literal_where_a_coordinate_is_composed() {
    let mut hits = Vec::new();
    for source in COORDINATE_SOURCES {
        let path = repo_root().join(source);
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (index, line) in body.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            let lowered = line.to_lowercase();
            for host in FORGE_HOSTS {
                if lowered.contains(host) {
                    hits.push(format!("{source}:{}: {}", index + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "every forge value these files read comes from configuration — NAVIGATOR_GITHUB_ORG and \
         NAVIGATOR_GIT_HOST, neither with a default — and a Project's own repository is a stored \
         URL, not a composed one. A forge host spelled here is either a fallback nobody chose or \
         a fixture pinning one deployment's spelling. Found {} occurrence(s):\n  {}",
        hits.len(),
        hits.join("\n  ")
    );
}

/// The scoped list has to stay real, or the second half quietly stops meaning
/// anything.
///
/// A path that no longer exists would be skipped by a `filter_map` somewhere and
/// the guard would keep passing while checking less than it claims — the same
/// failure mode as a misspelled allowlist entry.
#[test]
fn every_coordinate_source_still_exists_and_is_tracked() {
    let tracked: BTreeSet<String> = tracked_files().into_iter().collect();
    let missing: Vec<&&str> = COORDINATE_SOURCES
        .iter()
        .filter(|source| !tracked.contains(**source))
        .collect();
    assert!(
        missing.is_empty(),
        "these coordinate-composing sources are not tracked files; a renamed or deleted entry \
         makes the host check silently narrower than it claims: {missing:?}"
    );
}

/// Each surviving forge value is read from configuration, and neither supplies
/// a default.
///
/// This is the positive half. The two negative tests above prove no host is
/// *written down*; this one proves the values are actually *read*, so the guard
/// cannot be satisfied by a file that stopped reading configuration at all.
///
/// The two keys serve different purposes now and live in different crates:
/// `NAVIGATOR_GITHUB_ORG` is the organization this deployment's own automation
/// occupies, resolved by `cloud::workspace`; `NAVIGATOR_GIT_HOST` is the single
/// enterprise host `ops github setup` may write governance to. Neither composes
/// a Project's repository URL — see
/// [`no_project_repository_url_is_composed_from_a_project_code`].
#[test]
fn the_surviving_forge_values_are_read_from_configuration_with_no_default() {
    let workspace = std::fs::read_to_string(repo_root().join("cloud/src/workspace.rs"))
        .expect("read cloud/src/workspace.rs");
    assert!(
        workspace.contains("NAVIGATOR_GITHUB_ORG"),
        "cloud::workspace must read NAVIGATOR_GITHUB_ORG rather than naming an organization",
    );

    let governance = std::fs::read_to_string(repo_root().join("cli/src/devx/github_setup.rs"))
        .expect("read cli/src/devx/github_setup.rs");
    assert!(
        governance.contains("NAVIGATOR_GIT_HOST"),
        "ops github setup must read NAVIGATOR_GIT_HOST as its authorization boundary",
    );

    // A named deployment with no organization fails closed. Asserted against the
    // real resolver rather than against the file's text, because what matters is
    // the behaviour and not the spelling.
    let lookup = |key: &str| (key == "NAVIGATOR_GCP_PROJECT_ID").then(|| "neon-law".to_string());
    assert!(
        cloud::workspace::WorkspaceConfig::from_lookup(lookup).is_err(),
        "a named deployment with no organization must not resolve",
    );

    // And no deployment named stays the benign absence it is: the local loop and
    // this test suite operate no deployment.
    assert_eq!(
        cloud::workspace::WorkspaceConfig::from_lookup(|_| None).unwrap_err(),
        cloud::workspace::WorkspaceConfigError::MissingDeployment,
    );
}

/// A Project's repository URL is **stored data**, never composed.
///
/// This is the invariant that replaced the derivation, and it is the one a
/// future change is most likely to undo by reintroducing a convenience helper.
/// A Project's source may live on any forge in any organization, so composing
/// `{host}/{org}/{code}` would both invent a URL for a Project that has none and
/// silently override one that names somewhere else.
#[test]
fn no_project_repository_url_is_composed_from_a_project_code() {
    let workspace = std::fs::read_to_string(repo_root().join("cloud/src/workspace.rs"))
        .expect("read cloud/src/workspace.rs");
    for banned in ["project_repository", "RepositoryCoordinate"] {
        assert!(
            !workspace.contains(banned),
            "`{banned}` composes a Project repository coordinate; a Project's repository is \
             `store::projects::Project::repository_url`, a whole URL on any forge",
        );
    }

    // The positive half: the column is what carries it, and it is validated
    // rather than trusted.
    assert!(
        store::projects::is_valid_repository_url("https://gitlab.example/a-group/a-project"),
        "any forge must be storable",
    );
    assert!(
        !store::projects::is_valid_repository_url("https://forge.example"),
        "a forge root is not a repository",
    );
}
