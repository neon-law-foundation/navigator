//! Cucumber runner for `features/nautilus_workflows.feature`.
//!
//! Locks down the shape of each Neon Law Nautilus correspondence
//! notation (notice of representation, debt validation, cease /
//! FCRA dispute, settlement) and proves the unauthorized-practice-of-
//! law gate: no `document_open__*` fill state reaches an outbound
//! submission state without passing the bare `staff_review` gate (the
//! `@approve` attorney-approval step). Complements
//! `workflows/tests/workflow_integrity.rs` (generic invariants) and
//! `spec_coherence.rs` (frontmatter ↔ standalone YAML parity); these
//! scenarios pin the Nautilus-specific transitions and guardrail.

#![allow(clippy::unused_async)]
#![allow(clippy::missing_fields_in_debug)]
#![allow(clippy::struct_excessive_bools)]

use cucumber::{gherkin::Step, given, then, when, World};
use features::template_shapes::{templates_root, walk_chain};
use workflows::{
    classify, classify_fcra_result, classify_verification,
    continued_collection_is_possible_violation, firm_cut_of_savings_cents, litigation_referral,
    questionnaire_spec_from_template, route, staff_review_gates_filing, step_kind_for, triage,
    workflow_spec_from_template, CollectorMailClass, FcraDisputeResult, StateName, TriageRoute,
    VerificationOutcome, WorkflowSpec, CEASE_DOES_NOT_ERASE_DEBT,
};

#[derive(Default, World)]
#[world(init = Self::default)]
struct NautilusWorld {
    markdown: Option<String>,
    inbound_text: Option<String>,
    has_active_matter: bool,
    verification_text: Option<String>,
    written_dispute_open: bool,
    verification_mailed: bool,
    new_collection_attempt: bool,
    savings_cents: i64,
}

impl std::fmt::Debug for NautilusWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NautilusWorld")
            .field("has_markdown", &self.markdown.is_some())
            .field("has_inbound_text", &self.inbound_text.is_some())
            .field("has_active_matter", &self.has_active_matter)
            .finish()
    }
}

fn outcome_name(outcome: VerificationOutcome) -> &'static str {
    match outcome {
        VerificationOutcome::Verified => "Verified",
        VerificationOutcome::NotVerified => "NotVerified",
        VerificationOutcome::Partial => "Partial",
    }
}

fn fcra_name(result: FcraDisputeResult) -> &'static str {
    match result {
        FcraDisputeResult::CorrectedOrDeleted => "CorrectedOrDeleted",
        FcraDisputeResult::VerifiedUnchanged => "VerifiedUnchanged",
    }
}

fn class_name(class: CollectorMailClass) -> &'static str {
    match class {
        CollectorMailClass::LawsuitOrSummons => "LawsuitOrSummons",
        CollectorMailClass::ValidationResponse => "ValidationResponse",
        CollectorMailClass::SettlementOffer => "SettlementOffer",
        CollectorMailClass::NewContact => "NewContact",
        CollectorMailClass::Other => "Other",
    }
}

fn route_name(route: TriageRoute) -> &'static str {
    match route {
        TriageRoute::ReferLitigation => "ReferLitigation",
        TriageRoute::DebtValidation => "DebtValidation",
        TriageRoute::Settlement => "Settlement",
        TriageRoute::StaffReview => "StaffReview",
    }
}

#[given(regex = r#"^the bundled template "([^"]+)"$"#)]
async fn load_template(world: &mut NautilusWorld, relpath: String) {
    let path = templates_root().join(&relpath);
    world.markdown = Some(
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    );
}

#[then("the questionnaire transitions, in BEGIN-first order, are:")]
async fn assert_questionnaire_chain(world: &mut NautilusWorld, step: &Step) {
    let md = world.markdown.as_ref().expect("template loaded");
    let q = questionnaire_spec_from_template(md).expect("questionnaire frontmatter parses");
    assert_chain_matches(q.inner(), step);
}

#[then("every workflow state resolves to a StepKind")]
async fn assert_step_kinds_resolve(world: &mut NautilusWorld) {
    let md = world.markdown.as_ref().expect("template loaded");
    let w = workflow_spec_from_template(md).expect("workflow frontmatter parses");
    for state in w.states.keys() {
        if state.as_str() == StateName::END {
            continue;
        }
        assert!(
            step_kind_for(state).is_some(),
            "state `{}` has no StepKind (prefix `{}` is unrouted)",
            state.as_str(),
            state.prefix(),
        );
    }
}

#[then("the workflow gates every outbound letter behind attorney review")]
async fn assert_review_gate(world: &mut NautilusWorld) {
    let md = world.markdown.as_ref().expect("template loaded");
    let w = workflow_spec_from_template(md).expect("workflow frontmatter parses");
    if let Err(violations) = staff_review_gates_filing(&w) {
        panic!("an outbound letter can be sent without attorney review: {violations:?}");
    }
}

#[given(regex = r#"^an inbound collector email on an active matter saying "([^"]*)"$"#)]
async fn inbound_on_active_matter(world: &mut NautilusWorld, text: String) {
    world.inbound_text = Some(text);
    world.has_active_matter = true;
}

#[given(regex = r#"^an inbound collector email with no matching matter saying "([^"]*)"$"#)]
async fn inbound_unmatched(world: &mut NautilusWorld, text: String) {
    world.inbound_text = Some(text);
    world.has_active_matter = false;
}

#[then(regex = r#"^it is classified as "([^"]+)" and routed to "([^"]+)"$"#)]
async fn assert_class_and_route(world: &mut NautilusWorld, class: String, route_to: String) {
    let text = world.inbound_text.as_ref().expect("inbound text set");
    let actual_class = classify("", text);
    assert_eq!(class_name(actual_class), class, "classification mismatch");
    assert_eq!(route_name(route(actual_class)), route_to, "route mismatch");
}

#[then(regex = r#"^it is routed to "([^"]+)"$"#)]
async fn assert_route_only(world: &mut NautilusWorld, route_to: String) {
    let text = world.inbound_text.as_ref().expect("inbound text set");
    let (_, actual_route) = triage(world.has_active_matter, "", text);
    assert_eq!(route_name(actual_route), route_to, "route mismatch");
}

#[given(regex = r#"^a collector verification response saying "([^"]*)"$"#)]
async fn verification_response(world: &mut NautilusWorld, text: String) {
    world.verification_text = Some(text);
}

#[then(regex = r#"^the verification outcome is "([^"]+)"$"#)]
async fn assert_verification_outcome(world: &mut NautilusWorld, outcome: String) {
    let text = world
        .verification_text
        .as_ref()
        .expect("verification text set");
    assert_eq!(
        outcome_name(classify_verification(text)),
        outcome,
        "verification outcome mismatch"
    );
}

#[given("a written dispute is open and no verification has been mailed")]
async fn dispute_open(world: &mut NautilusWorld) {
    world.written_dispute_open = true;
    world.verification_mailed = false;
}

#[when("the collector makes a fresh collection attempt")]
async fn fresh_collection_attempt(world: &mut NautilusWorld) {
    world.new_collection_attempt = true;
}

#[then("a possible FDCPA violation is flagged for attorney review")]
async fn assert_violation_flagged(world: &mut NautilusWorld) {
    assert!(
        continued_collection_is_possible_violation(
            world.written_dispute_open,
            world.verification_mailed,
            world.new_collection_attempt,
        ),
        "expected a possible §1692g(b) violation to be flagged"
    );
}

#[given(regex = r#"^a credit bureau reinvestigation response saying "([^"]*)"$"#)]
async fn fcra_response(world: &mut NautilusWorld, text: String) {
    world.verification_text = Some(text);
}

#[then(regex = r#"^the FCRA result is "([^"]+)"$"#)]
async fn assert_fcra_result(world: &mut NautilusWorld, result: String) {
    let text = world
        .verification_text
        .as_ref()
        .expect("reinvestigation text set");
    assert_eq!(
        fcra_name(classify_fcra_result(text)),
        result,
        "FCRA result mismatch"
    );
}

#[then("the cease-communication disclaimer says it does not erase the debt")]
async fn assert_cease_disclaimer(_world: &mut NautilusWorld) {
    assert!(CEASE_DOES_NOT_ERASE_DEBT.contains("does not erase the debt"));
}

#[given(regex = r"^the client saves (\d+) cents in settlement$")]
async fn client_saves(world: &mut NautilusWorld, savings: i64) {
    world.savings_cents = savings;
}

#[then("the firm's cut is 0 cents")]
async fn assert_no_cut(world: &mut NautilusWorld) {
    assert_eq!(firm_cut_of_savings_cents(world.savings_cents), 0);
}

#[then(
    regex = r#"^the litigation referral links to "([^"]+)" and is not answered as correspondence$"#
)]
async fn assert_referral(_world: &mut NautilusWorld, link: String) {
    let referral = litigation_referral("a summons was served");
    assert_eq!(referral.counsel_link, link, "referral link mismatch");
    assert!(
        !referral.answered_as_correspondence,
        "a referred lawsuit must never be answered as correspondence"
    );
}

fn assert_chain_matches(spec: &WorkflowSpec, step: &Step) {
    let table = step.table.as_ref().expect("scenario has a data table");
    let expected: Vec<(&str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            (
                row.first().expect("from cell").as_str(),
                row.get(1).expect("to cell").as_str(),
            )
        })
        .collect();
    let chain = walk_chain(spec);
    let actual: Vec<(&str, &str)> = chain
        .iter()
        .map(|(f, t)| (f.as_str(), t.as_str()))
        .collect();
    assert_eq!(actual, expected, "transition chain mismatch");
}

#[tokio::main]
async fn main() {
    NautilusWorld::cucumber()
        .run("tests/features/nautilus_workflows.feature")
        .await;
}
