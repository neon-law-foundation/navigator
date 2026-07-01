//! Cucumber runner for `features/nautilus_debt_shield.feature`.
//!
//! The Nautilus journey: one bold client (Pisces) and one attorney, from
//! an inbound collector contact to a mailed, attorney-reviewed letter and
//! a running FDCPA clock. It stitches the primitives `nautilus_workflows`
//! pins (triage, the letter workflow, the statutory deadline, the no-cut
//! guard) into a single arc driven through the worker runtime — the web
//! walker's signed-template auto-drive doesn't fit Nautilus's
//! `document_open`-first workflow, so the staff-side steps run on the
//! worker, mirroring the `workflows-service` pod.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use cucumber::{given, then, when, World};
use features::journey::{client, matter, Journey};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use store::entity;
use uuid::Uuid;
use workflows::{
    bundled_spec_yaml, deadline_from, firm_cut_of_savings_cents, notation_session, route,
    staff_review_precedes_submission, triage, workflow_spec_from_yaml, CompliancePayload,
    DeadlineKind, DocumentPayload, MachineKind, NextStep, StateMachineRuntime, StateName,
    TriageRoute,
};

const COLLECTOR: &str = "Apex Recovery LLC";

#[derive(Default, World)]
#[world(init = Self::default)]
struct NautilusWorld {
    journey: Option<Journey>,
    person_id: Option<Uuid>,
    project_id: Option<Uuid>,
    notation_id: Option<Uuid>,
    route: Option<TriageRoute>,
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
        "entity__collector" => COLLECTOR,
        "custom_text__alleged_account" => "ACME-44910",
        "entity__original_creditor" => "ACME Card Services",
        "custom_text__disputed_reason" => "I do not recognize this debt and I demand validation",
        other => panic!("debt-validation questionnaire asked an unexpected question: {other}"),
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with an active Nautilus matter$"#)]
async fn seed_client_and_matter(world: &mut NautilusWorld, name: String, email: String) {
    let journey = Journey::open("nautilus").await;
    let person = client(&journey.db, &name, &email).await;
    let project_id = matter(&journey.db, person.id, "Nautilus debt-shield").await;
    world.person_id = Some(person.id);
    world.project_id = Some(project_id);
    world.journey = Some(journey);
}

#[when("a collector makes first contact demanding payment of an alleged debt")]
async fn inbound_contact(world: &mut NautilusWorld) {
    // An active Nautilus matter exists, so triage may auto-route.
    let body = "This is an attempt to collect a debt; the amount due must be paid in full.";
    let (_class, triage_route) = triage(true, "", body);
    world.route = Some(triage_route);
}

#[then("the contact is routed to debt validation")]
async fn assert_route(world: &mut NautilusWorld) {
    assert_eq!(
        world.route,
        Some(TriageRoute::DebtValidation),
        "first contact on an active matter should route to debt validation",
    );
    // Routing is a pure function of the classification; pin it too.
    assert_eq!(
        route(workflows::classify("", "you owe — amount due")),
        TriageRoute::DebtValidation
    );
}

#[when(regex = r#"^the firm walks the "([^"]+)" letter for the client$"#)]
async fn walk_letter(world: &mut NautilusWorld, code: String) {
    let journey = world.journey();
    let outcome = notation_session::start_notation(
        &journey.db,
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
        &journey.db,
        journey.runtime.as_ref(),
        Some(&journey.storage),
        notation_id,
    )
    .await
    .expect("current step")
    {
        notation_session::answer_step(
            &journey.db,
            journey.runtime.as_ref(),
            Some(&journey.storage),
            notation_id,
            &question.code,
            answer_for(&question.code),
            notation_session::AnswerAuthor::staff(None),
        )
        .await
        .expect("answer step");
    }
    world.notation_id = Some(notation_id);
}

#[when("the attorney approves the letter and the mailroom sends it")]
async fn drive_letter_workflow(world: &mut NautilusWorld) {
    let notation_id = world.notation_id();
    let yaml = bundled_spec_yaml("nautilus__debt_validation").expect("bundled spec");
    let spec = workflow_spec_from_yaml(yaml).expect("workflow spec parses");
    let worker = world.journey().worker();
    worker
        .start(MachineKind::Workflow, notation_id, &spec)
        .await
        .expect("start workflow");

    // intake_submitted lands on document_open__debt_validation — render the
    // letter PDF inline (worker side effect).
    let doc = serde_json::to_string(&DocumentPayload::Typst {
        storage_key: format!("notations/{notation_id}/debt-validation.pdf"),
        typst_source: "Debt validation request under 15 U.S.C. § 1692g.".into(),
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
        office: COLLECTOR.into(),
        summary: "Debt validation request (FDCPA §1692g)".into(),
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
    assert_eq!(landed.as_str(), "mailroom_send__debt_validation");
    worker
        .signal(MachineKind::Workflow, notation_id, "mailed", None)
        .await
        .expect("mailed");
}

#[then("the debt-validation letter reaches END")]
async fn assert_letter_end(world: &mut NautilusWorld) {
    let state = StateMachineRuntime::current_state(
        world.journey().runtime.as_ref(),
        MachineKind::Workflow,
        world.notation_id(),
    )
    .await;
    assert_eq!(state, Some(StateName::end()));
}

#[then("the letter was sent to the collector only after attorney review")]
async fn assert_gated_send(world: &mut NautilusWorld) {
    // Structural guarantee: no submission state is reachable without first
    // crossing `staff_review` (the N106 gate the firm relies on).
    let spec = workflow_spec_from_yaml(
        bundled_spec_yaml("nautilus__debt_validation").expect("bundled spec"),
    )
    .expect("spec parses");
    assert!(
        staff_review_precedes_submission(&spec).is_ok(),
        "every Nautilus letter must be gated behind attorney review",
    );
    // And the proof it actually went out: one mailroom `filings` row.
    let rows = entity::filing::Entity::find()
        .filter(entity::filing::Column::NotationId.eq(world.notation_id()))
        .all(&world.journey().db)
        .await
        .expect("query filings");
    assert_eq!(rows.len(), 1, "expected one mailed letter, got {rows:?}");
    assert_eq!(rows[0].kind, "mailroom_send");
    assert_eq!(rows[0].office, COLLECTOR);
}

#[then("the founder's debt-validation answers are on file")]
async fn assert_answers(world: &mut NautilusWorld) {
    let rows = entity::answer::Entity::find()
        .filter(entity::answer::Column::PersonId.eq(world.person_id.expect("person")))
        .all(&world.journey().db)
        .await
        .expect("query answers");
    assert_eq!(rows.len(), 5, "expected five debt-validation answers");
}

#[then(regex = r#"^the debt-validation window closes 30 days after it is triggered on "([^"]+)"$"#)]
async fn assert_deadline(_world: &mut NautilusWorld, trigger: String) {
    let date = chrono::NaiveDate::parse_from_str(&trigger, "%Y-%m-%d").expect("valid trigger date");
    let due = deadline_from(DeadlineKind::DebtValidationWindow, date);
    assert_eq!(due, date + chrono::Duration::days(30));
    assert_eq!(DeadlineKind::DebtValidationWindow.days(), 30);
}

#[then(regex = r#"^the window cites "([^"]+)"$"#)]
async fn assert_statute(_world: &mut NautilusWorld, citation: String) {
    assert_eq!(DeadlineKind::DebtValidationWindow.statute(), citation);
}

#[then(regex = r"^settling a debt and saving (\d+) cents costs the client a 0-cent firm cut$")]
async fn assert_no_cut(_world: &mut NautilusWorld, savings: i64) {
    assert_eq!(
        firm_cut_of_savings_cents(savings),
        0,
        "the flat $66/mo fee never takes a cut of what the client saves",
    );
}

#[tokio::main]
async fn main() {
    NautilusWorld::cucumber()
        .run("tests/features/nautilus_debt_shield.feature")
        .await;
}
