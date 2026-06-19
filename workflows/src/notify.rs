//! Ops-notification seam — the chat sibling of [`crate::email::EmailService`].
//!
//! Navigator's durable workflows already prove their liveness by *emailing*
//! firm ops (the six-hourly `Heartbeat`, the nightly `Archives` digest, the
//! `BillingCanary`, …). Those emails arrive over and over, so the signal is
//! easy to miss in an inbox — the very failure mode that hid a real heartbeat
//! gap once. This module adds a second, parallel delivery path: an incoming
//! **Slack** webhook to the engineering channel, so the same internal signal
//! lands where engineers already watch.
//!
//! Two pieces, mirroring [`crate::email`] exactly:
//!
//! - [`Notifier`] — the trait, with a real [`SlackNotifier`] (POSTs
//!   `{"text": …}` to an incoming webhook) and a [`CapturingNotifier`] that
//!   keeps messages in memory for KIND/tests so nothing leaves the binary.
//! - [`OpsEmailMirror`] — an [`EmailService`] decorator that sends the email
//!   through its inner backend **and** best-effort mirrors it to a notifier.
//!
//! **The load-bearing boundary:** [`OpsEmailMirror`] must wrap **only**
//! internal/operations email services — `Heartbeat`, `Archives`, `Statutes`,
//! `BillingCanary`, `BillingDigest`. Those carry no client, matter, or PII
//! data (their recipients are env-pinned to firm ops). It must **never** wrap
//! a client-facing email service (`Notation`, `RecurringBilling` invoices):
//! mirroring client email into a chat channel would push client content across
//! the firm's trust boundary, violating the standing no-content rule (see the
//! `observability` skill). The boundary is enforced at the wiring point in
//! `workflows-service`'s `main.rs`, not by per-message inspection here.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use thiserror::Error;

use crate::email::service::{EmailError, EmailService, OutboundEmail, SendReceipt};

/// Why a notification failed to deliver. Distinct from [`EmailError`] because a
/// notifier outage must never be conflated with — nor fail — an email send.
#[derive(Debug, Error)]
pub enum NotifyError {
    /// The HTTP request to the webhook never completed (DNS, TLS, timeout).
    #[error("transport error: {0}")]
    Transport(String),
    /// The webhook returned a non-2xx status (e.g. 404 for a revoked webhook).
    #[error("notification endpoint rejected the message: status {0}")]
    Rejected(u16),
}

/// A one-way channel for internal operations notifications. Implementors post a
/// short plain-text message somewhere firm engineers watch (Slack today).
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Deliver `text`. Returns `Ok(())` on a successful post.
    async fn notify(&self, text: String) -> Result<(), NotifyError>;
}

/// Captures every message in memory instead of sending it. Used by tests and
/// KIND/dev, where posting to the real engineering channel would be noise.
#[derive(Clone, Default)]
pub struct CapturingNotifier {
    sent: Arc<Mutex<Vec<String>>>,
}

impl CapturingNotifier {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every message handed to [`Notifier::notify`] so far.
    #[must_use]
    pub fn captured(&self) -> Vec<String> {
        self.sent.lock().expect("notifier lock poisoned").clone()
    }
}

#[async_trait]
impl Notifier for CapturingNotifier {
    async fn notify(&self, text: String) -> Result<(), NotifyError> {
        self.sent.lock().expect("notifier lock poisoned").push(text);
        Ok(())
    }
}

/// Posts to a Slack **incoming webhook**. The webhook URL already pins the
/// destination channel, so the only payload is the message text.
#[derive(Clone)]
pub struct SlackNotifier {
    http: reqwest::Client,
    webhook_url: String,
}

impl SlackNotifier {
    /// Production constructor: targets the given incoming-webhook URL
    /// (`SLACK_WEBHOOK_URL`).
    #[must_use]
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            webhook_url: webhook_url.into(),
        }
    }

    /// Build the JSON body for the Slack incoming-webhook endpoint. Pure —
    /// exposed for unit-testing the request shape without an HTTP round-trip.
    #[must_use]
    pub fn build_request_body(text: &str) -> serde_json::Value {
        json!({ "text": text })
    }
}

#[async_trait]
impl Notifier for SlackNotifier {
    async fn notify(&self, text: String) -> Result<(), NotifyError> {
        let resp = self
            .http
            .post(&self.webhook_url)
            .json(&Self::build_request_body(&text))
            .send()
            .await
            .map_err(|e| NotifyError::Transport(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(NotifyError::Rejected(status.as_u16()))
        }
    }
}

/// Render the Slack message for an internal ops email: the subject as a bold
/// header, then the plain-text body. Pure and exposed so the formatting is
/// unit-tested. The HTML part is intentionally dropped — Slack renders the
/// plain text and the body already reads as a standalone ops notice.
#[must_use]
pub fn ops_slack_text(email: &OutboundEmail) -> String {
    format!("*{}*\n{}", email.subject, email.body)
}

/// An [`EmailService`] decorator that, on every send, delivers the email
/// through `inner` **and then** best-effort mirrors it to `notifier`.
///
/// Email is the source of truth: a notifier failure is logged and swallowed,
/// never propagated, so a Slack outage can never fail the durable email step
/// (the workflow's `ctx.run("notify")`) and trigger a spurious retry. Wrap
/// ONLY internal/ops email services — see the module-level boundary note.
#[derive(Clone)]
pub struct OpsEmailMirror {
    inner: Arc<dyn EmailService>,
    notifier: Arc<dyn Notifier>,
}

impl OpsEmailMirror {
    #[must_use]
    pub fn new(inner: Arc<dyn EmailService>, notifier: Arc<dyn Notifier>) -> Self {
        Self { inner, notifier }
    }
}

#[async_trait]
impl EmailService for OpsEmailMirror {
    async fn send(&self, email: OutboundEmail) -> Result<SendReceipt, EmailError> {
        // Render the mirror text before `inner.send` consumes the email.
        let text = ops_slack_text(&email);
        let receipt = self.inner.send(email).await?;
        if let Err(err) = self.notifier.notify(text).await {
            // Best effort: the email already went out, so do not fail the
            // durable step. Carry no body — only the error, an identifier-safe
            // signal — to stay within the no-content rule.
            tracing::warn!(error = %err, "ops Slack mirror failed; email was sent");
        }
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ops_slack_text, CapturingNotifier, Notifier, NotifyError, OpsEmailMirror, SlackNotifier,
    };
    use crate::email::service::{CapturingEmail, EmailService, OutboundEmail};
    use async_trait::async_trait;
    use std::sync::Arc;

    fn ops_email() -> OutboundEmail {
        OutboundEmail::new(
            "nick@neonlaw.com",
            "Durable execution OK — heartbeat 2026-06-19 18:00 UTC",
            "The durable-execution heartbeat ran end to end.",
        )
    }

    #[test]
    fn slack_body_is_text_field() {
        let body = SlackNotifier::build_request_body("hello ops");
        assert_eq!(body, serde_json::json!({ "text": "hello ops" }));
    }

    #[test]
    fn ops_slack_text_is_bold_subject_then_body() {
        let text = ops_slack_text(&ops_email());
        assert!(text.starts_with("*Durable execution OK"));
        assert!(text.contains("ran end to end"));
    }

    #[tokio::test]
    async fn capturing_notifier_records_messages() {
        let n = CapturingNotifier::new();
        n.notify("first".into())
            .await
            .expect("capturing never fails");
        n.notify("second".into())
            .await
            .expect("capturing never fails");
        assert_eq!(
            n.captured(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[tokio::test]
    async fn mirror_sends_email_and_notifies_on_success() {
        let email = Arc::new(CapturingEmail::new());
        let notifier = Arc::new(CapturingNotifier::new());
        let mirror = OpsEmailMirror::new(email.clone(), notifier.clone());

        mirror.send(ops_email()).await.expect("send succeeds");

        assert_eq!(email.captured().len(), 1, "email delivered through inner");
        assert_eq!(notifier.captured().len(), 1, "mirrored to Slack");
        assert!(notifier.captured()[0].contains("heartbeat"));
    }

    /// A notifier whose every send fails — proves the mirror swallows the error.
    struct FailingNotifier;
    #[async_trait]
    impl Notifier for FailingNotifier {
        async fn notify(&self, _text: String) -> Result<(), NotifyError> {
            Err(NotifyError::Rejected(404))
        }
    }

    #[tokio::test]
    async fn mirror_still_sends_email_when_notifier_fails() {
        let email = Arc::new(CapturingEmail::new());
        let mirror = OpsEmailMirror::new(email.clone(), Arc::new(FailingNotifier));

        // A Slack outage must NOT fail the durable email step.
        mirror
            .send(ops_email())
            .await
            .expect("email send succeeds despite notifier failure");

        assert_eq!(email.captured().len(), 1, "email still delivered");
    }
}
