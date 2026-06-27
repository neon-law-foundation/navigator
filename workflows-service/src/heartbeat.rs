//! The `Heartbeat` Restate workflow — the durable-execution liveness canary.
//!
//! Every other scheduled workflow proves an *integration*: `Archives` proves
//! the database and GCS are reachable, `BillingCanary` proves Xero still
//! agrees with us. None of them answers the operator's bluntest question —
//! *is the durable-execution engine itself alive right now?* — because each
//! can go dark for a reason that has nothing to do with Restate (a GCS outage,
//! a revoked Xero token). The silence is ambiguous.
//!
//! `Heartbeat` removes the ambiguity. It depends on **nothing** — no
//! database, no object storage, no third-party API — so a green run can only
//! mean the engine accepted an invocation, journaled a step, and ran a second
//! step to completion. Two durable steps, each journaled independently:
//!
//! 1. `ctx.run("beat")` — capture the wall-clock instant the engine executed
//!    this step and journal it. The non-deterministic read (`Utc::now`) lives
//!    *inside* the journaled closure so a replay re-uses the recorded instant
//!    rather than reading the clock again — the same pattern `archives` and
//!    `billing-workflows` use.
//! 2. `ctx.run("notify")` — post the journaled beat to firm ops as a single
//!    line on the engineering Slack channel. Because step 2 reads step 1's
//!    *journaled* output, a worker crash between the two steps replays step 1
//!    from the journal instead of re-executing it: that replay is precisely the
//!    durability this workflow exists to prove.
//!
//! **Cadence: every six hours** (00:00 / 06:00 / 12:00 / 18:00 UTC), driven by
//! the `heartbeat-trigger` `CronJob`. A regular pulse, not a nightly one, so a
//! gap is noticed within hours rather than a day. The trigger keys each
//! invocation on the UTC **date + hour** (not the date alone) so the four
//! daily runs each get a distinct Restate workflow key — a date-only key would
//! make Restate dedupe three of the four into no-ops.
//!
//! The notice is an **internal operations signal**: it carries no client,
//! matter, or PII data — only a heart glyph, a liveness assertion, and the
//! journaled beat instant — and it posts straight to the Slack [`Notifier`],
//! whose webhook URL pins the destination to the firm-ops channel. It needs no
//! email framing (no recipient, subject, or HTML body): the heartbeat is a
//! liveness ping, not correspondence, and its *absence* — a six-hour window
//! with no heartbeat line in Slack — is the alarm. The operator runbook for
//! debugging that absence lives in `docs/durable-workflows.md`, not in the
//! recurring message itself.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use workflows::Notifier;

/// Request body for `Heartbeat::run`. Empty — the trigger only starts the
/// workflow — but kept as a struct so fields can be threaded later without
/// changing the handler signature.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// The journaled result of the beat step, surfaced as the Restate invocation
/// output and carried into the Slack notice.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HeartbeatReport {
    /// The Restate invocation id — the operator's deep-link handle into the
    /// Cloud console for this run.
    pub invocation_id: String,
    /// Wall-clock instant the `beat` step executed, captured inside the
    /// journaled step so a replay re-uses it rather than re-reading the clock.
    pub beat_at: DateTime<Utc>,
}

/// The one-line Slack notice for a completed heartbeat: a heart glyph, the
/// liveness assertion, and the journaled beat instant. Pure and exposed so the
/// formatting is unit-tested. The assertion is engine liveness only ("Durable
/// execution OK") — it never claims any backup or data write succeeded.
#[must_use]
pub fn heartbeat_message(report: &HeartbeatReport) -> String {
    let beat = report.beat_at.format("%Y-%m-%d %H:%M UTC");
    format!("💓 Durable execution OK — heartbeat {beat}")
}

#[restate_sdk::workflow]
#[name = "Heartbeat"]
pub trait Heartbeat {
    async fn run(req: Json<RunRequest>) -> Result<Json<HeartbeatReport>, HandlerError>;
}

/// Service registered with the Restate endpoint. Holds only the Slack
/// [`Notifier`] (for the notify step) — there is deliberately nothing else to
/// hold, because the whole point is to depend on nothing. Same shape as
/// `archives`'s `ArchivesService` and `billing-workflows`'s
/// `BillingCanaryService`, but it talks to the notifier directly rather than
/// through the `EmailService` ops-delivery adapter: the heartbeat has no email
/// framing to render.
#[derive(Clone)]
pub struct HeartbeatService {
    notifier: Arc<dyn Notifier>,
}

impl HeartbeatService {
    #[must_use]
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        Self { notifier }
    }
}

impl Heartbeat for HeartbeatService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: Json<RunRequest>,
    ) -> Result<Json<HeartbeatReport>, HandlerError> {
        let invocation_id = ctx.invocation_id().to_string();

        // Step 1 — "beat": journal the instant the engine ran this step. No
        // database, no object storage, no third party: a green beat means the
        // durable engine itself accepted the invocation and journaled a step.
        let report: HeartbeatReport = ctx
            .run(|| async {
                Ok(Json(HeartbeatReport {
                    invocation_id: invocation_id.clone(),
                    beat_at: Utc::now(),
                }))
            })
            .name("beat")
            .await?
            .into_inner();

        // Step 2 — "notify": post the journaled beat to the ops Slack channel,
        // journaled separately so a beat-step retry never re-posts and a
        // notify-step retry never re-reads the clock. Reading step 1's
        // journaled output here is the durability this workflow exists to
        // prove. A Slack failure surfaces as a `HandlerError`, so Restate
        // retries the step rather than dropping the only copy of the signal.
        let message = heartbeat_message(&report);
        let notifier = Arc::clone(&self.notifier);
        ctx.run(move || async move { notifier.notify(message).await.map_err(HandlerError::from) })
            .name("notify")
            .await?;

        Ok(Json(report))
    }
}

#[cfg(test)]
mod tests {
    use super::{heartbeat_message, HeartbeatReport};
    use chrono::{TimeZone, Utc};

    fn sample_report() -> HeartbeatReport {
        HeartbeatReport {
            invocation_id: "inv_abc123".into(),
            beat_at: Utc.with_ymd_and_hms(2026, 6, 12, 18, 0, 0).unwrap(),
        }
    }

    #[test]
    fn heartbeat_message_is_one_line_with_heart_and_liveness() {
        let msg = heartbeat_message(&sample_report());
        assert_eq!(
            msg,
            "💓 Durable execution OK — heartbeat 2026-06-12 18:00 UTC"
        );
        // One line: no embedded newline a chat client would split on.
        assert!(
            !msg.contains('\n'),
            "heartbeat notice must be a single line: {msg}"
        );
        // It carries the heart glyph and asserts only engine liveness.
        assert!(msg.starts_with('💓'));
        assert!(msg.contains("Durable execution OK"));
    }
}
