//! Cucumber runner for `features/nautilus_screening_shield.feature`.
//!
//! The Nautilus journey: one bold client (Pisces) and one attorney, from
//! an inbound adverse-action notice to a mailed, attorney-reviewed FCRA
//! dispute letter and a running §1681i reinvestigation clock. It stitches
//! the primitives `nautilus_workflows` pins (triage, the dispute-letter
//! workflow, the statutory deadline, the litigation boundary) into a
//! single arc driven through the worker runtime — the web walker's
//! signed-template auto-drive doesn't fit Nautilus's `generate_pdf`-first
//! workflow, so the lawyer-side steps run on the worker, mirroring the
//! `workflows-service` pod.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use cucumber::{given, then, when, World};
use features::journey::{client, matter, Journey};
use store::statutory_deadlines::{self, NewStatutoryDeadline};
use uuid::Uuid;
use workflows::{
    bundled_spec_yaml, deadline_from, lawyer_review_precedes_submission, notation_session, route,
    triage, workflow_spec_from_yaml, CompliancePayload, DeadlineKind, DocumentPayload, MachineKind,
    NextStep, StateMachineRuntime, StateName, StatutoryDeadline, TriageRoute,
};

const AGENCY: &str = "Acme Tenant Screening";
const TRIAGE_SOURCE: &str = "nautilus_inbound_triage";

#[derive(Default, World)]
#[world(init = Self::default)]
struct NautilusWorld {
    journey: Option<Journey>,
    person_id: Option<Uuid>,
    project_id: Option<Uuid>,
    notation_id: Option<Uuid>,
    route: Option<TriageRoute>,
    deadlines: Vec<StatutoryDeadline>,
}

impl std::fmt::Debug for NautilusWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NautilusWorld")
            .field("notation_id", &self.notation_id)
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

impl NautilusWorld {
    fn journey(&self) -> &Journey {
        self.journey.as_ref().expect("journey not built")
    }

    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation_id not captured")
    }
}

fn answer_for(code: &str) -> &'static str {
    match code {
        "person__client" => "Pisces",
        "custom_text__reporting_agency" => AGENCY,
        "custom_text__disputed_item" => "Eviction record, case #UD-2021-4432",
        "custom_text__report_error" => {
            "This eviction is not mine — my file was mixed with another consumer's."
        }
        other => panic!("fcra-dispute questionnaire asked an unexpected question: {other}"),
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with an active Nautilus matter$"#)]
async fn seed_client_and_matter(world: &mut NautilusWorld, name: String, email: String) {
    let journey = Journey::open("nautilus").await;
    let person = client(&journey.surreal, &name, &email).await;
    let project_id = matter(&journey.surreal, person.id, "Nautilus screening-shield").await;
    world.person_id = Some(person.id);
    world.project_id = Some(project_id);
    world.journey = Some(journey);
}

#[when("a landlord sends an adverse-action notice denying the application on a consumer report")]
async fn inbound_contact(world: &mut NautilusWorld) {
    // An active Nautilus matter exists, so triage may auto-route.
    let body = "We denied your application based on information in your consumer report.";
    let decision = triage(
        true,
        "Your rental application",
        body,
        chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
    );
    world.route = Some(decision.route);
    let project_id = world.project_id.expect("project");
    let deadline_rows: Vec<NewStatutoryDeadline<'_>> = decision
        .deadlines
        .iter()
        .map(|deadline| NewStatutoryDeadline {
            project_id,
            kind: deadline.storage_kind(),
            trigger_on: deadline.trigger_on,
            due_on: deadline.due_on,
            statute: deadline.statute,
            source: TRIAGE_SOURCE,
        })
        .collect();
    statutory_deadlines::record_all(&world.journey().surreal, &deadline_rows)
        .await
        .expect("record statutory deadlines");
    world.deadlines = decision.deadlines;
}

#[then("the notice is routed to open a consumer-report dispute")]
async fn assert_route(world: &mut NautilusWorld) {
    assert_eq!(
        world.route,
        Some(TriageRoute::OpenDispute),
        "an adverse-action notice on an active matter should open the dispute workflow",
    );
    // Routing is a pure function of the classification; pin it too.
    assert_eq!(
        route(workflows::classify(
            "",
            "We denied your application based on your consumer report."
        )),
        TriageRoute::OpenDispute
    );
    assert_eq!(
        world.deadlines,
        vec![
            StatutoryDeadline::new(
                DeadlineKind::FcraReinvestigation,
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            ),
            StatutoryDeadline::new(
                DeadlineKind::AdverseActionFreeReport,
                chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            ),
        ],
        "adverse-action routing must calendar both FCRA windows",
    );
    let rows = statutory_deadlines::by_project(
        &world.journey().surreal,
        world.project_id.expect("project"),
    )
    .await
    .expect("query statutory deadlines");
    assert_eq!(rows.len(), 2, "triage must persist both deadlines");
    assert!(
        rows.iter().any(|row| row.kind == "fcra_reinvestigation"
            && row.due_on == chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
            && row.statute == "15 U.S.C. § 1681i(a)(1)"
            && row.source == TRIAGE_SOURCE),
        "missing durable reinvestigation deadline: {rows:?}",
    );
    assert!(
        rows.iter()
            .any(|row| row.kind == "adverse_action_free_report"
                && row.due_on == chrono::NaiveDate::from_ymd_opt(2026, 7, 31).unwrap()
                && row.statute == "15 U.S.C. § 1681j(b)"
                && row.source == TRIAGE_SOURCE),
        "missing durable free-report deadline: {rows:?}",
    );
}

#[when(regex = r#"^the firm walks the "([^"]+)" letter for the client$"#)]
async fn walk_letter(world: &mut NautilusWorld, code: String) {
    let journey = world.journey();
    let outcome = notation_session::start_notation(
        &journey.surreal,
        journey.runtime.as_ref(),
        Some(&journey.storage),
        &code,
        world.person_id.expect("person"),
        world.project_id.expect("project"),
        None,
    )
    .await
    .expect("start nautilus notation");
    let notation_id = outcome.notation_id;
    // Walk the questionnaire one answer per question, in BEGIN order,
    // until it reports complete.
    while let NextStep::NeedsAnswer { question } = notation_session::current_step(
        &journey.surreal,
        journey.runtime.as_ref(),
        Some(&journey.storage),
        notation_id,
    )
    .await
    .expect("current step")
    {
        notation_session::answer_step(
            &journey.surreal,
            journey.runtime.as_ref(),
            Some(&journey.storage),
            notation_id,
            &question.code,
            answer_for(&question.code),
            notation_session::AnswerAuthor::lawyer(None),
        )
        .await
        .expect("answer step");
    }
    world.notation_id = Some(notation_id);
}

#[when("the attorney approves the letter and the mailroom sends it")]
async fn drive_letter_workflow(world: &mut NautilusWorld) {
    let notation_id = world.notation_id();
    let yaml = bundled_spec_yaml("nautilus__fcra_dispute").expect("bundled spec");
    let spec = workflow_spec_from_yaml(yaml).expect("workflow spec parses");
    let worker = world.journey().worker();
    worker
        .start(MachineKind::Workflow, notation_id, &spec)
        .await
        .expect("start workflow");

    // intake_submitted lands on generate_pdf__fcra_dispute — render the
    // letter PDF inline (worker side effect).
    let doc = serde_json::to_string(&DocumentPayload::Typst {
        storage_key: format!("notations/{notation_id}/fcra-dispute.pdf"),
        typst_source: "FCRA §1681i dispute of a consumer-report item.".into(),
    })
    .expect("serialize document payload");
    worker
        .signal(
            MachineKind::Workflow,
            notation_id,
            "intake_submitted",
            Some(&doc),
        )
        .await
        .expect("intake_submitted");
    worker
        .signal(MachineKind::Workflow, notation_id, "pdf_persisted", None)
        .await
        .expect("pdf_persisted");

    // The attorney approves; `approved` lands on mailroom_send, recording
    // the outbound `filings` row (the proof the letter was mailed).
    let compliance = serde_json::to_string(&CompliancePayload {
        office: AGENCY.into(),
        summary: "FCRA §1681i consumer-report dispute".into(),
        reference: None,
    })
    .expect("serialize compliance payload");
    let landed = worker
        .signal(
            MachineKind::Workflow,
            notation_id,
            "approved",
            Some(&compliance),
        )
        .await
        .expect("approved");
    assert_eq!(landed.as_str(), "mailroom_send__fcra_dispute");
    worker
        .signal(MachineKind::Workflow, notation_id, "mailed", None)
        .await
        .expect("mailed");
}

#[then("the fcra-dispute letter reaches END")]
async fn assert_letter_end(world: &mut NautilusWorld) {
    let state = StateMachineRuntime::current_state(
        world.journey().runtime.as_ref(),
        MachineKind::Workflow,
        world.notation_id(),
    )
    .await;
    assert_eq!(state, Some(StateName::end()));
}

#[then("the letter was sent to the reporting agency only after attorney review")]
async fn assert_gated_send(world: &mut NautilusWorld) {
    // Structural guarantee: no submission state is reachable without first
    // crossing `lawyer_review` (the N106 gate the firm relies on).
    let spec =
        workflow_spec_from_yaml(bundled_spec_yaml("nautilus__fcra_dispute").expect("bundled spec"))
            .expect("spec parses");
    assert!(
        lawyer_review_precedes_submission(&spec).is_ok(),
        "every Nautilus letter must be gated behind attorney review",
    );
    // And the proof it actually went out: one mailroom `filings` row.
    let rows = store::filings::for_notation(&world.journey().surreal, world.notation_id())
        .await
        .expect("query filings");
    assert_eq!(rows.len(), 1, "expected one mailed letter, got {rows:?}");
    assert_eq!(rows[0].kind, "mailroom_send");
    assert_eq!(rows[0].office, AGENCY);
}

#[then("the client's consumer-report dispute answers are on file")]
async fn assert_answers(world: &mut NautilusWorld) {
    let rows: Vec<_> = store::answers::list_all(&world.journey().surreal)
        .await
        .expect("query answers")
        .into_iter()
        .filter(|a| a.person_id == world.person_id.expect("person"))
        .collect();
    assert_eq!(
        rows.len(),
        4,
        "expected four consumer-report dispute answers"
    );
}

#[then(regex = r#"^the reinvestigation window closes 30 days after it is triggered on "([^"]+)"$"#)]
async fn assert_deadline(_world: &mut NautilusWorld, trigger: String) {
    let date = chrono::NaiveDate::parse_from_str(&trigger, "%Y-%m-%d").expect("valid trigger date");
    let due = deadline_from(DeadlineKind::FcraReinvestigation, date);
    assert_eq!(due, date + chrono::Duration::days(30));
    assert_eq!(DeadlineKind::FcraReinvestigation.days(), 30);
}

#[then(regex = r#"^the window cites "([^"]+)"$"#)]
async fn assert_statute(_world: &mut NautilusWorld, citation: String) {
    assert_eq!(DeadlineKind::FcraReinvestigation.statute(), citation);
}

#[then(
    regex = r#"^the free-report window closes 60 days after it is triggered on "([^"]+)" citing "([^"]+)"$"#
)]
async fn assert_free_report_window(_world: &mut NautilusWorld, trigger: String, citation: String) {
    let date = chrono::NaiveDate::parse_from_str(&trigger, "%Y-%m-%d").expect("valid trigger date");
    let due = deadline_from(DeadlineKind::AdverseActionFreeReport, date);
    assert_eq!(due, date + chrono::Duration::days(60));
    assert_eq!(DeadlineKind::AdverseActionFreeReport.days(), 60);
    assert_eq!(DeadlineKind::AdverseActionFreeReport.statute(), citation);
}

#[tokio::main]
async fn main() {
    NautilusWorld::cucumber()
        .run_and_exit("tests/features/nautilus_screening_shield.feature")
        .await;
}
