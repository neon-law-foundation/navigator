//! Cucumber runner for `features/acroform_fill.feature`.
//!
//! Drives an AcroForm fill end-to-end through the worker `document_open`
//! dispatch (`DispatchingRuntime` — the same in-process path the dev
//! binary uses): a blank fillable form is staged in storage, a
//! `document_open__nv_articles` transition carries a
//! `DocumentPayload::Acroform`, and the filled PDF's field values are
//! read back to prove they round-trip. A second scenario asserts the
//! attorney-review gate holds for the form's workflow spec.

#![allow(clippy::unused_async)]
#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::fs_storage;
use workflows::{
    staff_review_gates_filing, DispatchingRuntime, DocumentPayload, InMemoryRuntime, MachineKind,
    StateMachineRuntime, WorkflowSpec,
};

const FORM_KEY: &str = "forms/nv_articles.pdf";
const OUTPUT_KEY: &str = "notations/acroform-feature/nv_articles.pdf";

/// The form's workflow spec: fill → staff_review → file. The review
/// state sits between the fill and the filing step.
const SPEC: &str = r"
BEGIN:
  start: document_open__nv_articles
document_open__nv_articles:
  filled: staff_review__articles
staff_review__articles:
  approved: filing__nv_sos
  rejected: END
filing__nv_sos:
  filed: END
END: {}
";

#[derive(Default, World)]
#[world(init = Self::default)]
struct AcroWorld {
    storage: Option<Arc<dyn cloud::StorageService>>,
    runtime: Option<Arc<InMemoryRuntime>>,
    spec: Option<WorkflowSpec>,
}

impl std::fmt::Debug for AcroWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcroWorld").finish_non_exhaustive()
    }
}

impl AcroWorld {
    fn storage(&self) -> Arc<dyn cloud::StorageService> {
        self.storage.clone().expect("storage")
    }
}

#[given(regex = r#"^a blank fillable "([^"]+)" form is stored with fields "([^"]+)", "([^"]+)"$"#)]
async fn stage_blank_form(world: &mut AcroWorld, form: String, f1: String, f2: String) {
    assert_eq!(form, "nv_articles");
    let storage = fs_storage("acroform-fill").await;
    let blank = pdf::blank_acroform(&[f1.as_str(), f2.as_str()]);
    storage
        .put(FORM_KEY, &blank, "application/pdf")
        .await
        .unwrap();
    world.storage = Some(storage);
    world.runtime = Some(Arc::new(InMemoryRuntime::new()));
}

#[when(regex = r#"^the worker fills it for "([^"]+)" with agent "([^"]+)"$"#)]
async fn worker_fills(world: &mut AcroWorld, entity_name: String, agent: String) {
    let storage = world.storage();
    let inner = world.runtime.clone().unwrap();
    let email = Arc::new(workflows::CapturingEmail::new());
    let rt = DispatchingRuntime::new(inner, email, storage.clone());

    let spec = WorkflowSpec::from_yaml(SPEC).unwrap();
    let id = uuid::Uuid::from_u128(0xaced);
    rt.start(MachineKind::Workflow, id, &spec).await.unwrap();

    let mut fields = BTreeMap::new();
    fields.insert("entity_name".to_string(), entity_name);
    fields.insert("registered_agent".to_string(), agent);
    let payload = serde_json::to_string(&DocumentPayload::Acroform {
        storage_key: OUTPUT_KEY.to_string(),
        blank_form_key: FORM_KEY.to_string(),
        fields,
    })
    .unwrap();

    // Lands on document_open__nv_articles → the dispatcher fills + persists.
    let next = rt
        .signal(MachineKind::Workflow, id, "start", Some(&payload))
        .await
        .unwrap();
    assert_eq!(next.as_str(), "document_open__nv_articles");
}

#[then(regex = r#"^the stored form's "([^"]+)" reads "([^"]+)"$"#)]
async fn stored_field_reads(world: &mut AcroWorld, field: String, expected: String) {
    let stored = world.storage().get(OUTPUT_KEY).await.expect("filled form");
    assert_eq!(
        pdf::acroform::read_field_value(&stored.bytes, &field).as_deref(),
        Some(expected.as_str()),
    );
}

#[given("the nv_articles workflow spec")]
async fn load_spec(world: &mut AcroWorld) {
    world.spec = Some(WorkflowSpec::from_yaml(SPEC).unwrap());
}

#[then("the staff_review gate holds between fill and filing")]
async fn gate_holds(world: &mut AcroWorld) {
    let spec = world.spec.as_ref().expect("spec loaded");
    assert!(
        staff_review_gates_filing(spec).is_ok(),
        "the nv_articles spec must route every fill→file path through staff_review"
    );
}

#[tokio::main]
async fn main() {
    AcroWorld::cucumber()
        .run("tests/features/acroform_fill.feature")
        .await;
}
