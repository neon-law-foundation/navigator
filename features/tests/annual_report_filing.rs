//! Cucumber runner for `features/annual_report_filing.feature`.
//!
//! Drives the bundled `nv__annual_report` workflow end-to-end
//! through the in-process `DispatchingRuntime` (the same dispatch path
//! the dev binary uses, here with a database attached so the
//! compliance step can record a `filings` row): BEGIN → lawyer_review →
//! mailroom_send (records the filing) → END. Proves a compliance flow
//! runs to completion instead of parking, and that the durable filing
//! record lands only after the review gate.

#![allow(clippy::unused_async)]
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::fs_storage;
use uuid::Uuid;
use workflows::{
    lawyer_review_precedes_submission, CompliancePayload, DispatchingRuntime, InMemoryRuntime,
    MachineKind, StateMachineRuntime, WorkflowSpec,
};

const TEMPLATE_CODE: &str = "nv__annual_report";

fn annual_report_spec() -> WorkflowSpec {
    workflows::workflow_spec_from_yaml(workflows::bundled_spec_yaml(TEMPLATE_CODE).unwrap())
        .expect("nv__annual_report workflow block parses")
}

#[derive(Default, World)]
#[world(init = Self::default)]
struct ReportWorld {
    notation_id: Option<Uuid>,
    final_state: Option<String>,
}

impl std::fmt::Debug for ReportWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReportWorld")
            .field("notation_id", &self.notation_id)
            .field("final_state", &self.final_state)
            .finish_non_exhaustive()
    }
}

impl ReportWorld {
    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation")
    }
}

#[given("an annual-report notation for a project")]
async fn seed_notation(world: &mut ReportWorld) {
    let surreal = features::shared_surreal().await;
    let tmpl = store::templates::save_version(
        &surreal,
        None,
        TEMPLATE_CODE,
        store::templates::Version {
            title: "NV Annual Report".into(),
            respondent_type: "entity".into(),
            asset_id: None,
            form_code: None,
            kind: None,
            source_commit_sha: None,
        },
    )
    .await
    .unwrap()
    .into_model();
    let person = store::test_support::ensure_person(
        &surreal,
        &store::persons::NewPerson::new("Libra", "libra@example.com"),
    )
    .await;
    let proj = store::test_support::seed_project(&surreal, "annual report matter").await;
    let notation_id = store::notations::create(
        &surreal,
        &store::notations::NewNotation::new(tmpl.id, person.id, proj.id, "BEGIN"),
    )
    .await
    .unwrap()
    .id;
    world.notation_id = Some(notation_id);
}

#[when("the annual-report workflow runs through lawyer_review to mailroom_send and END")]
async fn run_workflow(world: &mut ReportWorld) {
    let surreal = features::shared_surreal().await;
    let id = world.notation_id();
    let rt = DispatchingRuntime::new(
        Arc::new(InMemoryRuntime::new()),
        Arc::new(workflows::CapturingEmail::new()),
        fs_storage("annual-report").await,
    )
    .with_store(surreal.clone());

    let spec = annual_report_spec();
    rt.start(MachineKind::Workflow, id, &spec).await.unwrap();
    // BEGIN -> lawyer_review
    rt.signal(MachineKind::Workflow, id, "_", None)
        .await
        .unwrap();
    // lawyer_review -> mailroom_send: this signal lands on the submission
    // step, so it carries the CompliancePayload the worker records.
    let payload = serde_json::to_string(&CompliancePayload {
        office: "Nevada Secretary of State".into(),
        summary: "Nevada annual report mailed".into(),
        reference: None,
    })
    .unwrap();
    let at_send = rt
        .signal(MachineKind::Workflow, id, "_", Some(&payload))
        .await
        .unwrap();
    assert_eq!(at_send.as_str(), "mailroom_send");
    // mailroom_send -> END
    let end = rt
        .signal(MachineKind::Workflow, id, "_", None)
        .await
        .unwrap();
    world.final_state = Some(end.as_str().to_string());
}

#[then(regex = r#"^the workflow reached "([^"]+)"$"#)]
async fn assert_reached(world: &mut ReportWorld, state: String) {
    assert_eq!(world.final_state.as_deref(), Some(state.as_str()));
}

#[then("one filing was recorded for the notation")]
async fn assert_one_filing(world: &mut ReportWorld) {
    let surreal = features::shared_surreal().await;
    let filings = store::filings::for_notation(&surreal, world.notation_id())
        .await
        .unwrap();
    assert_eq!(filings.len(), 1, "expected exactly one filing");
    assert_eq!(filings[0].kind, "mailroom_send");
}

#[then(regex = r#"^the recorded filing's office is "([^"]+)"$"#)]
async fn assert_office(world: &mut ReportWorld, office: String) {
    let surreal = features::shared_surreal().await;
    let filings = store::filings::for_notation(&surreal, world.notation_id())
        .await
        .unwrap();
    assert_eq!(filings[0].office, office);
}

#[then("no submission in the annual-report spec is reachable without lawyer_review")]
async fn assert_gate(_world: &mut ReportWorld) {
    assert!(lawyer_review_precedes_submission(&annual_report_spec()).is_ok());
}

#[tokio::main]
async fn main() {
    ReportWorld::cucumber()
        .run_and_exit("tests/features/annual_report_filing.feature")
        .await;
}
