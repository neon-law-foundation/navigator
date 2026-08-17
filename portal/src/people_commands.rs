//! Web adapter for the People command boundary.
//!
//! The DTOs, validation, and the create/update/delete writes live in
//! `store::people_commands` so `web`, `cli`, and `mcp` share one command
//! boundary without duplicating persistence. This module re-exports that
//! surface for the JSON `/app/api/people*` routes and the browser lawyer forms,
//! and owns only the one command that needs the mailer — `send_welcome`.

use std::sync::Arc;

use uuid::Uuid;

use crate::email::{EmailService, OutboundEmail};

pub use store::people_commands::{
    create_person, delete_person, parse_role, update_person, CreatePersonCommand,
    PeopleCommandError, UpdateContext, UpdatePersonCommand,
};

/// Render and dispatch the welcome email for one Person. Journals one
/// `sent_emails` row per attempt via the `LoggingEmail` decorator on the
/// injected service. Returns the recipient on success so the adapter can
/// personalize its confirmation.
pub async fn send_welcome(
    surreal: &store::surreal::SurrealDb,
    email: &Arc<dyn EmailService>,
    base_url: &str,
    id: Uuid,
) -> Result<store::persons::Person, PeopleCommandError> {
    let person = store::persons::find_by_id(surreal, id)
        .await
        .map_err(PeopleCommandError::Db)?
        .ok_or(PeopleCommandError::NotFound)?;
    let body = crate::welcome::render_welcome_body(&person.name, &person.email);
    let html = crate::welcome::render_welcome_html(&person.name, &person.email, base_url);
    let msg = OutboundEmail::new(
        person.email.clone(),
        crate::welcome::welcome_subject(),
        body,
    )
    .with_template("welcome")
    .with_html(html)
    .with_person(id.to_string());
    match email.send(msg).await {
        Ok(_) => Ok(person),
        Err(e) => {
            tracing::warn!(error = %e, person_id = %id, "people: welcome email send failed");
            Err(PeopleCommandError::SendFailed)
        }
    }
}
