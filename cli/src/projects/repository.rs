//! One Project, one repository: scaffold and validation.
//!
//! A Project's repository is named for its Project code and holds two kinds of
//! source side by side — that Project's notation templates under `templates/`,
//! and its client portal under `portal/`. There is one layout and one command
//! for both, because there is one repository.
//!
//! ```text
//! <organization>/<project-code>
//! ├── .github/workflows/gate.yml
//! ├── portal/            # React + Vite; the client's portal
//! ├── templates/         # *.md notation blueprints
//! ├── AGENTS.md
//! ├── CLAUDE.md
//! └── README.md
//! ```
//!
//! # Nothing declares its own name
//!
//! There is no manifest. The Project code *is* the repository name, and the
//! mount is that name plus the literal `portal`, so a manifest could only
//! restate what the repository is already called — and then disagree with it.
//! [`validate`] takes the code from the repository name, and CI has that name
//! as `github.event.repository.name`.
//!
//! # The scaffold does not write the portal
//!
//! It writes the repository shell and the templates half. `portal/` arrives
//! from the vibe-coding lane, which is what knows how to make a Vite
//! application and which released `@neon-law/ux` to pin. That keeps [`validate`]
//! unambiguous: `portal/` present means there is a portal to hold to the Vite
//! contract, and absent means this Project does not have one yet.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The notation blueprints Navigator imports.
const TEMPLATE_DIRECTORY: &str = "templates";
/// The client portal's Vite workspace.
const PORTAL_DIRECTORY: &str = "portal";
const WORKFLOW: &str = ".github/workflows/gate.yml";
const ALLOWED_ROOTS: &[&str] = &[
    ".github",
    ".gitignore",
    "AGENTS.md",
    "CLAUDE.md",
    // Every one of these repositories is proprietary, and a licence belongs at
    // the root where a reader looks for it. A portal-bearing repository could
    // hide one inside `portal/`; a templates-only Project has nowhere to put it
    // at all, so refusing it here made the layout unsatisfiable for that shape.
    "LICENSE.md",
    "README.md",
    "fixtures",
    PORTAL_DIRECTORY,
    TEMPLATE_DIRECTORY,
    "tests",
];
const FORBIDDEN_COMPONENTS: &[&str] = &[
    "answers",
    "build",
    "client_uploads",
    "dependencies",
    "dist",
    "documents",
    "generated",
    "node_modules",
    "output",
    "secrets",
    "target",
    "uploads",
    "vendor",
];
const FORBIDDEN_CREDENTIAL_EXTENSIONS: &[&str] = &["env", "key", "pem", "p12", "pfx"];
const FORBIDDEN_DOCUMENT_EXTENSIONS: &[&str] = &["doc", "docx", "odt", "pdf"];

/// The files a Vite-built portal must have at the root of its directory.
///
/// Deliberately **no dependency allowlist**: third-party libraries are the
/// point of a Project carrying a Vite portal, so the contract is the build
/// shape, not the package list. A lockfile is required but its flavor is not —
/// a Project repository picks its own package manager, and Node never enters
/// the Navigator workspace.
const VITE_ENTRYPOINTS: &[&str] = &["package.json", "index.html"];
const VITE_LOCKFILES: &[&str] = &[
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
];

#[derive(Debug)]
struct Finding {
    path: PathBuf,
    message: String,
}

impl Finding {
    fn at(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Create the reviewed scaffold without overwriting existing work.
pub fn scaffold(root: &Path, project_code: &str) -> ExitCode {
    if !store::projects::is_valid_code(project_code) {
        eprintln!(
            "navigator: invalid Project code `{project_code}`; use lowercase letters, digits, and single hyphens (80 characters maximum), and not a segment Navigator routes itself"
        );
        return ExitCode::from(2);
    }

    let files = [
        (root.join("README.md"), readme(project_code)),
        (root.join("AGENTS.md"), agents(project_code)),
        (root.join("CLAUDE.md"), agents(project_code)),
        (
            root.join(TEMPLATE_DIRECTORY).join("project_template.md"),
            example_template(),
        ),
        (root.join("tests/README.md"), tests_readme()),
        (root.join(WORKFLOW), workflow()),
    ];

    for (path, contents) in files {
        if path.exists() {
            println!("exists    {} (left alone)", path.display());
            continue;
        }
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("navigator: create {}: {error}", parent.display());
                return ExitCode::from(2);
            }
        }
        if let Err(error) = fs::write(&path, contents) {
            eprintln!("navigator: write {}: {error}", path.display());
            return ExitCode::from(2);
        }
        println!("created   {}", path.display());
    }

    println!(
        "\nValidate with: navigator projects repository validate {}",
        root.display()
    );
    ExitCode::SUCCESS
}

/// Validate one Project's repository.
///
/// Templates are intentionally passed to the rule engine under bare filenames:
/// they are Project blueprints, not members of Navigator's shared
/// `templates/neon_law` / `templates/forms` catalog. This mirrors
/// `store::template_source::persist_from_repo` exactly.
///
/// Three shapes are all valid — templates only, a portal only, or both — and a
/// repository carrying neither is reported distinctly rather than failed. A
/// Project may legitimately have opened before either half exists.
pub fn validate(root: &Path, repository: Option<&str>) -> ExitCode {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if !root.is_dir() {
        eprintln!(
            "navigator: Project repository root is not a directory: {}",
            root.display()
        );
        return ExitCode::from(2);
    }

    // The repository name is the Project code. Nothing declares it, so nothing
    // can disagree with it — but a checkout named something a Project code
    // could never be is a checkout this validator cannot speak about.
    let code = repository_name(root, repository);
    if !store::projects::is_valid_code(&code) {
        errors.push(Finding::at(
            root,
            format!(
                "repository name `{code}` is not a valid Navigator Project code; \
                 the repository name *is* the code"
            ),
        ));
    }

    let has_templates = root.join(TEMPLATE_DIRECTORY).is_dir();
    let has_portal = root.join(PORTAL_DIRECTORY).is_dir();

    validate_layout(root, &mut errors);
    let templates = if has_templates {
        validate_templates(root, &mut errors, &mut warnings)
    } else {
        0
    };
    if has_portal {
        validate_portal(root, &mut errors);
    }
    if !has_templates && !has_portal {
        println!(
            "note: {code} carries neither `{TEMPLATE_DIRECTORY}/` nor `{PORTAL_DIRECTORY}/` yet"
        );
    }

    for warning in &warnings {
        println!("{}: warning: {}", warning.path.display(), warning.message);
    }
    for error in &errors {
        eprintln!("{}: error: {}", error.path.display(), error.message);
    }
    println!(
        "Validated Project repository `{code}`: {templates} template(s), {} portal, {} error(s), {} warning(s)",
        if has_portal { "1" } else { "0" },
        errors.len(),
        warnings.len()
    );

    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn repository_name(root: &Path, explicit: Option<&str>) -> String {
    if let Some(name) = explicit.map(str::trim).filter(|name| !name.is_empty()) {
        return name.rsplit('/').next().unwrap_or(name).to_string();
    }
    if let Ok(repository) = std::env::var("GITHUB_REPOSITORY") {
        if let Some(name) = repository
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
        {
            return name.to_string();
        }
    }
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("<unknown>")
        .to_string()
}

fn validate_layout(root: &Path, errors: &mut Vec<Finding>) {
    if !root.join("README.md").is_file() {
        errors.push(Finding::at(
            root.join("README.md"),
            "missing required repository README",
        ));
    }

    let workflow_path = root.join(WORKFLOW);
    match fs::read_to_string(&workflow_path) {
        Ok(contents) => validate_workflow(&workflow_path, &contents, errors),
        Err(_) => errors.push(Finding::at(workflow_path, "missing required CI gate")),
    }

    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            // A portal's own build output and dependencies are forbidden by
            // name below; descending into them would report thousands of
            // findings for one mistake.
            entry.file_name() != ".git"
                && entry.file_name() != "node_modules"
                && entry.file_name() != "dist"
        })
    {
        let Ok(entry) = entry else {
            errors.push(Finding::at(root, "could not walk repository"));
            return;
        };
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let components: Vec<String> = relative
            .components()
            .filter_map(|component| component.as_os_str().to_str().map(str::to_string))
            .collect();
        let Some(first) = components.first() else {
            continue;
        };
        if components.len() == 1 && !ALLOWED_ROOTS.contains(&first.as_str()) {
            errors.push(Finding::at(
                entry.path(),
                "path is outside the source-only Project repository layout",
            ));
        }
        if let Some(component) = components
            .iter()
            .find(|component| FORBIDDEN_COMPONENTS.contains(&component.as_str()))
        {
            errors.push(Finding::at(
                entry.path(),
                format!("forbidden `{component}` path; repositories hold source, never client material or build output"),
            ));
        }
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            let extension = entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default();
            if name == ".env" || name.starts_with(".env.") || name.starts_with("answers.") {
                errors.push(Finding::at(
                    entry.path(),
                    "client answers and environment secrets must not be committed",
                ));
            }
            if FORBIDDEN_CREDENTIAL_EXTENSIONS.contains(&extension) {
                errors.push(Finding::at(
                    entry.path(),
                    "credential material must not be committed",
                ));
            }
            if FORBIDDEN_DOCUMENT_EXTENSIONS.contains(&extension) {
                errors.push(Finding::at(
                    entry.path(),
                    "legal documents and rendered output must not be committed",
                ));
            }
        }
    }
}

/// The portal's build shape, where a portal exists.
fn validate_portal(root: &Path, errors: &mut Vec<Finding>) {
    let portal = root.join(PORTAL_DIRECTORY);
    let mut missing: Vec<&str> = VITE_ENTRYPOINTS
        .iter()
        .copied()
        .filter(|file| !portal.join(file).is_file())
        .collect();
    if !VITE_LOCKFILES
        .iter()
        .any(|file| portal.join(file).is_file())
    {
        missing.push("a lockfile");
    }
    if !missing.is_empty() {
        errors.push(Finding::at(
            &portal,
            format!(
                "`{PORTAL_DIRECTORY}/` is present but is not a Vite workspace: missing {}",
                missing.join(", ")
            ),
        ));
    }
}

fn validate_workflow(path: &Path, contents: &str, errors: &mut Vec<Finding>) {
    const ACTION: &str = "neon-law-foundation/navigator/.github/actions/validate@";
    let action_version = contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix("- uses: ")
            .and_then(|value| value.strip_prefix(ACTION))
            .map(|value| value.split_whitespace().next().unwrap_or_default())
    });
    let input_version = contents.lines().find_map(|line| {
        line.trim()
            .strip_prefix("version:")
            .map(str::trim)
            .map(|value| value.trim_matches(['\'', '"']))
    });

    let Some(action_version) = action_version else {
        errors.push(Finding::at(
            path,
            "CI gate must call Navigator's pinned validate action",
        ));
        return;
    };
    let Some(input_version) = input_version else {
        errors.push(Finding::at(
            path,
            "CI gate must pass the action's exact release tag as `version`",
        ));
        return;
    };
    if action_version != input_version {
        errors.push(Finding::at(
            path,
            format!(
                "validation action ref `{action_version}` must equal its downloaded CLI version `{input_version}`"
            ),
        ));
    }
    if !is_release_tag(action_version) {
        errors.push(Finding::at(
            path,
            format!("validation action ref `{action_version}` must be an exact YY.M.D release tag"),
        ));
    }
    if !contents.contains("project_repository: true") {
        errors.push(Finding::at(
            path,
            "CI gate must set `project_repository: true`",
        ));
    }
}

fn is_release_tag(version: &str) -> bool {
    let segments: Vec<&str> = version.split('.').collect();
    (segments.len() == 3 || segments.len() == 4)
        && segments
            .iter()
            .all(|segment| !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()))
}

fn validate_templates(
    root: &Path,
    errors: &mut Vec<Finding>,
    warnings: &mut Vec<Finding>,
) -> usize {
    let directory = root.join(TEMPLATE_DIRECTORY);
    let mut paths = Vec::new();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(Finding::at(directory, format!("read templates: {error}")));
            return 0;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(Finding::at(
                    &directory,
                    format!("read template entry: {error}"),
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            errors.push(Finding::at(
                path,
                "Project templates must be direct `templates/<code>.md` files",
            ));
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("md") {
            paths.push(path);
        } else {
            errors.push(Finding::at(
                path,
                "only Markdown template blueprints belong in `templates/`",
            ));
        }
    }
    paths.sort();
    if paths.is_empty() {
        errors.push(Finding::at(
            directory,
            "`templates/` is present but empty; at least one `templates/<code>.md` blueprint is required",
        ));
        return 0;
    }

    let rules = rules::navigator_default_rules_with_codes(&rules::canonical_question_codes());
    let mut declared_codes = BTreeMap::new();
    for path in &paths {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) => {
                errors.push(Finding::at(path, format!("read template: {error}")));
                continue;
            }
        };
        let filename = path.file_name().map_or_else(PathBuf::new, PathBuf::from);
        let source = rules::SourceFile {
            path: filename,
            contents: contents.clone(),
        };
        for violation in rules.iter().flat_map(|rule| rule.lint(&source)) {
            let finding = Finding::at(path, format!("{}: {}", violation.code, violation.message));
            if rules::severity_for_code(violation.code) == rules::Severity::Error {
                errors.push(finding);
            } else {
                warnings.push(finding);
            }
        }
        if let Some(code) = rules::frontmatter::extract(&contents)
            .and_then(|frontmatter| rules::frontmatter::field(frontmatter, "code"))
        {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            if code != stem {
                errors.push(Finding::at(
                    path,
                    format!("template `code` `{code}` must equal filename stem `{stem}`"),
                ));
            }
            if let Some(first) = declared_codes.insert(code.clone(), path.clone()) {
                errors.push(Finding::at(
                    path,
                    format!(
                        "duplicate template `code` `{code}`; first declared in {}",
                        first.display()
                    ),
                ));
            }
        }
    }
    paths.len()
}

fn readme(project_code: &str) -> String {
    format!(
        "# {project_code}\n\nThis repository holds source-only material for Project `{project_code}`: its notation\n\
         templates under `templates/`, and its client portal under `portal/`.\n\n\
         The repository name *is* the Project code. Nothing in here declares it, so nothing can\n\
         disagree with it: Navigator's portal mount is `/app/projects/{project_code}/portal/`, derived from\n\
         the repository name plus one literal segment.\n\n\
         Navigator imports each direct `templates/<code>.md` file at the current commit, preserving both\n\
         that commit SHA and the template body's content hash as provenance.\n\n\
         Do not commit client uploads, answers, generated documents, secrets, dependencies, or build\n\
         output. Legal files live in Drive and in Navigator's assets, never in Git.\n\n\
         Run `navigator projects repository validate .` before opening a pull request.\n"
    )
}

fn agents(project_code: &str) -> String {
    format!(
        "# Working in {project_code}\n\n\
         This is one Project's repository. It holds two kinds of source and nothing else.\n\n\
         * `templates/` — notation blueprints, one `templates/<code>.md` per notation. Navigator\n\
           imports them and records the commit SHA as provenance.\n\
         * `portal/` — the client's React + Vite portal. Build it for the base\n\
           `/app/projects/{project_code}/portal/`, and derive every in-app path from\n\
           `import.meta.env.BASE_URL` rather than writing an absolute path by hand: a Vite base\n\
           rewrites module and asset URLs and never an `href` in source.\n\n\
         Read matter data through Navigator's `/api` read surfaces and write through its one REST\n\
         command boundary. Do not add a second backend, and do not put a legal file, a client upload,\n\
         an answer, a generated document, or a secret in this repository.\n"
    )
}

fn tests_readme() -> String {
    "# Tests\n\nKeep source-level tests for this Project's templates here. Generated documents and dependencies do not belong here.\n"
        .to_string()
}

fn example_template() -> String {
    r"---
kind: letter
title: Project Template Placeholder
respondent_type: entity
code: project_template
confidential: false
jurisdiction: NV
questionnaire:
  BEGIN:
    _: END
  END: {}
workflow:
  BEGIN:
    _: lawyer_review
  lawyer_review:
    _: END
  END: {}
---

Replace this placeholder with the Project-specific template approved for import.
"
    .to_string()
}

/// The one CI gate, in the one job name `ops github setup` requires.
///
/// There is deliberately **no** `paths:` filter. A filtered job that skips
/// reports success for work it never did, and a required check that can be
/// satisfied by a skip is not a gate. So the job always runs and the action
/// no-ops internally over whichever half this repository carries.
/// A raw string, not a `\`-continued one. A backslash continuation strips the
/// leading whitespace of the next line, which silently reflows YAML into
/// something that no longer parses — and a generated workflow that does not
/// parse fails in the Project repository rather than here.
fn workflow() -> String {
    r#"name: ci

on:
  pull_request:
  push:
    branches: [main]

jobs:
  # The one required check. It always runs: the gate no-ops over a half this
  # repository does not carry, rather than being skipped by a path filter and
  # reporting success for a job that never ran.
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
      - uses: neon-law-foundation/navigator/.github/actions/validate@26.7.27
        with:
          version: "26.7.27"
          project_repository: true
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{example_template, is_release_tag, repository_name, workflow, ALLOWED_ROOTS};
    use std::path::Path;

    #[test]
    fn repository_name_prefers_the_explicit_coordinate() {
        assert_eq!(
            repository_name(Path::new("/tmp/renamed"), Some("org/example")),
            "example"
        );
    }

    #[test]
    fn generated_template_has_a_stable_code() {
        assert!(example_template().contains("code: project_template"));
        assert!(is_release_tag("26.7.27"));
        assert!(!is_release_tag("main"));
    }

    /// The generated gate is one always-running required job.
    ///
    /// A `paths:` filter here would let a required check pass by being skipped,
    /// so the job name matches the one `ops github setup` binds and the gate
    /// carries no filter at all.
    #[test]
    fn a_licence_at_the_root_is_part_of_the_layout() {
        // Every one of these repositories is proprietary. A templates-only
        // Project has no `portal/` to hide a licence inside, so refusing it at
        // the root made the layout unsatisfiable for that shape rather than
        // merely opinionated.
        assert!(ALLOWED_ROOTS.contains(&"LICENSE.md"));
    }

    #[test]
    fn the_generated_gate_is_one_unfiltered_required_job() {
        let generated = workflow();
        assert!(generated.contains("\n  ci:\n"), "{generated}");
        assert!(
            !generated.contains("paths:"),
            "a path-filtered required check can be satisfied by a skip"
        );
        assert!(generated.contains("project_repository: true"));
    }
}
