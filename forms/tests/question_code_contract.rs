//! The field-name = question-code contract, verified as a guard test.
//!
//! Issue #231's core claim is that a government form's fill map is not
//! trusted, it is *checked*: every field a packet fills must resolve to a
//! real question the questionnaire actually asks, and every question must
//! be one of the canonical seeded types (`store/seeds/Question.yaml`, via
//! `rules::canonical_question_codes()` — the same source of truth the
//! notation-template linter uses post-#233).
//!
//! Today the three NV blanks still carry a `<code>.fields.toml` that maps
//! their hostile `OmniForm` `/T` names onto question references; the human
//! re-authoring that makes the PDF `/T` names *be* question codes is a
//! sequenced follow-on (see `docs/gov-forms.md`). This guard
//! pins the layer that exists today: it fails CI if a `.fields.toml`
//! references a question the notation never declares, or if a notation
//! declares a state whose type is not canonical. Either way a mis-map
//! breaks loudly here, before it can mis-fill a filing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

/// The notation frontmatter fields this guard reads.
#[derive(Debug, Deserialize)]
struct Notation {
    questionnaire: std::collections::BTreeMap<String, serde_yaml::Value>,
}

/// Read a vendored form's sibling notation `.md` and return its declared
/// questionnaire state names (excluding the `BEGIN` / `END` sentinels).
fn questionnaire_states(object_path: &str) -> Vec<String> {
    let md_rel = object_path.replace(".pdf", ".md");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("templates")
        .join(&md_rel);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read notation {}: {e}", path.display()));
    let fm = frontmatter(&contents)
        .unwrap_or_else(|| panic!("{}: no `---` frontmatter block", path.display()));
    let notation: Notation = serde_yaml::from_str(fm)
        .unwrap_or_else(|e| panic!("{}: parse frontmatter: {e}", path.display()));
    notation
        .questionnaire
        .into_keys()
        .filter(|s| s != "BEGIN" && s != "END")
        .collect()
}

/// The YAML frontmatter block between the leading `---` and its closer.
fn frontmatter(contents: &str) -> Option<&str> {
    let rest = contents.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// The `<type>` a questionnaire state is built from — the segment before
/// `__` (`entity__company` → `entity`, `people__managing_members` →
/// `people`). A state with no `__` is its own type.
fn state_type(state: &str) -> &str {
    state.split_once("__").map_or(state, |(t, _)| t)
}

/// A `.fields.toml` question reference resolves to a declared state when
/// its leading segment (before any dotted `.part`) either *is* a state or
/// is the `__role` suffix of *exactly one* — mirroring `fieldmap::answer_for`,
/// which looks an answer up by exact key, else by a `__{question}` suffix that
/// must match a single key (an ambiguous suffix returns `None` at runtime, so
/// the guard must treat it as unresolved too, not silently pass).
fn resolves_to_state(question: &str, states: &BTreeSet<&str>) -> bool {
    let head = question.split('.').next().unwrap_or(question);
    if states.contains(head) {
        return true;
    }
    let mut suffixed = states
        .iter()
        .filter(|s| s.strip_suffix(head).is_some_and(|p| p.ends_with("__")));
    suffixed.next().is_some() && suffixed.next().is_none()
}

#[test]
fn every_notation_state_is_a_canonical_question_type() {
    let canonical: BTreeSet<String> = rules::canonical_question_codes().into_iter().collect();
    for form in forms::registry().expect("registry loads") {
        for state in questionnaire_states(form.meta.object_path) {
            let ty = state_type(&state);
            assert!(
                canonical.contains(ty),
                "{}: questionnaire state `{state}` has type `{ty}`, which is not a \
                 canonical question code in store/seeds/Question.yaml",
                form.meta.code
            );
        }
    }
}

#[test]
fn every_mapped_question_resolves_to_a_declared_state() {
    for form in forms::registry().expect("registry loads") {
        let states = questionnaire_states(form.meta.object_path);
        let states: BTreeSet<&str> = states.iter().map(String::as_str).collect();
        let map = forms::field_map(form.meta.code)
            .expect("map parses")
            .expect("every vendored form has a map");
        for rule in &map.field {
            // A literal carries no question reference — it fills a fixed
            // value the blank can't pre-print, so there is nothing to
            // resolve against the questionnaire.
            for question in rule.question.iter().chain(rule.present_in.iter()) {
                assert!(
                    resolves_to_state(question, &states),
                    "{}: field `{}` references question `{question}`, which no \
                     questionnaire state in {}.md declares — the map was guessed, \
                     the question was renamed, or the notation drifted",
                    form.meta.code,
                    rule.name,
                    form.meta.code
                );
            }
        }
    }
}
