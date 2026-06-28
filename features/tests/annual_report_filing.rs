//! Cucumber runner for `features/annual_report_filing.feature`.
//!
//! Drives the bundled `nv__annual_report` workflow end-to-end
//! through the in-process `DispatchingRuntime` (the same dispatch path
//! the dev binary uses, here with a database attached so the
//! compliance step can record a `filings` row): BEGIN → staff_review →
//! mailroom_send (records the filing) → END. Proves a compliance flow
//! runs to completion instead of parking, and that the durable filing
//! record lands only after the review gate.

#![allow(clippy::unused_async)]
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::{fs_storage, in_memory_db};
use sea_orm::{ActiveModelTrait, ActiveValue};
use store::{entity, Db};
use uuid::Uuid;
use workflows::{
    staff_review_precedes_submission, CompliancePayload, DispatchingRuntime, InMemoryRuntime,
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
    db: Option<Db>,
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
    fn db(&self) -> &Db {
        self.db.as_ref().expect("db")
    }
    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation")
    }
}

#[given("an annual-report notation for a project")]
async fn seed_notation(world: &mut ReportWorld) {
    let db = in_memory_db().await;
    let tmpl = entity::template::ActiveModel {
        code: ActiveValue::Set(TEMPLATE_CODE.into()),
        title: ActiveValue::Set("NV Annual Report".into()),
        respondent_type: ActiveValue::Set("entity".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let person = entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let __dri = store::test_support::dri_person(&db).await;
    let proj = entity::project::ActiveModel {
        name: ActiveValue::Set("annual report matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&db).await),
        staff_dri_person_id: ActiveValue::Set(Some(__dri)),
        client_dri_person_id: ActiveValue::Set(Some(__dri)),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    let notation_id = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    world.db = Some(db);
    world.notation_id = Some(notation_id);
}

#[when("the annual-report workflow runs through staff_review to mailroom_send and END")]
async fn run_workflow(world: &mut ReportWorld) {
    let db = world.db().clone();
    let id = world.notation_id();
    let rt = DispatchingRuntime::new(
        Arc::new(InMemoryRuntime::new()),
        Arc::new(workflows::CapturingEmail::new()),
        fs_storage("annual-report").await,
    )
    .with_db(db);

    let spec = annual_report_spec();
    rt.start(MachineKind::Workflow, id, &spec).await.unwrap();
    // BEGIN -> staff_review
    rt.signal(MachineKind::Workflow, id, "_", None)
        .await
        .unwrap();
    // staff_review -> mailroom_send: this signal lands on the submission
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
    let filings = store::filings::for_notation(world.db(), world.notation_id())
        .await
        .unwrap();
    assert_eq!(filings.len(), 1, "expected exactly one filing");
    assert_eq!(filings[0].kind, "mailroom_send");
}

#[then(regex = r#"^the recorded filing's office is "([^"]+)"$"#)]
async fn assert_office(world: &mut ReportWorld, office: String) {
    let filings = store::filings::for_notation(world.db(), world.notation_id())
        .await
        .unwrap();
    assert_eq!(filings[0].office, office);
}

#[then("no submission in the annual-report spec is reachable without staff_review")]
async fn assert_gate(_world: &mut ReportWorld) {
    assert!(staff_review_precedes_submission(&annual_report_spec()).is_ok());
}

#[tokio::main]
async fn main() {
    ReportWorld::cucumber()
        .run("tests/features/annual_report_filing.feature")
        .await;
}
