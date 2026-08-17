//! The `GitHubAutomationHeartbeat` Restate workflow — the authority canary for
//! Navigator's GitHub engineering automation.
//!
//! [`crate::heartbeat::HeartbeatService`] proves only that the shared durable
//! engine can journal and notify. This workflow is separately bound only in
//! the automation home, so its scheduled Slack notice additionally
//! proves that the one allowed GitHub-automation worker is registered and can
//! complete a durable two-step invocation. It does not receive a webhook, read
//! GitHub content, mint an installation token, or spend an agent token.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use workflows::Notifier;

/// Empty request body for the scheduled authority canary.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// The journaled liveness result returned to the Restate invocation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct GitHubAutomationHeartbeatReport {
    /// Restate's identifier for this durable invocation.
    pub invocation_id: String,
    /// The instant the durable `beat` step ran.
    pub beat_at: DateTime<Utc>,
}

/// Render the body-free Slack notice for a completed authority canary.
#[must_use]
pub fn github_automation_heartbeat_message(report: &GitHubAutomationHeartbeatReport) -> String {
    let beat = report.beat_at.format("%Y-%m-%d %H:%M UTC");
    format!("🤖 GitHub automation authority OK — heartbeat {beat}")
}

/// Authority-only durable workflow. The worker's startup branch is the
/// security boundary: non-authoritative deployments never bind this service.
#[derive(Clone)]
pub struct GitHubAutomationHeartbeatService {
    notifier: Arc<dyn Notifier>,
}

impl GitHubAutomationHeartbeatService {
    #[must_use]
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        Self { notifier }
    }
}

#[restate_sdk::workflow(name = "GitHubAutomationHeartbeat")]
impl GitHubAutomationHeartbeatService {
    #[restate_sdk::handler]
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _request: Json<RunRequest>,
    ) -> Result<Json<GitHubAutomationHeartbeatReport>, HandlerError> {
        let invocation_id = ctx.invocation_id().to_string();
        let report = ctx
            .run(|| async {
                Ok(Json(GitHubAutomationHeartbeatReport {
                    invocation_id: invocation_id.clone(),
                    beat_at: Utc::now(),
                }))
            })
            .name("beat")
            .await?
            .into_inner();

        let message = github_automation_heartbeat_message(&report);
        let notifier = Arc::clone(&self.notifier);
        ctx.run(move || async move { notifier.notify(message).await.map_err(HandlerError::from) })
            .name("notify")
            .await?;

        Ok(Json(report))
    }
}

#[cfg(test)]
mod tests {
    use super::{github_automation_heartbeat_message, GitHubAutomationHeartbeatReport};
    use chrono::{TimeZone, Utc};

    #[test]
    fn authority_canary_notice_is_one_line_and_body_free() {
        let report = GitHubAutomationHeartbeatReport {
            invocation_id: "inv_abc123".into(),
            beat_at: Utc.with_ymd_and_hms(2026, 7, 27, 18, 0, 0).unwrap(),
        };

        let message = github_automation_heartbeat_message(&report);
        assert_eq!(
            message,
            "🤖 GitHub automation authority OK — heartbeat 2026-07-27 18:00 UTC"
        );
        assert!(!message.contains('\n'));
        assert!(!message.contains(&report.invocation_id));
    }
}
