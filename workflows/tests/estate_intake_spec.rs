//! Integration test for the Northstar estate-plan workflow.
//!
//! The spec lives in the frontmatter of `templates/neon_law/northstar/estate_plan.md`
//! so the template and this test fail together if either drifts. The
//! first test asserts the parsed state-machine shape; the second drives
//! a notation through the happy path on the in-memory runtime to confirm
//! it reaches END; the third confirms the attorney can reject at
//! `staff_review` and short-circuit to END before any client review.

use uuid::Uuid;
use workflows::{
    workflow_spec_from_template, InMemoryRuntime, MachineKind, StateMachineRuntime, StateName,
    WorkflowSpec,
};

const TEMPLATE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../templates/neon_law/northstar/estate_plan.md",
);

const KIND: MachineKind = MachineKind::Workflow;
const ID1: Uuid = Uuid::from_u128(101);
const ID2: Uuid = Uuid::from_u128(102);

fn load_spec() -> WorkflowSpec {
    let markdown = std::fs::read_to_string(TEMPLATE_PATH)
        .unwrap_or_else(|e| panic!("read {TEMPLATE_PATH}: {e}"));
    workflow_spec_from_template(&markdown).expect("Estate.md workflow frontmatter must parse")
}

#[test]
fn estate_spec_has_expected_state_machine() {
    let spec = load_spec();

    let begin = spec
        .transitions_from(&StateName::begin())
        .expect("BEGIN state");
    assert_eq!(
        begin.lookup("transcript_uploaded").map(StateName::as_str),
        Some("document_intake__transcript"),
    );

    // The uploaded transcript is filed into the matter by the reusable
    // document-intake step (no live STT — AIDA/Gemini transcribes offline,
    // then the text is uploaded), and the intake advances to extraction.
    let intake = spec
        .transitions_from(&StateName::from("document_intake__transcript"))
        .expect("document_intake__transcript state");
    assert_eq!(
        intake.lookup("transcript_ready").map(StateName::as_str),
        Some("extract__inputs"),
    );

    let extract = spec
        .transitions_from(&StateName::from("extract__inputs"))
        .expect("extract__inputs state");
    assert_eq!(
        extract.lookup("inputs_ready").map(StateName::as_str),
        Some("document_drafts__estate"),
    );

    let drafts = spec
        .transitions_from(&StateName::from("document_drafts__estate"))
        .expect("document_drafts__estate state");
    assert_eq!(
        drafts.lookup("drafts_persisted").map(StateName::as_str),
        Some("staff_review"),
    );

    // The attorney gate fans out: approve advances to client review,
    // reject ends the matter. The human-in-the-loop gate before any
    // client ever sees a draft.
    let reviewed = spec
        .transitions_from(&StateName::from("staff_review"))
        .expect("staff_review state");
    assert_eq!(
        reviewed.lookup("approved").map(StateName::as_str),
        Some("client_review"),
    );
    assert_eq!(reviewed.lookup("rejected"), Some(&StateName::end()));

    // The new reusable primitive: the client's comment-only approval
    // drives the matter to signing.
    let client = spec
        .transitions_from(&StateName::from("client_review"))
        .expect("client_review state");
    assert_eq!(
        client.lookup("client_approved").map(StateName::as_str),
        Some("sent_for_signature__pending"),
    );

    let sent = spec
        .transitions_from(&StateName::from("sent_for_signature__pending"))
        .expect("sent_for_signature__pending state");
    assert_eq!(sent.lookup("signature_received"), Some(&StateName::end()));
    assert_eq!(sent.lookup("signature_declined"), Some(&StateName::end()));

    assert!(spec.is_terminal(&StateName::end()));
}

#[tokio::test]
async fn estate_workflow_runs_recording_to_signature_on_in_memory_runtime() {
    let spec = load_spec();
    let rt = InMemoryRuntime::new();

    StateMachineRuntime::start(&rt, KIND, ID1, &spec)
        .await
        .unwrap();

    for (condition, expected) in [
        ("transcript_uploaded", "document_intake__transcript"),
        ("transcript_ready", "extract__inputs"),
        ("inputs_ready", "document_drafts__estate"),
        ("drafts_persisted", "staff_review"),
        ("approved", "client_review"),
        ("client_approved", "sent_for_signature__pending"),
    ] {
        let s = StateMachineRuntime::signal(&rt, KIND, ID1, condition, None)
            .await
            .unwrap();
        assert_eq!(s.as_str(), expected, "after {condition}");
    }

    let s = StateMachineRuntime::signal(&rt, KIND, ID1, "signature_received", None)
        .await
        .unwrap();
    assert_eq!(s, StateName::end());

    let events = StateMachineRuntime::events(&rt, KIND, ID1).await;
    assert_eq!(events.len(), 7);
    assert_eq!(events[0].condition, "transcript_uploaded");
    assert_eq!(events.last().unwrap().condition, "signature_received");
}

#[tokio::test]
async fn attorney_rejection_ends_the_matter_before_client_review() {
    let spec = load_spec();
    let rt = InMemoryRuntime::new();
    StateMachineRuntime::start(&rt, KIND, ID2, &spec)
        .await
        .unwrap();
    for condition in [
        "transcript_uploaded",
        "transcript_ready",
        "inputs_ready",
        "drafts_persisted",
    ] {
        StateMachineRuntime::signal(&rt, KIND, ID2, condition, None)
            .await
            .unwrap();
    }
    let s = StateMachineRuntime::signal(&rt, KIND, ID2, "rejected", None)
        .await
        .unwrap();
    assert_eq!(s, StateName::end());
    let events = StateMachineRuntime::events(&rt, KIND, ID2).await;
    assert_eq!(events.last().unwrap().condition, "rejected");
}
