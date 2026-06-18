//! Worker-side step dispatch for `email_send__<slug>` states.
//!
//! When a workflow transitions into an `email_send__welcome` state,
//! `workflows-service` calls [`dispatch_state`] to:
//!
//! 1. Parse the `<slug>` out of the state name.
//! 2. Look up the [`super::Template`] for that slug.
//! 3. Render the body using the per-template renderer (currently
//!    only `welcome::render`; new templates each export their own).
//! 4. Hand the resulting [`OutboundEmail`] to the injected
//!    [`EmailService`].
//!
//! The fn is pure relative to the injected service — i.e. it does
//! one `send` call and returns the outcome, leaving journaling to
//! the caller (`workflows-service` wraps it inside `ctx.run` so the
//! Restate journal stamps each side effect).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::service::{EmailError, EmailService, OutboundEmail};
use super::welcome;

/// Recipient payload for an `email_send__*` step. Carried as the
/// workflow's input through Restate state so the dispatch handler
/// can render the template at signal time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailPayload {
    pub name: String,
    pub email: String,
}

impl EmailPayload {
    #[must_use]
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("state `{0}` is not an `email_send__<slug>` step")]
    NotAnEmailSendState(String),
    #[error("no email template registered for slug `{0}`")]
    UnknownTemplate(String),
    #[error("email send failed: {0}")]
    Send(#[from] EmailError),
}

/// Resolve `email_send__<slug>` → `<slug>`, render the matching
/// template against `payload`, and send through `svc`. Returns the
/// [`OutboundEmail`] that was handed to the service so the caller
/// can journal it.
///
/// Errors deterministically (not retried by Restate) on
/// [`DispatchError::NotAnEmailSendState`] and
/// [`DispatchError::UnknownTemplate`]; only [`DispatchError::Send`]
/// reflects a transient-or-not transport result whose retry
/// behavior is the caller's choice.
pub async fn dispatch_state(
    svc: &dyn EmailService,
    state_name: &str,
    payload: &EmailPayload,
) -> Result<OutboundEmail, DispatchError> {
    let slug = parse_slug(state_name)?;
    let (subject, body, html) = render_for_slug(slug, payload)?;
    let email = OutboundEmail {
        to: payload.email.clone(),
        subject,
        body,
        html_body: Some(html),
        template_slug: Some(slug.to_string()),
        from: None,
        reply_to: None,
        // `EmailPayload` carries no `persons.id` yet, so workflow-driven
        // sends can't stamp `person_id` for the delivery-side join.
        // Threading it through the payload is a follow-up; the join
        // still works on `template_slug` in the meantime.
        person_id: None,
        // Workflow-driven sends aren't part of a support thread.
        in_reply_to: None,
        references: None,
    };
    svc.send(email.clone()).await?;
    Ok(email)
}

/// Returns the `<slug>` part of an `email_send__<slug>` state name,
/// or `None` if the prefix doesn't match.
pub fn parse_slug(state_name: &str) -> Result<&str, DispatchError> {
    state_name
        .strip_prefix("email_send__")
        .ok_or_else(|| DispatchError::NotAnEmailSendState(state_name.to_string()))
}

/// Render a template to `(subject, plain_body, html_body)`. The HTML
/// body uses the logo origin from [`super::layout::base_url_from_env`]
/// (driven by `NAV_BASE_URL`); the worker runs per-deploy so reading
/// the env here keeps the dispatch signature payload-only.
fn render_for_slug(
    slug: &str,
    payload: &EmailPayload,
) -> Result<(String, String, String), DispatchError> {
    match slug {
        "welcome" => {
            let base_url = super::layout::base_url_from_env();
            Ok((
                welcome::welcome_subject(),
                welcome::render_welcome_body(&payload.name, &payload.email),
                welcome::render_welcome_html(&payload.name, &payload.email, &base_url),
            ))
        }
        other => Err(DispatchError::UnknownTemplate(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::super::service::CapturingEmail;
    use super::{dispatch_state, parse_slug, DispatchError, EmailPayload};

    #[test]
    fn parse_slug_extracts_welcome() {
        assert_eq!(parse_slug("email_send__welcome").unwrap(), "welcome");
    }

    #[test]
    fn parse_slug_rejects_non_email_send_state() {
        let err = parse_slug("signature__signed").unwrap_err();
        assert!(matches!(err, DispatchError::NotAnEmailSendState(_)));
    }

    #[tokio::test]
    async fn dispatch_state_sends_welcome_through_capturing_backend() {
        let svc = CapturingEmail::new();
        let payload = EmailPayload::new("Aries", "aries@example.com");
        let sent = dispatch_state(&svc, "email_send__welcome", &payload)
            .await
            .expect("welcome dispatch must succeed");

        // The return value mirrors the in-flight OutboundEmail so the
        // caller can journal it without re-rendering.
        assert_eq!(sent.to, "aries@example.com");
        assert_eq!(sent.subject, "Welcome to Neon Law");
        assert_eq!(sent.template_slug.as_deref(), Some("welcome"));
        assert!(sent.body.contains("Aries"));
        assert!(sent.body.contains("aries@example.com"));

        // A styled HTML alternative is attached so rich clients render
        // the logo + formatting; text-only clients fall back to `body`.
        let html = sent.html_body.as_deref().expect("html alternative set");
        assert!(html.contains("logo-firm.png"));
        assert!(html.contains("Aries"));

        // And the same email actually went through the service.
        let captured = svc.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].to, "aries@example.com");
        assert_eq!(captured[0].subject, "Welcome to Neon Law");
        assert_eq!(captured[0].template_slug.as_deref(), Some("welcome"));
    }

    #[tokio::test]
    async fn dispatch_state_rejects_unknown_template_slug() {
        let svc = CapturingEmail::new();
        let payload = EmailPayload::new("x", "x@y");
        let err = dispatch_state(&svc, "email_send__password_reset", &payload)
            .await
            .unwrap_err();
        assert!(matches!(err, DispatchError::UnknownTemplate(slug) if slug == "password_reset"));
        assert!(svc.captured().is_empty());
    }

    #[tokio::test]
    async fn dispatch_state_rejects_non_email_send_state_name() {
        let svc = CapturingEmail::new();
        let payload = EmailPayload::new("x", "x@y");
        let err = dispatch_state(&svc, "BEGIN", &payload).await.unwrap_err();
        assert!(matches!(err, DispatchError::NotAnEmailSendState(_)));
    }
}
