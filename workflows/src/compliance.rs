//! `mailroom_send` / `certified_mail` / `e_filing` / `filing__*` step
//! dispatch — record an outbound compliance submission.
//!
//! Mirrors [`crate::email`] and [`crate::document`]: the caller threads
//! a [`CompliancePayload`] through the signal `value`, and the worker
//! (the `workflows-service` `NotationService` in prod, the in-process
//! [`crate::DispatchingRuntime`] in dev/tests) records a `filings` row
//! when a transition lands on a submission state.
//!
//! ## The review gate
//!
//! The side effect — a row in `filings`, the firm's proof of what was
//! filed and with which office — fires *only* on entering a submission
//! state, and the workflow spec guarantees no submission state is
//! reachable without first crossing `staff_review`
//! ([`crate::staff_review_precedes_submission`]). So a `filings` row
//! means a licensed attorney approved the specific submission: nothing
//! is mailed or filed with a government office unreviewed (N106).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::spec::StateName;
use crate::step::{step_kind_for, StepKind};

/// What to record for one submission. Threaded as the JSON `value` of
/// the signal that lands on the submission state. The `kind` is derived
/// from the state name (not carried here); only the office, summary, and
/// optional provider reference are caller-supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompliancePayload {
    /// Recipient office or party (e.g. `Nevada Secretary of State`).
    pub office: String,
    /// Human-readable summary of what was submitted.
    pub summary: String,
    /// Provider/office tracking reference, when known at submit time.
    #[serde(default)]
    pub reference: Option<String>,
}

/// Errors from recording a compliance submission.
#[derive(Debug, thiserror::Error)]
pub enum ComplianceError {
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// True for submission steps that record a durable `filings` row on
/// entry: `mailroom_send`, `certified_mail`, `e_filing`, `filing__*`.
/// `mailroom_receive` (inbound) and `notarization` (a human act before a
/// notary) are excluded — they have no worker side effect here.
#[must_use]
pub fn is_dispatched_submission(state: &StateName) -> bool {
    matches!(
        step_kind_for(state),
        Some(
            StepKind::MailroomSend | StepKind::CertifiedMail | StepKind::EFiling | StepKind::Filing
        )
    )
}

/// Record the submission in `filings`. The single side effect of a
/// submission step; callers wrap it in `ctx.run` (worker) or call it
/// inline (`DispatchingRuntime`). The `filings` row's `kind` is the
/// state-name prefix (`mailroom_send`, `filing`, …).
pub async fn dispatch_compliance(
    db: &store::Db,
    notation_id: Uuid,
    state: &StateName,
    payload: &CompliancePayload,
) -> Result<(), ComplianceError> {
    let submitted_at = chrono::Utc::now().to_rfc3339();
    store::filings::record(
        db,
        &store::filings::NewFiling {
            notation_id,
            kind: state.prefix(),
            office: &payload.office,
            summary: &payload.summary,
            reference: payload.reference.as_deref(),
            submitted_at: &submitted_at,
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_dispatched_submission;
    use crate::spec::StateName;

    #[test]
    fn outbound_submission_states_are_dispatched() {
        assert!(is_dispatched_submission(&StateName::from("mailroom_send")));
        assert!(is_dispatched_submission(&StateName::from(
            "certified_mail__nv_sos"
        )));
        assert!(is_dispatched_submission(&StateName::from(
            "e_filing__nv_sos"
        )));
        assert!(is_dispatched_submission(&StateName::from("filing__nv_sos")));
    }

    #[test]
    fn human_and_inbound_states_are_not_dispatched() {
        // notarization is a human act; mailroom_receive is inbound; the
        // signature/review/system states are not submissions.
        assert!(!is_dispatched_submission(&StateName::from("notarization")));
        assert!(!is_dispatched_submission(&StateName::from(
            "mailroom_receive"
        )));
        assert!(!is_dispatched_submission(&StateName::from(
            "staff_review__articles"
        )));
        assert!(!is_dispatched_submission(&StateName::from("END")));
    }
}
