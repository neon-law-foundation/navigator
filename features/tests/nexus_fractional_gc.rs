//! Cucumber runner for `features/nexus_fractional_gc.feature`.
//!
//! The Nexus journey models an *ongoing* relationship rather than a
//! one-shot matter, so it stitches three surfaces around the signed
//! engagement: the admin walker (sign the engagement letter — a stub
//! `onboarding__nexus` template), the `repos` engine (deliver work product
//! into the Project repo, visible in the listing), and the
//! `portal::email_threads` engine (route the founder's question to lawyer).

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::journey::{answer_body, client, Journey};
use portal::email::CapturingEmail;
use portal::email_threads::{thread_inbound, ThreadConfig};
use portal::inbound_email::InboundEmail;
use uuid::Uuid;
use workflows::{MachineKind, StateMachineRuntime, StateName};

const PARSE_HOST: &str = "parse.nexus.test";
const FOUNDER_EMAIL: &str = "sagittarius@example.com";
const RESOLUTION_PATH: &str = "resolutions/2026-07-board.md";

#[derive(Default, World)]
#[world(init = Self::default)]
struct NexusWorld {
    journey: Option<Journey>,
    email: Option<Arc<CapturingEmail>>,
    notation_id: Option<Uuid>,
    project_id: Option<Uuid>,
    project_code: Option<String>,
}

impl std::fmt::Debug for NexusWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NexusWorld")
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl NexusWorld {
    fn journey(&self) -> &Journey {
        self.journey.as_ref().expect("journey not built")
    }

    fn notation_id(&self) -> Uuid {
        self.notation_id.expect("notation not opened")
    }

    fn project_code(&self) -> &str {
        self.project_code.as_deref().expect("project not resolved")
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with a fractional-GC engagement$"#)]
async fn seed_client(world: &mut NexusWorld, name: String, email: String) {
    std::env::set_var(
        repos::REPO_ROOT_ENV,
        std::env::temp_dir().join("navigator-features-nexus-git"),
    );
    std::env::set_var("NAVIGATOR_PARSE_HOST", PARSE_HOST);
    std::env::set_var("NAVIGATOR_LAWYER_NOTIFY_EMAIL", "lawyer@neonlaw.com");

    let journey = Journey::open("nexus").await;
    client(&journey.surreal, &name, &email).await;
    world.email = Some(Arc::new(CapturingEmail::new()));
    world.journey = Some(journey);
}

#[given(regex = r#"^a lawyer "([^"]+)"$"#)]
async fn seed_lawyer(world: &mut NexusWorld, email: String) {
    store::test_support::ensure_person(
        &world.journey().surreal,
        &store::persons::NewPerson::with_role(
            "Neon Law Lawyer",
            email,
            store::persons::Role::Lawyer,
        ),
    )
    .await;
}

#[when("the firm opens the Nexus engagement for the founder")]
async fn open_engagement(world: &mut NexusWorld) {
    let body = format!(
        "client_email={}&retainer_template_code=onboarding__nexus",
        features::form_encode(FOUNDER_EMAIL),
    );
    let resp = world
        .journey()
        .lawyer_post("/lawyer/retainers/new", body)
        .await;
    let location = resp
        .location
        .unwrap_or_else(|| panic!("opening the engagement did not redirect ({})", resp.status));
    let id = location
        .strip_prefix("/lawyer/notations/")
        .and_then(|s| s.strip_suffix("/step"))
        .unwrap_or_else(|| panic!("unexpected redirect: {location}"));
    let notation_id = Uuid::parse_str(id).expect("notation id");

    // Walk the two onboarding questions; completing the intake parks the
    // notation at the `lawyer_review` human gate.
    let path = format!("/lawyer/notations/{notation_id}/step");
    for value in ["Sagittarius", "Horizon Robotics LLC"] {
        let resp = world.journey().lawyer_post(&path, answer_body(value)).await;
        assert!(
            resp.status.is_success() || resp.status.is_redirection(),
            "answering {value:?} returned {}",
            resp.status,
        );
    }

    // Lawyer approves (renders + persists the engagement letter at
    // `generate_pdf__*`) and sends it — driving the workflow to the
    // signature wait so the founder can sign next.
    for action in ["approve-send", "send"] {
        let resp = world
            .journey()
            .lawyer_post(
                &format!("/lawyer/notations/{notation_id}/{action}"),
                String::new(),
            )
            .await;
        assert!(
            resp.status.is_success() || resp.status.is_redirection(),
            "{action} returned {}",
            resp.status,
        );
    }

    // Resolve the project the walker created for the repo + later steps.
    let notation = store::notations::find_by_id(&world.journey().surreal, notation_id)
        .await
        .unwrap()
        .expect("notation exists");
    let project = store::projects::find_by_id(&world.journey().surreal, notation.project_id)
        .await
        .unwrap()
        .expect("project exists");
    world.project_id = Some(notation.project_id);
    world.project_code = Some(project.code);
    world.notation_id = Some(notation_id);
}

#[when("the founder signs the engagement letter")]
async fn sign(world: &mut NexusWorld) {
    let worker = world.journey().worker();
    worker
        .signal(
            MachineKind::Workflow,
            world.notation_id(),
            "signature_received",
            None,
        )
        .await
        .expect("signature_received");
}

#[then("the engagement is active")]
async fn assert_active(world: &mut NexusWorld) {
    let state = StateMachineRuntime::current_state(
        world.journey().runtime.as_ref(),
        MachineKind::Workflow,
        world.notation_id(),
    )
    .await;
    assert_eq!(
        state,
        Some(StateName::end()),
        "the engagement letter should be fully signed",
    );
}

#[when("the firm delivers a board resolution through the Project repo")]
async fn deliver_doc(world: &mut NexusWorld) {
    let store = repos::RepoStore::from_env().expect("repo root set");
    let project_code = world.project_code();
    store.ensure_code(project_code).expect("ensure repo");
    store
        .commit_as_code(
            project_code,
            repos::Author {
                name: "Neon Law",
                email: "support@neonlaw.com",
            },
            "Deliver July board resolution",
            &[(
                RESOLUTION_PATH,
                b"# Board resolution\n\nApproved by written consent.\n",
            )],
        )
        .expect("commit resolution");
}

#[then("the resolution appears in the Project repo listing")]
async fn assert_listed(world: &mut NexusWorld) {
    let store = repos::RepoStore::from_env().expect("repo root set");
    let listed = store
        .read_head_tree_code(world.project_code())
        .expect("read head tree")
        .iter()
        .any(|(p, _)| p == RESOLUTION_PATH);
    assert!(
        listed,
        "the delivered resolution should be in the repo listing"
    );
}

#[when("the founder emails a question to support")]
async fn founder_emails(world: &mut NexusWorld) {
    let cfg = ThreadConfig::from_env().expect("thread config");
    let inbound = InboundEmail {
        from: FOUNDER_EMAIL.into(),
        to: format!("support@{PARSE_HOST}"),
        subject: "Quick question on a vendor contract".into(),
        text: "Can we sign the new vendor MSA as-is, or do you want to review it first?".into(),
        raw: b"vendor question".to_vec(),
        dkim: String::new(),
        attachments: Vec::new(),
        quarantined_attachments: Vec::new(),
        message_id: None,
    };
    let j = world.journey();
    thread_inbound(
        &j.surreal,
        &j.storage,
        world.email.as_ref().expect("email").as_ref(),
        j.runtime.as_ref(),
        &cfg,
        &inbound,
        "raw/nexus-question.eml",
    )
    .await
    .expect("thread inbound");
}

#[then("the question is routed to a lawyer")]
async fn assert_routed(world: &mut NexusWorld) {
    let captured = world.email.as_ref().expect("email").captured();
    assert!(
        captured.iter().any(|m| m.to == "lawyer@neonlaw.com"),
        "the founder's question should be routed to a lawyer",
    );
}

#[tokio::main]
async fn main() {
    NexusWorld::cucumber()
        .run_and_exit("tests/features/nexus_fractional_gc.feature")
        .await;
}
