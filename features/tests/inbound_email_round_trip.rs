//! Cucumber runner for `features/inbound_email_round_trip.feature`.
//!
//! The inbound-email "headless Front" journey: a client emails support, the
//! firm threads it, an attorney binds the thread to the matter with an
//! `@link` staff command, and the attorney's reply relays back. Driven
//! against the `web::email_threads` engine (the webhook itself is multipart;
//! the engine is the testable seam), with a `CapturingEmail` standing in
//! for the outbound backend so the staff notification and the client relay
//! can be asserted.

// Cucumber's step-attribute macros require `async fn`, so assertion
// steps that don't await anything still have to be declared async.
#![allow(clippy::unused_async)]

use std::sync::Arc;

use cucumber::{given, then, when, World};
use features::journey::{client, matter, Journey};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use store::entity;
use uuid::Uuid;
use web::email::CapturingEmail;
use web::email_threads::{thread_inbound, ThreadConfig};
use web::inbound_email::InboundEmail;

const PARSE_HOST: &str = "parse.neonlaw.test";
const CLIENT_EMAIL: &str = "aries@example.com";

#[derive(Default, World)]
#[world(init = Self::default)]
struct EmailWorld {
    journey: Option<Journey>,
    email: Option<Arc<CapturingEmail>>,
    notation_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
    reply_to: Option<String>,
}

impl std::fmt::Debug for EmailWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailWorld")
            .field("conversation_id", &self.conversation_id)
            .finish_non_exhaustive()
    }
}

impl EmailWorld {
    fn journey(&self) -> &Journey {
        self.journey.as_ref().expect("journey not built")
    }

    fn email(&self) -> &Arc<CapturingEmail> {
        self.email.as_ref().expect("email not built")
    }

    fn cfg() -> ThreadConfig {
        ThreadConfig::from_env().expect("NAVIGATOR_PARSE_HOST + staff-notify set in build_app")
    }

    async fn deliver(&self, inbound: &InboundEmail, raw_key: &str) {
        let j = self.journey();
        thread_inbound(
            &j.db,
            &j.storage,
            self.email().as_ref(),
            j.runtime.as_ref(),
            &Self::cfg(),
            inbound,
            raw_key,
        )
        .await
        .expect("thread inbound");
    }
}

fn inbound(from: &str, to: &str, subject: &str, text: &str) -> InboundEmail {
    InboundEmail {
        from: from.to_string(),
        to: to.to_string(),
        subject: subject.to_string(),
        text: text.to_string(),
        raw: text.as_bytes().to_vec(),
        dkim: String::new(),
        attachments: Vec::new(),
        message_id: None,
    }
}

#[given(regex = r#"^a client named "([^"]+)" <([^>]+)> with two open matters$"#)]
async fn seed_client(world: &mut EmailWorld, name: String, email: String) {
    // Wire the threading layer's env (DKIM domain left unset, so staff
    // commands are trusted on role alone in this test).
    std::env::set_var("NAVIGATOR_PARSE_HOST", PARSE_HOST);
    std::env::set_var("NAVIGATOR_STAFF_NOTIFY_EMAIL", "staff@neonlaw.com");

    let journey = Journey::open("inbound-email").await;
    let person = client(&journey.db, &name, &email).await;
    let project_id = matter(&journey.db, person.id, "Aries open matter").await;
    // A real matter notation to @link the thread to.
    let template = entity::template::Entity::find()
        .filter(entity::template::Column::Code.eq("onboarding__retainer_nest"))
        .one(&journey.db)
        .await
        .unwrap()
        .expect("onboarding__retainer_nest seeded");
    let notation_id = entity::notation::ActiveModel {
        template_id: ActiveValue::Set(template.id),
        person_id: ActiveValue::Set(person.id),
        project_id: ActiveValue::Set(project_id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&journey.db)
    .await
    .unwrap()
    .id;
    // A second open matter makes the sender ambiguous, so the engine's
    // auto-route declines (it threads straight onto a *sole* open matter
    // only) and leaves the conversation unlinked — exactly the case the
    // attorney's `@link` command disambiguates. The thread binds to the
    // first matter (`notation_id`) only once the attorney links it.
    let second_matter = matter(&journey.db, person.id, "Aries second open matter").await;
    entity::notation::ActiveModel {
        template_id: ActiveValue::Set(template.id),
        person_id: ActiveValue::Set(person.id),
        project_id: ActiveValue::Set(second_matter),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(&journey.db)
    .await
    .unwrap();
    world.notation_id = Some(notation_id);
    world.email = Some(Arc::new(CapturingEmail::new()));
    world.journey = Some(journey);
}

#[given(regex = r#"^a staff member "([^"]+)"$"#)]
async fn seed_staff(world: &mut EmailWorld, email: String) {
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

#[when("the client emails support asking about their filing")]
async fn client_emails(world: &mut EmailWorld) {
    let to = format!("support@{PARSE_HOST}");
    let msg = inbound(
        CLIENT_EMAIL,
        &to,
        "Question about my filing",
        "Hi — can you tell me where my Nevada filing stands? Thanks.",
    );
    world.deliver(&msg, "raw/inbound-1.eml").await;

    // Resolve the conversation the engine just opened.
    let convo = entity::email_conversation::Entity::find()
        .filter(entity::email_conversation::Column::ExternalEmail.eq(CLIENT_EMAIL))
        .one(&world.journey().db)
        .await
        .unwrap()
        .expect("a conversation was opened");
    world.conversation_id = Some(convo.id);
}

#[then("a support conversation is opened for the client")]
async fn assert_conversation(world: &mut EmailWorld) {
    let convo = entity::email_conversation::Entity::find_by_id(world.conversation_id.unwrap())
        .one(&world.journey().db)
        .await
        .unwrap()
        .expect("conversation exists");
    assert_eq!(convo.external_email, CLIENT_EMAIL);
    assert!(
        convo.notation_id.is_none(),
        "a fresh conversation is not yet bound to a matter",
    );
}

#[then("the firm is notified with a reply-to thread token")]
async fn assert_staff_notified(world: &mut EmailWorld) {
    let captured = world.email().captured();
    let notice = captured
        .iter()
        .find(|m| m.to == "staff@neonlaw.com")
        .expect("staff were notified of the new conversation");
    let reply_to = notice
        .reply_to
        .clone()
        .expect("the notification carries a per-conversation reply-to token");
    assert!(
        reply_to.contains(PARSE_HOST),
        "the reply-to should be a thread token at the parse host: {reply_to}",
    );
    world.reply_to = Some(reply_to);
}

#[when(regex = r#"^the attorney replies "@link" to the thread and answers the client$"#)]
async fn staff_replies(world: &mut EmailWorld) {
    // Reply to the token address the notification carried, so the engine
    // threads it to the right conversation.
    let token_addr = world.reply_to.clone().expect("captured reply-to token");
    let notation_id = world.notation_id.unwrap();
    let body = format!(
        "@link {notation_id}\n\nThanks for reaching out — we have your filing in hand and it is on track."
    );
    let msg = inbound(
        "Neon Law <staff@neonlaw.com>",
        &token_addr,
        "Re: Question about my filing",
        &body,
    );
    world.deliver(&msg, "raw/inbound-2.eml").await;
}

#[then("the conversation is bound to the client's matter")]
async fn assert_bound(world: &mut EmailWorld) {
    let convo = entity::email_conversation::Entity::find_by_id(world.conversation_id.unwrap())
        .one(&world.journey().db)
        .await
        .unwrap()
        .expect("conversation exists");
    assert_eq!(
        convo.notation_id, world.notation_id,
        "the @link command should bind the conversation to the matter",
    );
}

#[then("the attorney's answer is relayed back to the client")]
async fn assert_relay(world: &mut EmailWorld) {
    let captured = world.email().captured();
    let relay = captured.iter().find(|m| m.to == CLIENT_EMAIL);
    let relay = relay.expect("the attorney's answer should relay to the client");
    assert!(
        relay.body.contains("on track"),
        "the relay should carry the attorney's prose, got: {}",
        relay.body,
    );
}

#[tokio::main]
async fn main() {
    EmailWorld::cucumber()
        .run("tests/features/inbound_email_round_trip.feature")
        .await;
}
