//! `N104` — questionnaire states must reference valid question codes
//! and workflow states must compose known workflow step prefixes.
//!
//! Questionnaire state name shape: `<question_code>__<discriminator>`
//! (the `__<discriminator>` part is optional). The prefix before the
//! first `__` must appear in the configured valid-codes set. Workflow
//! state names use the same discriminator convention, but their prefix
//! must come from the reusable workflow-step catalog. The sentinel
//! states `BEGIN` and `END` are exempt.
//!
//! Both the `questionnaire:` and `workflow:` maps in frontmatter
//! are validated; both must declare a `BEGIN` state and reach
//! `END` from at least one transition.

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F104FlowQuestionCodes {
    valid_codes: HashSet<String>,
}

impl F104FlowQuestionCodes {
    pub const CODE: &'static str = "N104";

    #[must_use]
    pub fn new<I, S>(codes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            valid_codes: codes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

impl Rule for F104FlowQuestionCodes {
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

        let mut violations = Vec::new();
        let Some(questionnaire) = parsed.questionnaire else {
            violations.push(violation(file, "Missing required `questionnaire` key"));
            return violations;
        };
        let Some(workflow) = parsed.workflow else {
            violations.push(violation(file, "Missing required `workflow` key"));
            return violations;
        };
        self.validate_questionnaire(file, &questionnaire, &mut violations);
        Self::validate_workflow(file, &workflow, &mut violations);
        violations
    }
}

impl F104FlowQuestionCodes {
    fn validate_common_shape(
        file: &SourceFile,
        map: &BTreeMap<String, BTreeMap<String, String>>,
        map_name: &str,
        violations: &mut Vec<Violation>,
    ) -> bool {
        if !map.contains_key("BEGIN") {
            violations.push(violation(
                file,
                format!("{map_name} is missing required BEGIN state"),
            ));
            return false;
        }
        let reaches_end = map.values().any(|t| t.values().any(|n| n == "END"));
        if !reaches_end {
            violations.push(violation(
                file,
                format!("{map_name} is missing required END state"),
            ));
            return false;
        }
        true
    }

    fn validate_questionnaire(
        &self,
        file: &SourceFile,
        map: &BTreeMap<String, BTreeMap<String, String>>,
        violations: &mut Vec<Violation>,
    ) {
        if !Self::validate_common_shape(file, map, "questionnaire", violations) {
            return;
        }
        if self.valid_codes.is_empty() {
            // No registry provided — fall back to structural checks only
            // (BEGIN/END presence), matching the behavior callers get
            // when the default factory is used without supplying codes.
            return;
        }
        for state in map.keys() {
            if state == "BEGIN" || state == "END" {
                continue;
            }
            let prefix = state.split_once("__").map_or(state.as_str(), |(p, _)| p);
            if !self.valid_codes.contains(prefix) {
                violations.push(violation(
                    file,
                    format!("Invalid question code: `{prefix}` (from state `{state}`)"),
                ));
            }
        }
    }

    fn validate_workflow(
        file: &SourceFile,
        map: &BTreeMap<String, BTreeMap<String, String>>,
        violations: &mut Vec<Violation>,
    ) {
        if !Self::validate_common_shape(file, map, "workflow", violations) {
            return;
        }
        for state in map.keys() {
            if state == "BEGIN" || state == "END" {
                continue;
            }
            let prefix = state.split_once("__").map_or(state.as_str(), |(p, _)| p);
            if !valid_workflow_step_prefix(prefix) {
                violations.push(violation(
                    file,
                    format!("Invalid workflow step prefix: `{prefix}` (from state `{state}`)"),
                ));
            }
        }
    }
}

/// Whether `prefix` is an allowed workflow-step prefix. Delegates to the
/// single source of truth, the [`crate::workflow_steps`] catalog (which
/// also handles the `_signature` / `_signatures` suffix family), so the
/// allow-list and the hover descriptions can never drift apart.
#[must_use]
pub fn valid_workflow_step_prefix(prefix: &str) -> bool {
    crate::workflow_steps::is_allowed_prefix(prefix)
}

fn violation(file: &SourceFile, message: impl Into<String>) -> Violation {
    Violation {
        code: F104FlowQuestionCodes::CODE,
        path: file.path.clone(),
        line: 1,
        range: line_byte_range(&file.contents, 1),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{valid_workflow_step_prefix, F104FlowQuestionCodes};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    const VALID_CODES: &[&str] = &["trustee_name", "beneficiary_name"];

    fn rule() -> F104FlowQuestionCodes {
        F104FlowQuestionCodes::new(VALID_CODES.iter().copied())
    }

    #[test]
    fn passes_on_clean_questionnaire_and_workflow() {
        let body = "---
title: T
questionnaire:
  BEGIN:
    created: trustee_name
  trustee_name:
    answered: beneficiary_name
  beneficiary_name:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: staff_review
  staff_review:
    approved: END
  END: {}
---
";
        let violations = rule().lint(&file(body));
        assert!(violations.is_empty(), "got {violations:?}");
    }

    #[test]
    fn no_frontmatter_means_no_violation() {
        assert!(rule().lint(&file("just body")).is_empty());
    }

    #[test]
    fn missing_questionnaire_key_is_a_violation() {
        let body = "---\nworkflow:\n  BEGIN:\n    a: END\n  END: {}\n---\n";
        let v = rule().lint(&file(body));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("Missing required `questionnaire`"));
    }

    #[test]
    fn missing_workflow_key_is_a_violation() {
        let body = "---\nquestionnaire:\n  BEGIN:\n    a: END\n  END: {}\n---\n";
        let v = rule().lint(&file(body));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("Missing required `workflow`"));
    }

    #[test]
    fn missing_begin_state_in_either_map_is_a_violation() {
        let body = "---
questionnaire:
  trustee_name:
    answered: END
  END: {}
workflow:
  BEGIN:
    a: END
  END: {}
---
";
        let v = rule().lint(&file(body));
        assert!(v
            .iter()
            .any(|x| x.message.contains("questionnaire") && x.message.contains("BEGIN")));
    }

    #[test]
    fn flags_state_referencing_unknown_question_code() {
        let body = "---
questionnaire:
  BEGIN:
    created: not_a_valid_code
  not_a_valid_code:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: staff_review
  staff_review:
    approved: END
  END: {}
---
";
        let v = rule().lint(&file(body));
        assert!(v.iter().any(|x| x.message.contains("Invalid question code")
            && x.message.contains("not_a_valid_code")));
    }

    #[test]
    fn double_underscore_suffix_is_stripped_for_code_lookup() {
        let body = "---
questionnaire:
  BEGIN:
    created: trustee_name__for_grantor
  trustee_name__for_grantor:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: staff_review
  staff_review:
    approved: END
  END: {}
---
";
        assert!(rule().lint(&file(body)).is_empty());
    }

    #[test]
    fn workflow_states_are_validated_against_step_prefixes_not_question_codes() {
        let body = "---
title: T
questionnaire:
  BEGIN:
    created: trustee_name
  trustee_name:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: document_open__trust_pdf
  document_open__trust_pdf:
    persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
  END: {}
---
";
        assert!(rule().lint(&file(body)).is_empty());
    }

    #[test]
    fn workflow_signature_suffixes_are_known_steps() {
        let body = "---
title: T
questionnaire:
  BEGIN:
    created: trustee_name
  trustee_name:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: member_signatures
  member_signatures:
    signed: staff_review
  staff_review:
    approved: END
  END: {}
---
";
        assert!(rule().lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_workflow_states_outside_the_step_catalog() {
        let body = "---
title: T
questionnaire:
  BEGIN:
    created: trustee_name
  trustee_name:
    answered: END
  END: {}
workflow:
  BEGIN:
    created: bespoke_magic
  bespoke_magic:
    done: END
  END: {}
---
";
        let v = rule().lint(&file(body));
        assert!(v.iter().any(|x| x
            .message
            .contains("Invalid workflow step prefix: `bespoke_magic`")));
    }

    #[test]
    fn n104_accepts_every_engine_step_prefix() {
        // N104's allow-list is the workflow_steps catalog, which the
        // catalog's own drift test pins to workflows::step::STEP_PREFIXES.
        // This guards the N104 entry point specifically, including the
        // `_signature` suffix family.
        for (prefix, _) in workflows::step::STEP_PREFIXES {
            if *prefix == "_signature" {
                assert!(
                    valid_workflow_step_prefix("member_signatures"),
                    "signature suffix family should be accepted by N104",
                );
                continue;
            }
            assert!(
                valid_workflow_step_prefix(prefix),
                "workflow engine prefix `{prefix}` is not accepted by N104",
            );
        }
    }
}
