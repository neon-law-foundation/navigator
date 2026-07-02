//! `N118` — a questionnaire is one linear `_` chain from `BEGIN` to `END`.
//!
//! The walker advances a questionnaire on exactly one signal — "the
//! respondent answered" (`_`) — and renders "step N of M" from the `_`
//! chain out of `BEGIN`. A non-`_` condition, a state off that chain, or
//! a chain that stops or cycles before `END` is a questionnaire the
//! walker cannot honestly run: it strands the respondent or lies about
//! the step total. This is the authoring-time / LSP mirror of
//! [`workflows::QuestionnaireSpec::validate`] — the same invariant the
//! parse gate enforces, hoisted to a red squiggle in the editor. The
//! `linearity_classification_matches_questionnaire_spec_validation`
//! drift test below locks the two definitions together (`workflows`
//! stays a dev-dependency so the lint engine's build graph stays lean).
//!
//! Files without frontmatter or without a `questionnaire:` key are
//! silently skipped, like the other questionnaire-shape rules. Base
//! shape errors (missing `BEGIN`/`END`, dangling targets) are not
//! reported here — only the linearity findings are N118's to own.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F118QuestionnaireLinearity;

impl F118QuestionnaireLinearity {
    pub const CODE: &'static str = "N118";
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

impl Rule for F118QuestionnaireLinearity {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = frontmatter::extract(&file.contents) else {
            return Vec::new();
        };
        let Ok(parsed) = serde_yaml::from_str::<FrontmatterShape>(fm) else {
            return Vec::new();
        };
        let Some(questionnaire) = parsed.questionnaire else {
            return Vec::new();
        };

        // One violation per file: the first break in the chain usually
        // cascades (everything after a broken link looks off-chain too).
        let Some((state, message)) = linearity_finding(&questionnaire) else {
            return Vec::new();
        };
        let line = questionnaire_state_line(&file.contents, &state);
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line,
            range: line_byte_range(&file.contents, line),
            message,
        }]
    }
}

/// The first linearity break in `questionnaire`, as
/// `(offending state, message)` — or `None` when the chain is linear
/// **or** the block has a base-shape problem (missing `BEGIN`/`END`,
/// dangling target) that the parse gate owns instead of this rule.
///
/// Mirrors `workflows::QuestionnaireSpec::validate` exactly; the drift
/// test pins the classification to it.
fn linearity_finding(
    questionnaire: &BTreeMap<String, BTreeMap<String, String>>,
) -> Option<(String, String)> {
    if !questionnaire.contains_key("BEGIN") || !questionnaire.contains_key("END") {
        return None;
    }
    for transitions in questionnaire.values() {
        if transitions
            .values()
            .any(|to| !questionnaire.contains_key(to))
        {
            return None;
        }
    }

    for (state, transitions) in questionnaire {
        for condition in transitions.keys() {
            if condition != "_" {
                return Some((
                    state.clone(),
                    format!(
                        "questionnaire state `{state}` has condition `{condition}` — `_` (\"the \
                         respondent answered\") is the only questionnaire condition"
                    ),
                ));
            }
        }
    }

    // Walk the `_` chain from BEGIN. Condition-uniqueness above means at
    // most one `_` per state, so the walk is deterministic.
    let mut on_chain: BTreeSet<&str> = BTreeSet::new();
    let mut here = "BEGIN";
    loop {
        let Some(next) = questionnaire.get(here).and_then(|t| t.get("_")) else {
            return Some((here.to_string(), chain_broken_message(here)));
        };
        if next == "END" {
            break;
        }
        if !on_chain.insert(next) {
            return Some((next.clone(), chain_broken_message(next)));
        }
        here = next;
    }

    for state in questionnaire.keys() {
        if state != "BEGIN" && state != "END" && !on_chain.contains(state.as_str()) {
            return Some((
                state.clone(),
                format!(
                    "questionnaire state `{state}` is not on the `_` chain from BEGIN — the \
                     walker would never ask it and the rendered step total would lie"
                ),
            ));
        }
    }
    None
}

fn chain_broken_message(state: &str) -> String {
    format!(
        "questionnaire `_` chain from BEGIN stops or cycles at `{state}` before reaching END — \
         the walker would strand the respondent there"
    )
}

/// 1-based line of the indented questionnaire state key `state`, so the
/// squiggle lands on the offending state rather than the frontmatter
/// delimiter. Falls back to line 1 when the key can't be located.
fn questionnaire_state_line(contents: &str, state: &str) -> usize {
    let key = format!("{state}:");
    for (idx, raw) in contents.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.len() < raw.len() && trimmed == key {
            return idx + 1;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::F118QuestionnaireLinearity;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn a_linear_chain_passes() {
        let body = "---
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: project__engagement
  project__engagement:
    _: END
  END: {}
---
";
        assert!(F118QuestionnaireLinearity.lint(&file(body)).is_empty());
    }

    #[test]
    fn a_non_underscore_condition_is_flagged_on_its_state_line() {
        let body = "---
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    skip: END
    _: END
  END: {}
---
";
        let v = F118QuestionnaireLinearity.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("skip"), "{}", v[0].message);
        assert_eq!(v[0].line, 5, "{v:?}");
    }

    #[test]
    fn an_off_chain_state_is_flagged() {
        let body = "---
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: END
  entity__company:
    _: END
  END: {}
---
";
        let v = F118QuestionnaireLinearity.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].message.contains("entity__company")
                && v[0].message.contains("not on the `_` chain"),
            "{}",
            v[0].message
        );
        assert_eq!(v[0].line, 7, "{v:?}");
    }

    #[test]
    fn a_chain_that_cycles_before_end_is_flagged() {
        let body = "---
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: person__spouse
  person__spouse:
    _: person__client
  END: {}
---
";
        let v = F118QuestionnaireLinearity.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("stops or cycles"), "{}", v[0].message);
    }

    #[test]
    fn base_shape_failures_are_not_this_rules_findings() {
        // Missing END and dangling targets are parse-gate failures, not
        // linearity ones.
        let missing_end = "---
questionnaire:
  BEGIN:
    _: person__client
  person__client: {}
---
";
        assert!(F118QuestionnaireLinearity
            .lint(&file(missing_end))
            .is_empty());
        let dangling = "---
questionnaire:
  BEGIN:
    _: person__missing
  END: {}
---
";
        assert!(F118QuestionnaireLinearity.lint(&file(dangling)).is_empty());
    }

    #[test]
    fn no_frontmatter_or_questionnaire_means_no_violation() {
        assert!(F118QuestionnaireLinearity
            .lint(&file("just body"))
            .is_empty());
        assert!(F118QuestionnaireLinearity
            .lint(&file("---\ntitle: T\n---\nbody\n"))
            .is_empty());
    }

    #[test]
    fn is_error_severity() {
        use crate::{severity_for_code, Severity};
        assert_eq!(severity_for_code("N118"), Severity::Error);
    }

    /// Drift lock: N118's linearity classification must stay identical to
    /// the parse gate's (`workflows::QuestionnaireSpec::validate`). For
    /// every corpus entry: rule finding ⇔ a `Questionnaire*` parse error;
    /// base-shape parse errors and valid specs both stay unflagged.
    #[test]
    fn linearity_classification_matches_questionnaire_spec_validation() {
        use workflows::{QuestionnaireSpec, WorkflowSpecError};

        const CORPUS: &[&str] = &[
            // linear (valid)
            "BEGIN:\n  _: a\na:\n  _: END\nEND: {}\n",
            // empty-but-linear (valid)
            "BEGIN:\n  _: END\nEND: {}\n",
            // non-`_` condition
            "BEGIN:\n  _: a\na:\n  skip: END\n  _: END\nEND: {}\n",
            // off-chain state
            "BEGIN:\n  _: a\na:\n  _: END\nb:\n  _: END\nEND: {}\n",
            // dead end before END
            "BEGIN:\n  _: a\na: {}\nEND: {}\n",
            // cycle before END
            "BEGIN:\n  _: a\na:\n  _: b\nb:\n  _: a\nEND: {}\n",
            // base-shape: missing END (not N118's finding)
            "BEGIN:\n  _: a\na: {}\n",
            // base-shape: dangling target (not N118's finding)
            "BEGIN:\n  _: ghost\nEND: {}\n",
        ];

        for yaml in CORPUS {
            let raw: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, String>,
            > = serde_yaml::from_str(yaml).expect("corpus yaml parses");
            let rule_flags = super::linearity_finding(&raw).is_some();
            // Only the linearity variants count — base-shape errors
            // (missing END, dangling) belong to the parse gate, not N118.
            let gate_flags = matches!(
                QuestionnaireSpec::from_yaml(yaml),
                Err(WorkflowSpecError::QuestionnaireCondition { .. }
                    | WorkflowSpecError::QuestionnaireOffChain { .. }
                    | WorkflowSpecError::QuestionnaireChainBroken { .. })
            );
            assert_eq!(
                rule_flags, gate_flags,
                "N118 drifted from QuestionnaireSpec::validate for:\n{yaml}"
            );
        }
    }
}
