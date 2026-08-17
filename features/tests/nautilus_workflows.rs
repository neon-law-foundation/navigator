//! Cucumber runner for `features/nautilus_workflows.feature`.
//!
//! Locks down the shape of the Neon Law Nautilus consumer-report dispute
//! notation (`nautilus__fcra_dispute`) and proves the unauthorized-
//! practice-of-law gate: no `generate_pdf__*` fill state reaches an
//! outbound submission state without passing the bare `lawyer_review` gate
//! (the `@approve` attorney-approval step). Complements
//! `workflows/tests/workflow_integrity.rs` (generic invariants) and
//! `spec_coherence.rs` (frontmatter ↔ standalone YAML parity); these
//! scenarios pin the Nautilus-specific transitions, the inbound-triage
//! classification, and the litigation boundary.

#![allow(clippy::unused_async)]
#![allow(clippy::missing_fields_in_debug)]

use cucumber::{gherkin::Step, given, then, World};
use features::template_shapes::{templates_root, walk_chain};
use workflows::{
    classify, classify_fcra_result, lawyer_review_gates_filing, litigation_referral,
    questionnaire_spec_from_template, route, step_kind_for, triage, FcraDisputeResult,
    ScreeningMailClass, StateName, TriageRoute, WorkflowSpec,
};

#[derive(Default, World)]
#[world(init = Self::default)]
struct NautilusWorld {
    markdown: Option<String>,
    inbound_text: Option<String>,
    has_active_matter: bool,
    reinvestigation_text: Option<String>,
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

fn fcra_name(result: FcraDisputeResult) -> &'static str {
    match result {
        FcraDisputeResult::CorrectedOrDeleted => "CorrectedOrDeleted",
        FcraDisputeResult::VerifiedUnchanged => "VerifiedUnchanged",
    }
}

fn class_name(class: ScreeningMailClass) -> &'static str {
    match class {
        ScreeningMailClass::LawsuitOrSummons => "LawsuitOrSummons",
        ScreeningMailClass::ReinvestigationResult => "ReinvestigationResult",
        ScreeningMailClass::AdverseAction => "AdverseAction",
        ScreeningMailClass::ReportForwarded => "ReportForwarded",
        ScreeningMailClass::Other => "Other",
    }
}

fn route_name(route: TriageRoute) -> &'static str {
    match route {
        TriageRoute::ReferLitigation => "ReferLitigation",
        TriageRoute::OpenDispute => "OpenDispute",
        TriageRoute::ReinvestigationReview => "ReinvestigationReview",
        TriageRoute::LawyerReview => "LawyerReview",
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
    let w = workflows::workflow_spec_from_template(md).expect("workflow frontmatter parses");
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
    let w = workflows::workflow_spec_from_template(md).expect("workflow frontmatter parses");
    if let Err(violations) = lawyer_review_gates_filing(&w) {
        panic!("an outbound letter can be sent without attorney review: {violations:?}");
    }
}

#[given(regex = r#"^an inbound screening email on an active matter saying "([^"]*)"$"#)]
async fn inbound_on_active_matter(world: &mut NautilusWorld, text: String) {
    world.inbound_text = Some(text);
    world.has_active_matter = true;
}

#[given(regex = r#"^an inbound screening email with no matching matter saying "([^"]*)"$"#)]
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
    let decision = triage(
        world.has_active_matter,
        "",
        text,
        chrono::NaiveDate::from_ymd_opt(2026, 6, 3).unwrap(),
    );
    assert_eq!(route_name(decision.route), route_to, "route mismatch");
}

#[given(regex = r#"^a consumer reporting agency reinvestigation response saying "([^"]*)"$"#)]
async fn fcra_response(world: &mut NautilusWorld, text: String) {
    world.reinvestigation_text = Some(text);
}

#[then(regex = r#"^the FCRA result is "([^"]+)"$"#)]
async fn assert_fcra_result(world: &mut NautilusWorld, result: String) {
    let text = world
        .reinvestigation_text
        .as_ref()
        .expect("reinvestigation text set");
    assert_eq!(
        fcra_name(classify_fcra_result(text)),
        result,
        "FCRA result mismatch"
    );
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
        .run_and_exit("tests/features/nautilus_workflows.feature")
        .await;
}
