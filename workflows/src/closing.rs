//! Matter-close dispatch — flip the bound Project to `closed` when the
//! firm signs the closing letter.
//!
//! Mirrors [`crate::compliance`]: a worker side effect that fires on a
//! particular kind of transition. Where a submission step records a
//! `filings` row, the firm signing the closing letter
//! (`firm_signature__*`, the [`StepKind::FirmSignature`] step) flips the
//! matter `open` → `closed`. Callers wrap [`close_matter`] in `ctx.run`
//! so a replay reuses the outcome rather than re-updating.
//!
//! A matter *opens* on the client's signed retainer and *closes* on the
//! firm's signed closing letter — so the close is keyed off the
//! firm-signature step, the symmetric bookend of the respondent
//! `_signature` family.

use uuid::Uuid;

use crate::spec::StateName;
use crate::step::{step_kind_for, StepKind};

/// Errors from closing a matter.
#[derive(Debug, thiserror::Error)]
pub enum CloseError {
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// True when leaving `state` means the firm has signed the closing
/// letter — the act that closes the matter. That is any
/// `firm_signature__*` step: the [`StepKind::FirmSignature`] kind exists
/// only for the firm-signed closing letter (see `step.rs`).
#[must_use]
pub fn closes_matter(state: &StateName) -> bool {
    matches!(step_kind_for(state), Some(StepKind::FirmSignature))
}

/// Flip the matter `notation_id` belongs to from `open` to `closed`.
/// The single side effect of the firm signing the closing letter;
/// callers wrap it in `ctx.run`. Idempotent and monotonic — see
/// [`store::projects::close_for_notation`].
pub async fn close_matter(db: &store::Db, notation_id: Uuid) -> Result<(), CloseError> {
    store::projects::close_for_notation(db, notation_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::closes_matter;
    use crate::spec::StateName;

    #[test]
    fn firm_signature_states_close_the_matter() {
        assert!(closes_matter(&StateName::from(
            "firm_signature__closing_letter"
        )));
        assert!(closes_matter(&StateName::from("firm_signature")));
    }

    #[test]
    fn other_states_do_not_close_the_matter() {
        // The respondent signing (retainer) opens, it doesn't close; a
        // staff review, a render, and END are not the firm's signature.
        assert!(!closes_matter(&StateName::from("client_signature")));
        assert!(!closes_matter(&StateName::from("staff_review")));
        assert!(!closes_matter(&StateName::from(
            "document_open__closing_letter"
        )));
        assert!(!closes_matter(&StateName::end()));
    }
}
