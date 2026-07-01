//! Cucumber runner for `features/nexus_fractional_gc.feature`.
//!
//! The Nexus journey models an *ongoing* relationship rather than a
//! one-shot matter, so it stitches three surfaces around the signed
//! engagement: the admin walker (sign the engagement letter — a stub
//! `onboarding__nexus` template), the `repos` engine (deliver work product
//! into the Project repo, visible in the listing), and the
//! `web::email_threads` engine (route the founder's question to staff).

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::journey::{answer_body, client, Journey};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
use store::entity;
use uuid::Uuid;
use web::email::CapturingEmail;
use web::email_threads::{thread_inbound, ThreadConfig};
use web::inbound_email::InboundEmail;
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

    fn project_id(&self) -> Uuid {
        self.project_id.expect("project not resolved")
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with a fractional-GC engagement$"#)]
async fn seed_client(world: &mut NexusWorld, name: String, email: String) {
    std::env::set_var(
        repos::REPO_ROOT_ENV,
        std::env::temp_dir().join("navigator-features-nexus-git"),
    );
    std::env::set_var("NAVIGATOR_PARSE_HOST", PARSE_HOST);
    std::env::set_var("NAVIGATOR_STAFF_NOTIFY_EMAIL", "staff@neonlaw.com");

    let journey = Journey::open("nexus").await;
    client(&journey.db, &name, &email).await;
    world.email = Some(Arc::new(CapturingEmail::new()));
    world.journey = Some(journey);
}

#[given(regex = r#"^a staff member "([^"]+)"$"#)]
async fn seed_staff(world: &mut NexusWorld, email: String) {
    entity::person::ActiveModel {
        name: ActiveValue::Set("Neon Law Staff".into()),
        email: ActiveValue::Set(email),
        role: ActiveValue::Set(entity::person::Role::Staff),
        ..Default::default()
    }
    .insert(&world.journey().db)
    .await
    .expect("insert staff");
}

#[when("the firm opens the Nexus engagement for the founder")]
async fn open_engagement(world: &mut NexusWorld) {
    let body = format!(
        "client_email={}&retainer_template_code=onboarding__nexus",
        features::form_encode(FOUNDER_EMAIL),
    );
    let resp = world
        .journey()
        .staff_post("/portal/admin/retainers/new", body)
        .await;
    let location = resp
        .location
        .unwrap_or_else(|| panic!("opening the engagement did not redirect ({})", resp.status));
    let id = location
        .strip_prefix("/portal/admin/notations/")
        .and_then(|s| s.strip_suffix("/step"))
        .unwrap_or_else(|| panic!("unexpected redirect: {location}"));
    let notation_id = Uuid::parse_str(id).expect("notation id");

    // Walk the three onboarding questions; the last drives the workflow to
    // the signature wait.
    let path = format!("/portal/admin/notations/{notation_id}/step");
    for value in [
        "Sagittarius",
        "Horizon Robotics LLC",
        "Outside general counsel: contracts, corporate housekeeping, day-to-day questions",
    ] {
        let resp = world.journey().staff_post(&path, answer_body(value)).await;
        assert!(
            resp.status.is_success() || resp.status.is_redirection(),
            "answering {value:?} returned {}",
            resp.status,
        );
    }

    // Resolve the project the walker created for the repo + later steps.
    let notation = entity::notation::Entity::find_by_id(notation_id)
        .one(&world.journey().db)
        .await
        .unwrap()
        .expect("notation exists");
    world.project_id = Some(notation.project_id);
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
    let project_id = world.project_id();
    store.ensure(project_id).expect("ensure repo");
    store
        .commit_as(
            project_id,
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
        .read_head_tree(world.project_id())
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
        message_id: None,
    };
    let j = world.journey();
    thread_inbound(
        &j.db,
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

#[then("the question is routed to staff")]
async fn assert_routed(world: &mut NexusWorld) {
    let captured = world.email.as_ref().expect("email").captured();
    assert!(
        captured.iter().any(|m| m.to == "staff@neonlaw.com"),
        "the founder's question should be routed to staff",
    );
}

#[tokio::main]
async fn main() {
    NexusWorld::cucumber()
        .run("tests/features/nexus_fractional_gc.feature")
        .await;
}
