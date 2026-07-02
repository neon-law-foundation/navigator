//! `N117` — `custom_text__*` states must not smuggle glossary-backed nouns.
//!
//! `custom_text` is an escape hatch for document-specific primitive prose,
//! not a way to model durable people, contact facts, legal actors, projects,
//! or product scope outside the glossary-backed typed states. This rule keeps
//! the obvious cases deterministic: names/emails and legal actor roles belong
//! to `person__*` or a richer typed model, and product descriptions belong to
//! the product/project render context rather than an intake free-text answer.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct F117GlossaryBackedCustomText;

impl F117GlossaryBackedCustomText {
    pub const CODE: &'static str = "N117";
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    questionnaire: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

const ALLOWED_CUSTOM_TEXT_ROLES: &[&str] = &[
    "alleged_account",
    "annual_salary",
    "contractor_rate",
    "contractor_term",
    "disputed_reason",
    "dissolution_reason",
    "file_retention",
    "fundraising_activities",
    "matter_summary",
    "mission_statement",
    "next_obligation",
    "pay_schedule",
    "report_error",
    "revenue_strategy",
    "settlement_target",
    "settlement_terms",
    "termination_notice_days",
    "time_outside_us",
    "tradeline",
    "trust_property",
    "worker_duties",
];

const GLOSSARY_BACKED_TOKENS: &[&str] = &[
    "agent",
    "beneficiary",
    "client",
    "email",
    "executor",
    "guardian",
    "name",
    "testator",
    "trustee",
];

const GLOSSARY_BACKED_ROLES: &[&str] = &["product_description"];

impl Rule for F117GlossaryBackedCustomText {
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

        questionnaire
            .keys()
            .filter_map(|state| custom_text_role(state))
            .filter(|role| !ALLOWED_CUSTOM_TEXT_ROLES.contains(role))
            .filter(|role| is_glossary_backed_role(role))
            .map(|role| {
                let state = format!("custom_text__{role}");
                let line = questionnaire_state_line(&file.contents, &state);
                Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range: line_byte_range(&file.contents, line),
                    message: format!(
                        "`{state}` models glossary-backed vocabulary through `custom_text`; \
                         use a typed questionnaire state such as `person__*`, `entity__*`, \
                         `project__*`, or product/project render context instead"
                    ),
                }
            })
            .collect()
    }
}

fn custom_text_role(state: &str) -> Option<&str> {
    state.strip_prefix("custom_text__")
}

fn is_glossary_backed_role(role: &str) -> bool {
    GLOSSARY_BACKED_ROLES.contains(&role)
        || role
            .split('_')
            .any(|token| GLOSSARY_BACKED_TOKENS.contains(&token))
}

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
    use super::F117GlossaryBackedCustomText;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn flags_custom_text_name_and_email_shapes() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__client_name
  custom_text__client_name:
    _: custom_text__client_email
  custom_text__client_email:
    _: END
  END: {}
---
";
        let v = F117GlossaryBackedCustomText.lint(&file(body));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().any(|v| v.message.contains("client_name")));
        assert!(v.iter().any(|v| v.message.contains("client_email")));
    }

    #[test]
    fn flags_custom_text_agent_and_trustee_shapes() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__healthcare_agent
  custom_text__healthcare_agent:
    _: custom_text__successor_trustee
  custom_text__successor_trustee:
    _: END
  END: {}
---
";
        let v = F117GlossaryBackedCustomText.lint(&file(body));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().any(|v| v.message.contains("healthcare_agent")));
        assert!(v.iter().any(|v| v.message.contains("successor_trustee")));
    }

    #[test]
    fn flags_product_description_as_render_context() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__product_description
  custom_text__product_description:
    _: END
  END: {}
---
";
        let v = F117GlossaryBackedCustomText.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("product/project render context"));
    }

    #[test]
    fn allowlisted_free_text_primitives_pass() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__settlement_terms
  custom_text__settlement_terms:
    _: custom_text__disputed_reason
  custom_text__disputed_reason:
    _: END
  END: {}
---
";
        assert!(F117GlossaryBackedCustomText.lint(&file(body)).is_empty());
    }

    #[test]
    fn violation_points_at_the_offending_questionnaire_state_line() {
        let body = "---
questionnaire:
  BEGIN:
    _: custom_text__registered_agent
  custom_text__registered_agent:
    _: END
  END: {}
---
";
        let v = F117GlossaryBackedCustomText.lint(&file(body));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 5, "{v:?}");
    }

    #[test]
    fn no_frontmatter_or_questionnaire_means_no_violation() {
        assert!(F117GlossaryBackedCustomText
            .lint(&file("just body"))
            .is_empty());
        assert!(F117GlossaryBackedCustomText
            .lint(&file("---\ntitle: T\n---\nbody\n"))
            .is_empty());
    }

    #[test]
    fn is_error_severity() {
        use crate::{severity_for_code, Severity};
        assert_eq!(severity_for_code("N117"), Severity::Error);
    }
}
