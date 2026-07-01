//! Authoring-time coverage for the Northstar estate suite.
//!
//! The estate plan is a *suite* of templates — the onboarding matter
//! (`neon_law/northstar/estate_plan.md`) plus the four instrument stubs under
//! `templates/neon_law/northstar/`.
//! The recorded sitting must answer every
//! question the suite needs, so the extraction step has a value for
//! every `{{placeholder}}` the instruments render. This test pins that
//! invariant at authoring time, cross-file, so a hand-edit that adds a
//! placeholder without asking the question (or asks a state with an
//! unregistered question type) fails fast:
//!
//! 1. Every data `{{placeholder}}` in an instrument body is a question
//!    the sitting actually asks (it appears in `neon_law/northstar/estate_plan.md`).
//! 2. Every question state the sitting asks uses a seeded type prefix and
//!    declares a template prompt, so the extractor and questionnaire can
//!    resolve it.
//!
//! No hard-coded code list: the asked set is derived from the Estate
//! template itself, the union the suite needs from the instrument bodies,
//! and the seeded type set from `Question.yaml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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

/// Every `{{ … }}` data placeholder in a template body, normalized to
/// its questionnaire state. Dotted glossary fields such as
/// `{{person__testator.name}}` are backed by the `person__testator`
/// state; signature anchors such as `{{client.signature}}` are not data
/// questions and are skipped.
fn data_placeholders(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = body;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let token = after[..close].trim();
        if let Some(token) = normalize_data_placeholder(token) {
            out.insert(token.to_string());
        }
        rest = &after[close + 2..];
    }
    out
}

fn normalize_data_placeholder(token: &str) -> Option<&str> {
    if token.is_empty() {
        return None;
    }
    let (head, field) = token.split_once('.').unwrap_or((token, ""));
    if matches!(head, "client" | "firm") && matches!(field, "signature" | "date") {
        return None;
    }
    Some(head)
}

/// The question codes the canonical `Question.yaml` seed declares.
fn seeded_question_types() -> BTreeSet<String> {
    let yaml = read(&workspace_root().join("store/seeds/Question.yaml"));
    yaml.lines()
        .filter_map(|l| l.trim().strip_prefix("- code:"))
        .map(|c| c.trim().to_string())
        .collect()
}

fn question_type(state_name: &str) -> &str {
    state_name.split("__").next().unwrap_or(state_name)
}

fn prompt_key(state_name: &str) -> Option<&str> {
    state_name.split_once("__").map(|(_, key)| key)
}

fn prompt_keys(state_name: &str) -> Vec<String> {
    let Some(key) = prompt_key(state_name) else {
        return Vec::new();
    };
    let mut keys = vec![key.to_string()];
    if question_type(state_name) == "person" {
        keys.push(format!("{key}_name"));
    }
    keys
}

#[derive(Debug, Default, Deserialize)]
struct TemplateFrontmatter {
    #[serde(default)]
    prompts: std::collections::BTreeMap<String, String>,
}

fn frontmatter_prompts(template: &str) -> BTreeSet<String> {
    let Some(rest) = template.strip_prefix("---\n") else {
        return BTreeSet::new();
    };
    let Some((fm, _)) = rest.split_once("\n---") else {
        return BTreeSet::new();
    };
    serde_yaml::from_str::<TemplateFrontmatter>(fm)
        .expect("estate template frontmatter should parse")
        .prompts
        .into_keys()
        .collect()
}

const INSTRUMENTS: &[&str] = &[
    "templates/neon_law/northstar/nv__will.md",
    "templates/neon_law/northstar/nv__trust.md",
    "templates/neon_law/northstar/nv__directive_health.md",
    "templates/neon_law/northstar/nv__directive_financial.md",
];

#[test]
fn estate_suite_questions_are_all_asked_and_seeded() {
    let root = workspace_root();
    let estate_plan = read(&root.join("templates/neon_law/northstar/estate_plan.md"));
    let asked = data_placeholders(&estate_plan);
    assert!(
        !asked.is_empty(),
        "the Estate onboarding template declares no questions — wrong path?"
    );

    let seeded = seeded_question_types();
    let prompts = frontmatter_prompts(&estate_plan);

    // (2) Every question state the sitting asks uses a seeded type
    // prefix, and every discriminated custom state declares its prompt
    // in the template frontmatter.
    for state_name in &asked {
        let typ = question_type(state_name);
        assert!(
            seeded.contains(typ),
            "Estate asks `{state_name}` but no question type `{typ}` is seeded in \
             store/seeds/Question.yaml"
        );
        let prompt_keys = prompt_keys(state_name);
        if !prompt_keys.is_empty() {
            assert!(
                prompt_keys.iter().any(|key| prompts.contains(key)),
                "Estate asks `{state_name}` but does not declare one of prompts.{}",
                prompt_keys.join(" / prompts.")
            );
        }
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
                 (add it to neon_law/northstar/estate_plan.md's questionnaire + body)"
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
