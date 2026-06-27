//! `N112` — a workflow step is allowed but its automation is not built
//! yet.
//!
//! This is an *advisory* ([`crate::Severity::Warning`], yellow in the
//! editor), not a blocker: the step is a legitimate member of the
//! workflow-step catalog ([`crate::f104::VALID_WORKFLOW_STEP_PREFIXES`]),
//! but the firm has not yet built the automation behind it, so a
//! notation that uses it advances only as far as the human gate. The
//! companion red error is `N104` (a step that isn't in the catalog at
//! all).
//!
//! The not-built set is [`WORKFLOW_STEPS_NOT_BUILT`]. As each step's
//! automation lands, drop it from that list and the yellow squiggle
//! disappears; when a new allowed-but-stubbed step is introduced, add
//! it. Today the only allowed workflow step is `staff_review`, and its
//! automation is not built, so it is the sole entry.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

/// Workflow-step prefixes that are allowed (in
/// [`crate::f104::VALID_WORKFLOW_STEP_PREFIXES`]) but whose automation
/// is not built yet. A `workflow:` state on one of these prefixes earns
/// a yellow `N112` advisory.
pub const WORKFLOW_STEPS_NOT_BUILT: &[&str] = &["staff_review"];

pub struct F112WorkflowStepNotBuilt;

impl F112WorkflowStepNotBuilt {
    pub const CODE: &'static str = "N112";
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

/// True when `prefix` names a step that is allowed but not built yet.
#[must_use]
pub fn workflow_step_not_built(prefix: &str) -> bool {
    WORKFLOW_STEPS_NOT_BUILT.contains(&prefix)
}

impl Rule for F112WorkflowStepNotBuilt {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        "Workflow step is allowed but its automation is not built yet"
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = frontmatter::extract(&file.contents) else {
            return Vec::new();
        };
        let Ok(parsed) = serde_yaml::from_str::<FrontmatterShape>(fm) else {
            return Vec::new();
        };
        let Some(workflow) = parsed.workflow else {
            return Vec::new();
        };

        let mut violations = Vec::new();
        for state in workflow.keys() {
            if state == "BEGIN" || state == "END" {
                continue;
            }
            let prefix = state.split_once("__").map_or(state.as_str(), |(p, _)| p);
            if workflow_step_not_built(prefix) {
                let line = workflow_state_line(&file.contents, state);
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range: line_byte_range(&file.contents, line),
                    message: format!(
                        "workflow step `{prefix}` is allowed but its automation is not built yet \
                         (from state `{state}`)"
                    ),
                });
            }
        }
        violations
    }
}

/// 1-based line of the indented `workflow:` state key `state` in the raw
/// source, so the squiggle lands on the step itself rather than the
/// frontmatter delimiter. Falls back to line 1 if the key can't be
/// located (e.g. an unusual indentation the parser still accepted).
fn workflow_state_line(contents: &str, state: &str) -> usize {
    let key = format!("{state}:");
    for (idx, raw) in contents.lines().enumerate() {
        // A state key is nested (indented) under `workflow:`, never at
        // column zero — that guard avoids matching a same-named line
        // that isn't a mapping key.
        let trimmed = raw.trim_start();
        if trimmed.len() < raw.len() && trimmed == key {
            return idx + 1;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::F112WorkflowStepNotBuilt;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    const STAFF_REVIEW_WORKFLOW: &str = "---
title: T
workflow:
  BEGIN:
    intake_submitted: staff_review
  staff_review:
    approved: END
    rejected: END
  END: {}
---
";

    #[test]
    fn warns_on_a_staff_review_state() {
        let v = F112WorkflowStepNotBuilt.lint(&file(STAFF_REVIEW_WORKFLOW));
        assert_eq!(v.len(), 1, "exactly one not-built advisory, got {v:?}");
        assert_eq!(v[0].code, "N112");
        assert!(v[0].message.contains("staff_review"));
        assert!(v[0].message.contains("not built"));
    }

    #[test]
    fn advisory_points_at_the_staff_review_line_not_line_one() {
        let v = F112WorkflowStepNotBuilt.lint(&file(STAFF_REVIEW_WORKFLOW));
        // `staff_review:` is the 6th line of the body above.
        assert_eq!(v[0].line, 6, "squiggle should land on the step, got {v:?}");
    }

    #[test]
    fn does_not_warn_on_built_steps() {
        // document_open / sent_for_signature are implemented steps: no
        // advisory, even though they're not staff_review.
        let body = "---
title: T
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
        assert!(F112WorkflowStepNotBuilt.lint(&file(body)).is_empty());
    }

    #[test]
    fn warns_on_a_discriminated_staff_review_state() {
        let body = "---
title: T
workflow:
  BEGIN:
    created: staff_review__for_grantor
  staff_review__for_grantor:
    approved: END
  END: {}
---
";
        let v = F112WorkflowStepNotBuilt.lint(&file(body));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("staff_review__for_grantor"));
    }

    #[test]
    fn no_workflow_means_no_advisory() {
        assert!(F112WorkflowStepNotBuilt.lint(&file("just body")).is_empty());
        let only_q = "---\nquestionnaire:\n  BEGIN:\n    a: END\n  END: {}\n---\n";
        assert!(F112WorkflowStepNotBuilt.lint(&file(only_q)).is_empty());
    }

    #[test]
    fn advisory_is_warning_severity() {
        use crate::{severity_for_code, Severity};
        assert_eq!(severity_for_code("N112"), Severity::Warning);
    }
}
