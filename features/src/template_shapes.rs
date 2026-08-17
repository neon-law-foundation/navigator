//! Composition-locking helpers for the `*_workflow_shapes.feature`
//! Cucumber runners.
//!
//! Each scenario in those suites loads one bundled template,
//! parses its `questionnaire:` and `workflow:` frontmatter blocks,
//! and walks the resulting state machine from BEGIN following the
//! canonical `_` transition. The walk is compared to a Gherkin data
//! table that pins the reusable-step composition — an accidental
//! reshape shows up as a named failing scenario.

use std::path::PathBuf;

use workflows::{StateName, WorkflowSpec};

/// Workspace-relative `templates/` directory, computed from the
/// `features` crate's `CARGO_MANIFEST_DIR`.
#[must_use]
pub fn templates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("features crate is one level below the workspace root")
        .join("templates")
}

/// Strip the second `END: {}` declaration from a bundled template's
/// frontmatter — the one belonging to the `workflow:` block — so a
/// rejection scenario can confirm the parser surfaces
/// `WorkflowSpecError::MissingEnd`.
///
/// # Panics
///
/// Panics if `markdown` doesn't contain a `\nworkflow:\n` anchor.
#[must_use]
pub fn strip_workflow_end(markdown: &str) -> String {
    let workflow_anchor = markdown
        .find("\nworkflow:\n")
        .expect("template has a `workflow:` block");
    let (head, tail) = markdown.split_at(workflow_anchor);
    let mutated_tail = tail.replace("  END: {}\n", "");
    format!("{head}{mutated_tail}")
}

/// Walk a linear state machine from BEGIN following the canonical
/// `_` transition and return the sequence of `(from, to)` pairs.
///
/// # Panics
///
/// Panics if any state along the way doesn't have a `_` transition
/// (i.e. the spec isn't a linear chain), or if the walk runs past
/// 32 hops (cycle guard).
#[must_use]
pub fn walk_chain(spec: &WorkflowSpec) -> Vec<(String, String)> {
    let mut chain = Vec::new();
    let mut cursor = StateName::begin();
    let mut guard = 0;
    while cursor.as_str() != StateName::END {
        let next = spec
            .transitions_from(&cursor)
            .and_then(|t| t.lookup("_"))
            .unwrap_or_else(|| {
                panic!(
                    "no `_` transition out of `{}`; spec is not a linear chain",
                    cursor.as_str(),
                )
            });
        chain.push((cursor.0.clone(), next.0.clone()));
        cursor = next.clone();
        guard += 1;
        assert!(guard < 32, "transition chain looks unbounded; cycle?");
    }
    chain
}
