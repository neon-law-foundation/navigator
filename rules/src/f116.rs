//! `N116` — a `staff_review` must gate every outbound submission.
//!
//! On **every path from `BEGIN`**, a `staff_review` state must be
//! reached before any **outbound submission** step: mail to a party or a
//! filing with a government office (`mailroom_send`, `mailroom_receive`,
//! `certified_mail`, `e_filing`, `filing__*`). This is the
//! authoring-time / LSP mirror of the runtime guardrail
//! [`workflows::guardrail::staff_review_precedes_submission`]: the same
//! "no outbound act without a human" invariant the engine enforces at
//! run time, hoisted to validation so a template that mails or files
//! without an attorney review is a red squiggle in the editor and a
//! `navigator validate` error — before it ever runs.
//!
//! The gated set is deliberately the **submission** set, not every
//! binding-looking step:
//!
//! - `document_open__*` renders a PDF for attorney review
//!   ("review-ready, never auto-filed") — it is the *start* of a gated
//!   path, not the outbound act. Gating it would wrongly flag the
//!   Nautilus letters, which render *then* review *then* mail.
//! - `sent_for_signature__*` is the e-signature send. Its gating is
//!   deliberately **non-universal** at run time — the Nautilus
//!   settlement letter has the client authorize the firm *before*
//!   `staff_review` gates the outbound mailing — so it stays out of this
//!   universal rule. The engine covers the engagement-document signature
//!   gate separately.
//!
//! The gated prefix set is kept in lockstep with the engine by the
//! `outbound_set_matches_workflows_guardrail` drift test below.
//!
//! Files without frontmatter or without a `workflow:` key are silently
//! skipped, like the other workflow-shape rules.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct F116StaffReviewGatesSubmission;

impl F116StaffReviewGatesSubmission {
    pub const CODE: &'static str = "N116";
}

#[derive(Debug, Deserialize)]
struct FrontmatterShape {
    #[serde(default)]
    workflow: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

/// The bare step prefix (the part before a `__discriminator`).
fn prefix_of(state: &str) -> &str {
    state.split_once("__").map_or(state, |(p, _)| p)
}

/// A state that submits a document to the outside world — mail to a
/// party or a filing with a government office. Mirrors
/// [`workflows::guardrail::is_submission_state`]; the drift test locks
/// the two definitions together.
fn is_submission(state: &str) -> bool {
    matches!(
        prefix_of(state),
        "mailroom_send" | "mailroom_receive" | "certified_mail" | "e_filing"
    ) || state.starts_with("filing__")
}

/// The mandatory human gate (`staff_review*`).
fn is_review(state: &str) -> bool {
    prefix_of(state) == "staff_review"
}

impl Rule for F116StaffReviewGatesSubmission {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let Some(fm) = crate::frontmatter::extract(&file.contents) else {
            return Vec::new();
        };
        let Ok(parsed) = serde_yaml::from_str::<FrontmatterShape>(fm) else {
            return Vec::new();
        };
        let Some(workflow) = parsed.workflow else {
            return Vec::new();
        };

        ungated_submissions(&workflow)
            .into_iter()
            .map(|state| {
                let line = workflow_state_line(&file.contents, &state);
                Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line,
                    range: line_byte_range(&file.contents, line),
                    message: format!(
                        "outbound submission `{state}` is reachable from BEGIN without a preceding \
                         `staff_review` — every mail/filing step must be attorney-reviewed first"
                    ),
                }
            })
            .collect()
    }
}

/// 1-based line of the indented `workflow:` state key `state`, so the
/// squiggle lands on the offending step rather than the frontmatter
/// delimiter. Falls back to line 1 when the key can't be located.
fn workflow_state_line(contents: &str, state: &str) -> usize {
    let key = format!("{state}:");
    for (idx, raw) in contents.lines().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.len() < raw.len() && trimmed == key {
            return idx + 1;
        }
    }
    1
}

/// Every outbound-submission state reachable from `BEGIN` without a
/// `staff_review` on the path before it. BFS over `(state, seen_review)`
/// pairs — the review flag is part of the visit key so a state reached
/// both with and without a prior review is explored under both, and a
/// cycle can't loop forever.
fn ungated_submissions(workflow: &BTreeMap<String, BTreeMap<String, String>>) -> BTreeSet<String> {
    let mut offending = BTreeSet::new();
    let mut visited: BTreeSet<(String, bool)> = BTreeSet::new();
    let mut queue: Vec<(String, bool)> = vec![("BEGIN".to_string(), false)];
    while let Some((node, seen_review)) = queue.pop() {
        if !visited.insert((node.clone(), seen_review)) {
            continue;
        }
        if !seen_review && node != "BEGIN" && is_submission(&node) {
            offending.insert(node.clone());
        }
        let downstream_seen = seen_review || is_review(&node);
        if let Some(transitions) = workflow.get(&node) {
            for target in transitions.values() {
                queue.push((target.clone(), downstream_seen));
            }
        }
    }
    offending
}

#[cfg(test)]
mod tests {
    use super::{is_submission, F116StaffReviewGatesSubmission};
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn flags_submission_reached_before_review() {
        let body = "---
workflow:
  BEGIN:
    intake_submitted: mailroom_send__notice
  mailroom_send__notice:
    mailed: END
  END: {}
---
";
        let v = F116StaffReviewGatesSubmission.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "N116");
        assert!(v[0].message.contains("mailroom_send__notice"));
    }

    #[test]
    fn passes_when_review_precedes_submission() {
        let body = "---
workflow:
  BEGIN:
    intake_submitted: staff_review
  staff_review:
    approved: mailroom_send__notice
    rejected: END
  mailroom_send__notice:
    mailed: END
  END: {}
---
";
        assert!(F116StaffReviewGatesSubmission.lint(&file(body)).is_empty());
    }

    #[test]
    fn formation_shape_passes_filing_after_review() {
        // The shipped NV formation shape: staff_review gates the whole
        // render → sign → file tail, so filing__nv_sos is reviewed.
        let body = "---
workflow:
  BEGIN:
    intake_submitted: intake_persisted__organizer
  intake_persisted__organizer:
    articles_rendered: staff_review
  staff_review:
    approved: document_open__articles_pdf
    rejected: END
  document_open__articles_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: filing__nv_sos
    signature_declined: END
  filing__nv_sos:
    filed: END
  END: {}
---
";
        assert!(F116StaffReviewGatesSubmission.lint(&file(body)).is_empty());
    }

    #[test]
    fn settlement_letter_shape_passes_client_authorizes_then_review_gates_mail() {
        // The deliberate Nautilus settlement shape: the client authorizes
        // the firm (sent_for_signature__settlement) BEFORE staff_review,
        // but staff_review gates the outbound mailing. N116 must NOT flag
        // it — the outbound act (mailroom_send) is reviewed.
        let body = "---
workflow:
  BEGIN:
    intake_submitted: document_open__settlement_letter
  document_open__settlement_letter:
    pdf_persisted: sent_for_signature__settlement
  sent_for_signature__settlement:
    client_authorized: staff_review
    client_declined: END
  staff_review:
    approved: mailroom_send__settlement_letter
    rejected: END
  mailroom_send__settlement_letter:
    mailed: END
  END: {}
---
";
        assert!(
            F116StaffReviewGatesSubmission.lint(&file(body)).is_empty(),
            "settlement letter gates its outbound mailing behind review"
        );
    }

    #[test]
    fn render_and_signature_are_not_submissions() {
        // document_open + sent_for_signature with NO review at all: neither
        // is an outbound submission, so N116 stays silent (that gap is the
        // engine's engagement signature gate, not this universal rule).
        let body = "---
workflow:
  BEGIN:
    created: document_open__retainer_pdf
  document_open__retainer_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: END
  END: {}
---
";
        assert!(F116StaffReviewGatesSubmission.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_branch_that_bypasses_review() {
        // One branch renders → review → file (ok); the other files
        // directly (violation). The whole workflow must fail.
        let body = "---
workflow:
  BEGIN:
    created: document_open__form
  document_open__form:
    reviewed: staff_review
    rushed: filing__nv_sos
  staff_review:
    approved: filing__nv_sos
    rejected: END
  filing__nv_sos:
    filed: END
  END: {}
---
";
        let v = F116StaffReviewGatesSubmission.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("filing__nv_sos"));
    }

    #[test]
    fn each_outbound_prefix_is_gated() {
        for target in [
            "certified_mail",
            "e_filing__uscis",
            "filing__nv_sos",
            "mailroom_send__x",
            "mailroom_receive__notice",
        ] {
            let body = format!(
                "---\nworkflow:\n  BEGIN:\n    go: {target}\n  {target}:\n    done: END\n  END: {{}}\n---\n"
            );
            let v = F116StaffReviewGatesSubmission.lint(&file(&body));
            assert_eq!(v.len(), 1, "{target} should be gated: {v:?}");
        }
    }

    #[test]
    fn cycle_does_not_hang_and_still_flags() {
        let body = "---
workflow:
  BEGIN:
    a: loop
  loop:
    back: BEGIN
    out: certified_mail
  certified_mail:
    done: END
  END: {}
---
";
        let v = F116StaffReviewGatesSubmission.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("certified_mail"));
    }

    #[test]
    fn no_frontmatter_or_workflow_means_no_violation() {
        assert!(F116StaffReviewGatesSubmission
            .lint(&file("just body"))
            .is_empty());
        let no_wf = "---\ntitle: T\n---\nbody\n";
        assert!(F116StaffReviewGatesSubmission.lint(&file(no_wf)).is_empty());
    }

    #[test]
    fn violation_points_at_the_offending_state_line() {
        let body = "---
workflow:
  BEGIN:
    go: certified_mail
  certified_mail:
    done: END
  END: {}
---
";
        let v = F116StaffReviewGatesSubmission.lint(&file(body));
        assert_eq!(v.len(), 1);
        // `certified_mail:` is line 5 of the body.
        assert_eq!(v[0].line, 5, "{v:?}");
    }

    #[test]
    fn is_error_severity() {
        use crate::{severity_for_code, Severity};
        assert_eq!(severity_for_code("N116"), Severity::Error);
    }

    #[test]
    fn outbound_set_matches_workflows_guardrail() {
        // Drift lock: N116's outbound-submission classification must stay
        // identical to the runtime guardrail it mirrors. If the engine
        // adds or moves a submission prefix, this fails until N116 keeps
        // up — one definition of "outbound", two enforcement points.
        use workflows::guardrail::is_submission_state;
        use workflows::spec::StateName;
        for state in [
            "mailroom_send",
            "mailroom_send__notice",
            "mailroom_receive",
            "mailroom_receive__biometrics",
            "certified_mail",
            "certified_mail__demand",
            "e_filing",
            "e_filing__uscis",
            "filing__nv_sos",
            "document_open__retainer_pdf",
            "sent_for_signature__pending",
            "sent_for_signature__settlement",
            "staff_review",
            "staff_review__for_grantor",
            "client_review",
            "document_drafts__estate",
            "intake_persisted__client",
            "BEGIN",
            "END",
        ] {
            assert_eq!(
                is_submission(state),
                is_submission_state(&StateName::from(state)),
                "N116 outbound set drifted from workflows::guardrail::is_submission_state for `{state}`",
            );
        }
    }
}
