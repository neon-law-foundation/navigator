//! Authoring-time coverage for the Northstar estate suite.
//!
//! The estate plan is a *suite* of templates — the onboarding matter
//! (`onboarding/estate.md`) plus the four instrument stubs under
//! `templates/northstar/`. The recorded sitting must answer every
//! question the suite needs, so the extraction step has a value for
//! every `{{placeholder}}` the instruments render. This test pins that
//! invariant at authoring time, cross-file, so a hand-edit that adds a
//! placeholder without asking the question (or asks a question nothing
//! is seeded for) fails fast:
//!
//! 1. Every data `{{placeholder}}` in an instrument body is a question
//!    the sitting actually asks (it appears in `onboarding/estate.md`).
//! 2. Every question the sitting asks is seeded in `Question.yaml` with
//!    a prompt, so the extractor and the questionnaire can resolve it.
//!
//! No hard-coded code list: the asked set is derived from the Estate
//! template itself, the union the suite needs from the instrument
//! bodies, and the seeded set from `Question.yaml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // store/ is one level below the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("store crate lives one level below the workspace root")
        .to_path_buf()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `{{ … }}` data placeholder in a template body — the trimmed
/// inner token, excluding signature placeholders (those contain a `.`,
/// e.g. `{{client.signature}}`; see `rules::f107`).
fn data_placeholders(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let token = after[..close].trim();
        if !token.is_empty() && !token.contains('.') {
            out.insert(token.to_string());
        }
        rest = &after[close + 2..];
    }
    out
}

/// The question codes the canonical `Question.yaml` seed declares.
fn seeded_question_codes() -> BTreeSet<String> {
    let yaml = read(&workspace_root().join("store/seeds/Question.yaml"));
    yaml.lines()
        .filter_map(|l| l.trim().strip_prefix("- code:"))
        .map(|c| c.trim().to_string())
        .collect()
}

const INSTRUMENTS: &[&str] = &[
    "templates/northstar/will.md",
    "templates/northstar/trust.md",
    "templates/northstar/directive_health.md",
    "templates/northstar/directive_financial.md",
];

#[test]
fn estate_suite_questions_are_all_asked_and_seeded() {
    let root = workspace_root();
    let asked = data_placeholders(&read(&root.join("templates/onboarding/estate.md")));
    assert!(
        !asked.is_empty(),
        "the Estate onboarding template declares no questions — wrong path?"
    );

    let seeded = seeded_question_codes();

    // (2) Every question the sitting asks is seeded with a prompt.
    for code in &asked {
        assert!(
            seeded.contains(code),
            "Estate asks `{code}` but no question with that code is seeded in \
             store/seeds/Question.yaml"
        );
    }

    // (1) Every instrument placeholder is a question the sitting asks —
    // so extraction fills a value for every field the drafts render.
    let mut suite_needs = BTreeSet::new();
    for rel in INSTRUMENTS {
        let placeholders = data_placeholders(&read(&root.join(rel)));
        assert!(
            !placeholders.is_empty(),
            "{rel} renders no placeholders — a stub with no fields is not useful"
        );
        for code in &placeholders {
            assert!(
                asked.contains(code),
                "{rel} renders `{{{{{code}}}}}` but the sitting never asks `{code}` \
                 (add it to onboarding/estate.md's questionnaire + body)"
            );
            suite_needs.insert(code.clone());
        }
    }

    // Sanity: the suite actually exercises several distinct fields.
    assert!(
        suite_needs.len() >= 5,
        "expected the estate instruments to need several fields, got {suite_needs:?}"
    );
}
