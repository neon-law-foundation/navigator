//! Inbound-triage classification and statutory-deadline calculation for
//! the Neon Law Nautilus debt-collection shield.
//!
//! Workflow 02 turns an inbound collector email into the right
//! downstream action. The classifier here is pure logic — it reads the
//! subject and body and returns a [`CollectorMailClass`]; [`route`]
//! maps that class to the sub-workflow that handles it. The live
//! `workflows-service` worker calls these over each inbound `.eml` that
//! threads onto an active Nautilus matter.
//!
//! Two rules are load-bearing and grounded in statute:
//!
//! 1. **Litigation is detected first and referred out.** A summons or
//!    lawsuit is classified ahead of every other category so a
//!    settlement or validation phrase buried in a court document can
//!    never mask it. A lawsuit is never answered as correspondence —
//!    it goes to litigation counsel.
//! 2. **The 30-day windows are statutory.** [`DeadlineKind`] carries the
//!    FDCPA §1692g(a) validation window and the FCRA §1681i(a)(1)
//!    reinvestigation period, both 30 days, so workflows 03 and 04
//!    calendar them from one calculator rather than hard-coding the
//!    number twice.

use chrono::{Duration, NaiveDate};

/// Classification of an inbound collector email against an active
/// Nautilus matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectorMailClass {
    /// A summons, complaint, or lawsuit notice. Refer to litigation
    /// counsel — never answered as correspondence.
    LawsuitOrSummons,
    /// The collector's verification of the debt, typically in response
    /// to a §1692g(b) validation request.
    ValidationResponse,
    /// An offer to settle or reduce the balance. Client-directed
    /// settlement (workflow 05), never for a cut.
    SettlementOffer,
    /// A first or routine collection contact. Opens the §1692g 30-day
    /// validation window (workflow 03).
    NewContact,
    /// Anything we cannot confidently route — flag for a staff member.
    Other,
}

/// Phrases that mark a court action. Checked first; a match here wins
/// over every other category.
const LAWSUIT_MARKERS: &[&str] = &[
    "summons",
    "complaint",
    "you are being sued",
    "being sued",
    "notice of lawsuit",
    "civil action",
    "writ of",
    "garnish",
    "judgment",
    "served with",
];

/// Phrases that mark a collector's verification of the debt.
const VALIDATION_MARKERS: &[&str] = &[
    "verification of the debt",
    "verifying the debt",
    "validation of the debt",
    "documentation of the debt",
    "itemization",
    "enclosed is the verification",
    "we have verified",
];

/// Phrases that mark a settlement or balance-reduction offer.
const SETTLEMENT_MARKERS: &[&str] = &[
    "settle",
    "settlement",
    "reduce your balance",
    "reduced balance",
    "lump sum",
    "pay only",
    "discounted payoff",
    "resolve this account for",
];

/// Phrases that mark a routine first/continuing collection contact.
const NEW_CONTACT_MARKERS: &[&str] = &[
    "this is an attempt to collect a debt",
    "amount due",
    "outstanding balance",
    "you owe",
    "first notice",
    "please remit",
    "past due",
];

/// Classify an inbound collector email from its subject and body. The
/// precedence is intentional (lawsuit → validation → settlement → new
/// contact → other); see the module docs.
#[must_use]
pub fn classify(subject: &str, body: &str) -> CollectorMailClass {
    let hay = format!("{} {}", subject.to_lowercase(), body.to_lowercase());
    let has = |needles: &[&str]| needles.iter().any(|n| hay.contains(n));
    if has(LAWSUIT_MARKERS) {
        CollectorMailClass::LawsuitOrSummons
    } else if has(VALIDATION_MARKERS) {
        CollectorMailClass::ValidationResponse
    } else if has(SETTLEMENT_MARKERS) {
        CollectorMailClass::SettlementOffer
    } else if has(NEW_CONTACT_MARKERS) {
        CollectorMailClass::NewContact
    } else {
        CollectorMailClass::Other
    }
}

/// Where a classified inbound message is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageRoute {
    /// Refer to litigation counsel (Sethi Legal). A lawsuit or summons
    /// is never answered as correspondence.
    ReferLitigation,
    /// Invoke the debt-validation workflow (03).
    DebtValidation,
    /// Invoke the client-directed settlement workflow (05).
    Settlement,
    /// No active matter matched, or an unroutable message — flag for a
    /// staff member.
    StaffReview,
}

/// Map a classification to the sub-workflow that handles it. A new
/// contact and a validation response both feed the debt-validation
/// workflow (03): a new contact opens the §1692g window, a verification
/// is the response that workflow waits on.
#[must_use]
pub fn route(class: CollectorMailClass) -> TriageRoute {
    match class {
        CollectorMailClass::LawsuitOrSummons => TriageRoute::ReferLitigation,
        CollectorMailClass::NewContact | CollectorMailClass::ValidationResponse => {
            TriageRoute::DebtValidation
        }
        CollectorMailClass::SettlementOffer => TriageRoute::Settlement,
        CollectorMailClass::Other => TriageRoute::StaffReview,
    }
}

/// Triage an inbound message end to end. An email whose sender does not
/// match an active Nautilus matter is always flagged for staff,
/// whatever its content — we never auto-route mail we can't tie to a
/// represented client.
#[must_use]
pub fn triage(
    has_active_matter: bool,
    subject: &str,
    body: &str,
) -> (CollectorMailClass, TriageRoute) {
    let class = classify(subject, body);
    let route = if has_active_matter {
        route(class)
    } else {
        TriageRoute::StaffReview
    };
    (class, route)
}

/// A statutory deadline the deadline spine tracks as a durable timer and
/// surfaces in the client portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineKind {
    /// FDCPA §1692g(a): the consumer's 30-day window to dispute the debt
    /// or request validation, running from receipt of the validation
    /// notice.
    DebtValidationWindow,
    /// FCRA §1681i(a)(1): the credit bureau's 30-day reinvestigation
    /// period, running from receipt of the dispute.
    FcraReinvestigation,
}

impl DeadlineKind {
    /// The statutory length of the window, in days.
    #[must_use]
    pub const fn days(self) -> i64 {
        match self {
            DeadlineKind::DebtValidationWindow | DeadlineKind::FcraReinvestigation => 30,
        }
    }

    /// The official citation for the window, for the portal and the
    /// journal.
    #[must_use]
    pub const fn statute(self) -> &'static str {
        match self {
            DeadlineKind::DebtValidationWindow => "15 U.S.C. § 1692g(a)",
            DeadlineKind::FcraReinvestigation => "15 U.S.C. § 1681i(a)(1)",
        }
    }
}

/// The date a statutory window closes, given the date it was triggered.
#[must_use]
pub fn deadline_from(kind: DeadlineKind, trigger: NaiveDate) -> NaiveDate {
    trigger + Duration::days(kind.days())
}

/// The outcome of a collector's response to a §1692g validation request,
/// surfaced to the client in plain language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The collector mailed verification of the debt — it proved the
    /// debt is real and the client's.
    Verified,
    /// The collector could not or would not verify — e.g. it is no
    /// longer collecting or has closed the account.
    NotVerified,
    /// The collector verified only part of what it claimed.
    Partial,
}

/// Phrases that mark a failure or refusal to verify.
const NOT_VERIFIED_MARKERS: &[&str] = &[
    "unable to verify",
    "cannot verify",
    "could not verify",
    "no longer collecting",
    "ceased collection",
    "account closed",
    "closing your account",
    "will not pursue",
];

/// Phrases that mark a partial verification.
const PARTIAL_MARKERS: &[&str] = &[
    "partial",
    "a portion of",
    "part of the balance",
    "some of the",
];

/// Phrases that mark a successful verification.
const VERIFIED_MARKERS: &[&str] = &[
    "enclosed is the verification",
    "we have verified",
    "verification of the debt",
    "documentation of the debt",
    "itemization",
    "the debt is valid",
];

/// Classify a collector's verification response. Precedence: an explicit
/// failure-to-verify wins (it is the client-favorable outcome and the
/// one that ends collection), then a partial verification, then a full
/// verification; anything else is treated as not yet verified so the
/// matter stays open for attorney review rather than silently closing.
#[must_use]
pub fn classify_verification(body: &str) -> VerificationOutcome {
    let hay = body.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| hay.contains(n));
    if has(NOT_VERIFIED_MARKERS) {
        VerificationOutcome::NotVerified
    } else if has(PARTIAL_MARKERS) {
        VerificationOutcome::Partial
    } else if has(VERIFIED_MARKERS) {
        VerificationOutcome::Verified
    } else {
        VerificationOutcome::NotVerified
    }
}

/// FDCPA §1692g(b): once the consumer disputes the debt in writing
/// within the 30-day window, the collector must cease collection until
/// it mails verification. A fresh collection attempt while a written
/// dispute is open and no verification has been mailed is a **possible**
/// violation — this predicate only flags it for attorney review; it
/// never decides a claim or triggers litigation.
#[must_use]
pub fn continued_collection_is_possible_violation(
    written_dispute_open: bool,
    verification_mailed: bool,
    new_collection_attempt: bool,
) -> bool {
    written_dispute_open && !verification_mailed && new_collection_attempt
}

/// The result of a credit bureau's FCRA §1681i reinvestigation of a
/// disputed tradeline, surfaced to the client in plain language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcraDisputeResult {
    /// The bureau corrected or deleted the disputed item — the
    /// client-favorable outcome.
    CorrectedOrDeleted,
    /// The bureau verified the item as accurate and left it unchanged.
    VerifiedUnchanged,
}

/// Phrases that mark a corrected or deleted tradeline.
const FCRA_FIXED_MARKERS: &[&str] = &["deleted", "removed", "corrected", "updated", "modified"];

/// Classify a credit bureau's reinvestigation response. A correction or
/// deletion wins; otherwise the item is treated as verified-unchanged,
/// so an ambiguous response is never reported to the client as fixed.
#[must_use]
pub fn classify_fcra_result(body: &str) -> FcraDisputeResult {
    let hay = body.to_lowercase();
    if FCRA_FIXED_MARKERS.iter().any(|n| hay.contains(n)) {
        FcraDisputeResult::CorrectedOrDeleted
    } else {
        FcraDisputeResult::VerifiedUnchanged
    }
}

/// A cease-communication letter under FDCPA §1692c(c) stops the
/// collector from contacting the client, but it does **not** erase the
/// debt. This is a constant the client-facing surfaces use so the
/// promise stays honest — the cease letter changes who may contact the
/// client, not whether the debt is owed.
pub const CEASE_DOES_NOT_ERASE_DEBT: &str =
    "A cease-communication letter stops the collector from contacting you. It does not erase the debt you owe.";

/// The firm's cut of any amount a Nautilus client saves in settlement —
/// always zero, at every amount. Nautilus is a flat $66/month fee; it
/// never takes a percentage of a reduced or settled balance. Encoding
/// the promise as a function (not prose) is what keeps a future change
/// honest: a non-zero return would fail [`tests::the_firm_never_takes_a_cut_of_savings`].
#[must_use]
pub const fn firm_cut_of_savings_cents(_savings_cents: i64) -> i64 {
    0
}

/// A litigation referral. Nautilus halts and hands the matter to
/// litigation counsel rather than answering the correspondence. A
/// lawsuit, a summons, or a viable FDCPA damages claim leaves the
/// correspondence shield the moment it appears — this is the boundary
/// that keeps Nautilus inside the firm's no-litigation identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LitigationReferral {
    /// Why the matter is being referred (e.g. "a summons was served").
    pub reason: String,
    /// The site route to litigation counsel.
    pub counsel_link: &'static str,
    /// Always false: a referred matter is never answered as
    /// correspondence.
    pub answered_as_correspondence: bool,
}

/// Build the litigation referral for a matter that has left the
/// correspondence shield.
#[must_use]
pub fn litigation_referral(reason: impl Into<String>) -> LitigationReferral {
    LitigationReferral {
        reason: reason.into(),
        counsel_link: "/services/litigation",
        answered_as_correspondence: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summons_is_classified_as_litigation_even_with_a_settlement_word() {
        // Precedence: a settlement phrase buried in a summons must not
        // mask the lawsuit — litigation is detected first.
        let class = classify(
            "SUMMONS — Civil Action",
            "You are being sued. You may settle this matter by paying the balance in full.",
        );
        assert_eq!(class, CollectorMailClass::LawsuitOrSummons);
        assert_eq!(route(class), TriageRoute::ReferLitigation);
    }

    #[test]
    fn a_verification_letter_is_a_validation_response() {
        let class = classify(
            "Re: your dispute",
            "Enclosed is the verification of the debt you requested, with an itemization.",
        );
        assert_eq!(class, CollectorMailClass::ValidationResponse);
        assert_eq!(route(class), TriageRoute::DebtValidation);
    }

    #[test]
    fn a_settlement_offer_routes_to_settlement() {
        let class = classify(
            "Settlement offer",
            "We can settle this account for a lump sum of 60% of the balance.",
        );
        assert_eq!(class, CollectorMailClass::SettlementOffer);
        assert_eq!(route(class), TriageRoute::Settlement);
    }

    #[test]
    fn a_first_contact_opens_the_validation_workflow() {
        let class = classify(
            "Outstanding balance",
            "This is an attempt to collect a debt. The amount due is $1,234.",
        );
        assert_eq!(class, CollectorMailClass::NewContact);
        assert_eq!(route(class), TriageRoute::DebtValidation);
    }

    #[test]
    fn an_unrecognized_message_is_flagged_for_staff() {
        let class = classify("Hello", "Please call our office at your convenience.");
        assert_eq!(class, CollectorMailClass::Other);
        assert_eq!(route(class), TriageRoute::StaffReview);
    }

    #[test]
    fn an_unmatched_sender_is_always_flagged_for_staff() {
        // Even a routine collection contact is staff-flagged when we
        // can't tie the sender to a represented client.
        let (class, route) = triage(
            false,
            "Outstanding balance",
            "This is an attempt to collect a debt.",
        );
        assert_eq!(class, CollectorMailClass::NewContact);
        assert_eq!(route, TriageRoute::StaffReview);
    }

    #[test]
    fn verification_outcomes_classify_by_collector_response() {
        assert_eq!(
            classify_verification("Enclosed is the verification of the debt, with an itemization."),
            VerificationOutcome::Verified,
        );
        assert_eq!(
            classify_verification(
                "We are unable to verify this debt and are no longer collecting."
            ),
            VerificationOutcome::NotVerified,
        );
        assert_eq!(
            classify_verification("We can verify a portion of the balance only."),
            VerificationOutcome::Partial,
        );
        // An ambiguous response keeps the matter open (not-verified)
        // rather than silently closing it as verified.
        assert_eq!(
            classify_verification("Thank you for your letter."),
            VerificationOutcome::NotVerified,
        );
    }

    #[test]
    fn continued_collection_during_open_dispute_is_flagged() {
        // §1692g(b): collecting while a written dispute is open and no
        // verification has been mailed is a possible violation.
        assert!(continued_collection_is_possible_violation(
            true, false, true
        ));
        // Not a violation once verification was mailed.
        assert!(!continued_collection_is_possible_violation(
            true, true, true
        ));
        // Not a violation with no fresh collection attempt.
        assert!(!continued_collection_is_possible_violation(
            true, false, false
        ));
        // Not a violation when no dispute is open.
        assert!(!continued_collection_is_possible_violation(
            false, false, true
        ));
    }

    #[test]
    fn fcra_results_classify_by_bureau_response() {
        assert_eq!(
            classify_fcra_result("The disputed item has been deleted from your file."),
            FcraDisputeResult::CorrectedOrDeleted,
        );
        assert_eq!(
            classify_fcra_result("We verified the item as accurate; it remains on your report."),
            FcraDisputeResult::VerifiedUnchanged,
        );
        // Ambiguous → treated as unchanged, never reported as fixed.
        assert_eq!(
            classify_fcra_result("Thank you for your dispute."),
            FcraDisputeResult::VerifiedUnchanged,
        );
    }

    #[test]
    fn cease_letter_disclaimer_is_honest_about_the_debt() {
        assert!(CEASE_DOES_NOT_ERASE_DEBT.contains("does not erase the debt"));
    }

    #[test]
    fn the_firm_never_takes_a_cut_of_savings() {
        // No percentage of any reduced balance, at any amount.
        for savings in [0, 1, 50_000, 5_000_000, i64::MAX] {
            assert_eq!(firm_cut_of_savings_cents(savings), 0);
        }
    }

    #[test]
    fn a_lawsuit_is_referred_out_not_answered() {
        // A summons routes to litigation, and the referral never answers
        // the correspondence — it hands off to litigation counsel.
        let class = classify("Summons", "You are being sued in civil action.");
        assert_eq!(route(class), TriageRoute::ReferLitigation);
        let referral = litigation_referral("a summons was served");
        assert_eq!(referral.counsel_link, "/services/litigation");
        assert!(!referral.answered_as_correspondence);
    }

    #[test]
    fn both_statutory_windows_are_thirty_days_with_citations() {
        let trigger = NaiveDate::from_ymd_opt(2026, 6, 3).unwrap();
        assert_eq!(
            deadline_from(DeadlineKind::DebtValidationWindow, trigger),
            NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(),
        );
        assert_eq!(
            deadline_from(DeadlineKind::FcraReinvestigation, trigger),
            NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(),
        );
        assert_eq!(
            DeadlineKind::DebtValidationWindow.statute(),
            "15 U.S.C. § 1692g(a)"
        );
        assert_eq!(
            DeadlineKind::FcraReinvestigation.statute(),
            "15 U.S.C. § 1681i(a)(1)"
        );
    }
}
