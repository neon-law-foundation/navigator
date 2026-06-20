//! `N104` — questionnaire and workflow state names must reference
//! valid question codes.
//!
//! State name shape: `<question_code>__<discriminator>` (the
//! `__<discriminator>` part is optional). The prefix before the
//! first `__` must appear in the configured valid-codes set. The
//! sentinel states `BEGIN` and `END` are exempt.
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
        self.validate_map(file, &questionnaire, "questionnaire", &mut violations);
        self.validate_map(file, &workflow, "workflow", &mut violations);
        violations
    }
}

impl F104FlowQuestionCodes {
    fn validate_map(
        &self,
        file: &SourceFile,
        map: &BTreeMap<String, BTreeMap<String, String>>,
        map_name: &str,
        violations: &mut Vec<Violation>,
    ) {
        if !map.contains_key("BEGIN") {
            violations.push(violation(
                file,
                format!("{map_name} is missing required BEGIN state"),
            ));
            return;
        }
        let reaches_end = map.values().any(|t| t.values().any(|n| n == "END"));
        if !reaches_end {
            violations.push(violation(
                file,
                format!("{map_name} is missing required END state"),
            ));
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
    use super::F104FlowQuestionCodes;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    const VALID_CODES: &[&str] = &["trustee_name", "beneficiary_name", "staff_review"];

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
}
