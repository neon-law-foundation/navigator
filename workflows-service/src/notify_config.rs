//! Worker-side [`Notifier`](workflows::Notifier) factory.
//!
//! The chat sibling of [`crate::email_config`]. Selection is driven by a single
//! optional env var, `SLACK_WEBHOOK_URL`:
//!
//! - set (non-empty) → [`workflows::SlackNotifier`] posting to that incoming
//!   webhook (the engineering channel in prod).
//! - unset / empty → [`workflows::CapturingNotifier`], so KIND and tests never
//!   post to a real channel.
//!
//! Unlike the email backend there is no hard failure mode: a missing webhook is
//! a valid (capturing) configuration, so the worker never crash-loops over it.

use std::sync::Arc;

use workflows::{CapturingNotifier, Notifier, SlackNotifier};

/// Build the worker's notifier from the process environment.
#[must_use]
pub fn from_env() -> Arc<dyn Notifier> {
    from_lookup(|k| std::env::var(k).ok())
}

/// Testable seam: any `key -> Option<value>` lookup.
pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Arc<dyn Notifier> {
    match get("SLACK_WEBHOOK_URL").filter(|s| !s.is_empty()) {
        Some(url) => Arc::new(SlackNotifier::new(url)),
        None => Arc::new(CapturingNotifier::new()),
    }
}

/// Whether the configured backend is the real Slack webhook (for boot logging).
#[must_use]
pub fn slack_enabled<F: Fn(&str) -> Option<String>>(get: F) -> bool {
    get("SLACK_WEBHOOK_URL").is_some_and(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{from_lookup, slack_enabled};
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[tokio::test]
    async fn capturing_when_webhook_unset() {
        assert!(!slack_enabled(|_| None));
        // The capturing backend always accepts a message.
        from_lookup(|_| None)
            .notify("dev".into())
            .await
            .expect("capturing backend always succeeds");
    }

    #[test]
    fn capturing_when_webhook_empty() {
        assert!(!slack_enabled(lookup(&[("SLACK_WEBHOOK_URL", "")])));
    }

    #[test]
    fn slack_when_webhook_set() {
        assert!(slack_enabled(lookup(&[(
            "SLACK_WEBHOOK_URL",
            "https://hooks.slack.com/services/T/B/x"
        )])));
    }
}
