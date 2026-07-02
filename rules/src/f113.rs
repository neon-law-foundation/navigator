//! `N113` — a questionnaire state's `<type>` must be a registered question
//! type.
//!
//! The typed grammar for a questionnaire state is `<type>__<role>` (e.g.
//! `entity__company`, `custom_text__mission`, `address__for_trustor`). The
//! `<type>` prefix must be one of the closed [`REGISTERED_QUESTION_TYPES`] —
//! the vocabulary `store::question_registry::QuestionType` defines and a
//! `cli` parity test pins this list to. A typed state naming an unknown
//! `<type>` is flagged so authors migrate to the registered grammar rather
//! than inventing ad-hoc typed states.
//!
//! Only questionnaire states using the typed grammar (containing `__`) are
//! checked; bare question-code states and workflow states stay `N104`'s
//! domain. The "type → real `store::entity`" half is grounded once by the
//! Slice-4 registry test, not re-checked per file.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

/// The closed set of `<type>` tokens a typed questionnaire state may use —
/// the string form of every `store::question_registry::QuestionType`
/// variant. Duplicated here so `rules` (and thus the LSP) stays free of the
/// `store`/`sea_orm` dependency; `cli/tests/question_type_registry_parity`
/// grounds this list to the canonical enum so the two cannot drift.
pub const REGISTERED_QUESTION_TYPES: &[&str] = &[
    // record — singular
    "person",
    "entity",
    "address",
    "role",
    "filing",
    "credential",
    "disclosure",
    "issuance",
    "signature",
    "notarization",
    // record — aggregate
    "people",
    "entities",
    "addresses",
    "roles",
    "filings",
    "credentials",
    "disclosures",
    "issuances",
    // reference — singular
    "jurisdiction",
    "country",
    "entity_type",
    "product",
    "statute",
    "project",
    // reference — aggregate
    "jurisdictions",
    "entity_types",
    "products",
    "statutes",
    // custom primitives
    "custom_text",
    "custom_phone",
    "custom_yes_no",
    "custom_single_choice",
    "custom_multiple_choice",
    "custom_usd",
    "custom_datetime",
];

pub struct F113TypeGrounding;

impl F113TypeGrounding {
    pub const CODE: &'static str = "N113";
}

/// A one-line hover/completion description of a registered `<type>` token,
/// or `None` if it is not registered. Sourced from the vocabulary the LSP
/// shares (the shape half without pulling in `store`): custom primitive,
/// aggregate, or singular typed answer.
#[must_use]
pub fn describe_question_type(token: &str) -> Option<String> {
    if !REGISTERED_QUESTION_TYPES.contains(&token) {
        return None;
    }
    let shape = if token.starts_with("custom_") {
        "custom primitive — the value lives in the answer JSON"
    } else if crate::AGGREGATE_QUESTION_TYPES.contains(&token) {
        "aggregate — an array of the singular's shape"
    } else {
        "singular typed answer — grounds to a `store::entity` row"
    };
    Some(format!("`{token}` — registered question type ({shape})"))
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

impl Rule for F113TypeGrounding {
    fn code(&self) -> &'static str {
        Self::CODE
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

        let mut violations = Vec::new();
        for state in questionnaire.keys() {
            if state == "BEGIN" || state == "END" {
                continue;
            }
            // Only the typed grammar (`<type>__<role>`) is grounded; a bare
            // question-code state carries no type claim and is N104's domain.
            let Some((prefix, _role)) = state.split_once("__") else {
                continue;
            };
            if !REGISTERED_QUESTION_TYPES.contains(&prefix) {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: 1,
                    range: line_byte_range(&file.contents, 1),
                    message: format!(
                        "Questionnaire state `{state}` uses unregistered type `{prefix}` — \
                         the `<type>__<role>` grammar requires a registered question type"
                    ),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::F113TypeGrounding;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_registered_typed_states() {
        let body = "---
questionnaire:
  BEGIN:
    _: entity__company
  entity__company:
    _: custom_text__mission
  custom_text__mission:
    _: people__members
  people__members:
    _: END
  END: {}
---
";
        assert!(F113TypeGrounding.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_an_unregistered_typed_state() {
        let body = "---
questionnaire:
  BEGIN:
    _: trustee_name__for_grantor
  trustee_name__for_grantor:
    _: END
  END: {}
---
";
        let v = F113TypeGrounding.lint(&file(body));
        assert_eq!(v.len(), 1, "got {v:?}");
        assert!(v[0].message.contains("unregistered type `trustee_name`"));
    }

    #[test]
    fn bare_states_are_not_type_checked() {
        // A bare question-code state carries no `<type>__<role>` claim; N104
        // validates its code, not N113.
        let body = "---
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: END
  END: {}
---
";
        assert!(F113TypeGrounding.lint(&file(body)).is_empty());
    }

    #[test]
    fn no_frontmatter_means_no_violation() {
        assert!(F113TypeGrounding.lint(&file("just body")).is_empty());
    }

    #[test]
    fn a_renamed_custom_type_is_registered() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_datetime__tax_year
  custom_datetime__tax_year:
    _: custom_single_choice__basis
  custom_single_choice__basis:
    _: END
  END: {}
---
";
        assert!(F113TypeGrounding.lint(&file(body)).is_empty());
    }
}
