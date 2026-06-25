//! Ops-notification seam — the chat sibling of [`crate::email::EmailService`].
//!
//! Neon Law Navigator's durable workflows prove their liveness by notifying firm ops
//! (the six-hourly `Heartbeat`, the nightly `Archives` digest, the
//! `BillingCanary`, …). That signal goes to an incoming **Slack** webhook on
//! the engineering channel — where engineers already watch — and **no longer
//! also goes out as email**: a recurring liveness signal is trivial to lose in
//! an inbox (the very failure mode that hid a real heartbeat gap once), so once
//! Slack delivery proved reliable the firm dropped the duplicate ops email (the
//! follow-up to the dual-send introduced in PR #13).
//!
//! Two pieces:
//!
//! - [`Notifier`] — the trait, with a real [`SlackNotifier`] (POSTs
//!   `{"text": …}` to an incoming webhook) and a [`CapturingNotifier`] that
//!   keeps messages in memory for KIND/tests so nothing leaves the binary.
//! - [`SlackOpsDelivery`] — an [`EmailService`] adapter that delivers an ops
//!   notice to a [`Notifier`] (Slack) *instead of* sending email. The ops
//!   workflows render their notice as an [`OutboundEmail`] and hand it to their
//!   `EmailService`; wiring them with this adapter routes the notice to Slack
//!   and sends no mail at all.
//!
//! **The load-bearing boundary:** [`SlackOpsDelivery`] must back **only**
//! internal/operations services — `Heartbeat`, `Archives`, `Statutes`,
//! `BillingCanary`, `BillingDigest`. Those carry no client, matter, or PII
//! data (their recipients are env-pinned to firm ops). It must **never** back
//! a client-facing email service (`Notation`, `RecurringBilling` invoices):
//! pushing client content into a chat channel would cross the firm's trust
//! boundary, violating the standing no-content rule (see the `observability`
//! skill). The boundary is enforced at the wiring point in `workflows-service`'s
//! `main.rs`, not by per-message inspection here.

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

/// An [`EmailService`] adapter that delivers an ops notice to a [`Notifier`]
/// (Slack) **instead of** sending email. The internal/ops workflows render
/// their notice as an [`OutboundEmail`] and hand it to their `EmailService`;
/// backing them with this adapter posts that notice to the engineering channel
/// and sends no mail.
///
/// Slack is now the single delivery path for the ops signal, so — unlike the
/// former best-effort dual-send mirror — a delivery failure **is** propagated
/// as an [`EmailError`]. That fails the workflow's durable `ctx.run("notify")`
/// step, so Restate retries and redelivers once Slack recovers, rather than
/// silently dropping the only copy of the signal. Back ONLY internal/ops
/// services — see the module-level boundary note.
#[derive(Clone)]
pub struct SlackOpsDelivery {
    notifier: Arc<dyn Notifier>,
}

impl SlackOpsDelivery {
    #[must_use]
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        Self { notifier }
    }
}

#[async_trait]
impl EmailService for SlackOpsDelivery {
    async fn send(&self, email: OutboundEmail) -> Result<SendReceipt, EmailError> {
        self.notifier
            .notify(ops_slack_text(&email))
            .await
            .map_err(|err| EmailError::Transport(err.to_string()))?;
        // No mail was sent, so there is no provider message id to surface.
        Ok(SendReceipt { message_id: None })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ops_slack_text, CapturingNotifier, Notifier, NotifyError, SlackNotifier, SlackOpsDelivery,
    };
    use crate::email::service::{EmailError, EmailService, OutboundEmail};
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
    async fn delivery_posts_notice_to_slack_only() {
        let notifier = Arc::new(CapturingNotifier::new());
        let delivery = SlackOpsDelivery::new(notifier.clone());

        delivery.send(ops_email()).await.expect("send succeeds");

        assert_eq!(notifier.captured().len(), 1, "notice posted to Slack");
        assert!(notifier.captured()[0].contains("heartbeat"));
    }

    /// A notifier whose every send fails — proves the adapter surfaces the error.
    struct FailingNotifier;
    #[async_trait]
    impl Notifier for FailingNotifier {
        async fn notify(&self, _text: String) -> Result<(), NotifyError> {
            Err(NotifyError::Rejected(404))
        }
    }

    #[tokio::test]
    async fn delivery_propagates_failure_so_durable_step_retries() {
        let delivery = SlackOpsDelivery::new(Arc::new(FailingNotifier));

        // Slack is the only delivery path now, so a Slack outage MUST fail the
        // durable notify step (Restate then retries) rather than be swallowed.
        let err = delivery
            .send(ops_email())
            .await
            .expect_err("a Slack failure must surface to the caller");

        assert!(matches!(err, EmailError::Transport(_)));
    }
}
