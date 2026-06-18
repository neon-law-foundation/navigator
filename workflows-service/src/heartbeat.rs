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
//! 2. `ctx.run("notify")` — email the operator the journaled beat. Because
//!    step 2 reads step 1's *journaled* output, a worker crash between the two
//!    steps replays step 1 from the journal instead of re-executing it: that
//!    replay is precisely the durability this workflow exists to prove.
//!
//! **Cadence: every six hours** (00:00 / 06:00 / 12:00 / 18:00 UTC), driven by
//! the `heartbeat-trigger` `CronJob`. A regular pulse, not a nightly one, so a
//! gap is noticed within hours rather than a day. The trigger keys each
//! invocation on the UTC **date + hour** (not the date alone) so the four
//! daily runs each get a distinct Restate workflow key — a date-only key would
//! make Restate dedupe three of the four into no-ops.
//!
//! The email is an **internal operations signal** (per the legal-council
//! review): it carries no client, matter, or PII data — only an invocation id,
//! a timestamp, and operator debugging links — its recipient is env-pinned to
//! firm ops and never client-supplied, and its subject claims only that the
//! engine ran two steps end-to-end, never that any backup or data write
//! succeeded. Every send carries a **"Where to look"** block with the exact
//! Restate Cloud and GCP console links and the kubectl/curl chain, so the same
//! email that confirms health also onboards whoever debugs its *absence*: a
//! six-hour window with no heartbeat means the engine may be down.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use workflows::{EmailService, OutboundEmail};

/// Default heartbeat recipient when `HEARTBEAT_NOTIFY_EMAIL` is unset.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// Request body for `Heartbeat::run`. Empty — the trigger only starts the
/// workflow — but kept as a struct so fields can be threaded later without
/// changing the handler signature.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// The journaled result of the beat step, surfaced as the Restate invocation
/// output and carried into the email.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HeartbeatReport {
    /// The Restate invocation id — the operator's deep-link handle into the
    /// Cloud console for this run.
    pub invocation_id: String,
    /// Wall-clock instant the `beat` step executed, captured inside the
    /// journaled step so a replay re-uses it rather than re-reading the clock.
    pub beat_at: DateTime<Utc>,
}

/// Operator debugging destinations, resolved from the worker's environment.
/// Built through a `key -> value` seam so the rendered links are unit-testable
/// without mutating process env. Every field is optional: an OSS fork or KIND
/// run with nothing set still gets a useful email (generic console hostnames +
/// the manual chain), while a configured prod deploy gets deep links.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpsLinks {
    /// Base URL of the Restate Cloud console for this environment
    /// (`RESTATE_CLOUD_CONSOLE_URL`) — the Invocations view lives under it.
    pub restate_console_url: Option<String>,
    /// GCP project id (`NAVIGATOR_GCP_PROJECT_ID`) — anchors Cloud Logging and
    /// GKE Workloads deep links.
    pub gcp_project: Option<String>,
}

impl OpsLinks {
    /// Resolve from a `key -> value` lookup (`std::env::var` in production).
    fn from_env<F: Fn(&str) -> Option<String>>(get: F) -> Self {
        let non_empty = |k: &str| get(k).filter(|s| !s.is_empty());
        Self {
            restate_console_url: non_empty("RESTATE_CLOUD_CONSOLE_URL"),
            gcp_project: non_empty("NAVIGATOR_GCP_PROJECT_ID"),
        }
    }
}

/// Best-effort liveness of the OpenTelemetry collector, reported in the email.
/// **Never fails the beat** — observability must not be a silent SPOF, but the
/// heartbeat's core claim (the durable engine is alive) depends on nothing, so
/// the collector probe is purely informational. Because logs dual-path to
/// stdout→Cloud Logging, an unreachable collector means "no live traces / no
/// lake telemetry," never "lost a log line."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectorStatus {
    /// A TCP connect to the collector's OTLP port succeeded.
    Reachable,
    /// The connect failed or timed out — live traces + lake telemetry may be
    /// dropping (stdout logs still reach Cloud Logging).
    Unreachable,
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` is unset (KIND / dev / OSS fork) — no
    /// collector to probe.
    NotConfigured,
}

impl CollectorStatus {
    fn line(self) -> &'static str {
        match self {
            Self::Reachable => "OTel collector: reachable.",
            Self::Unreachable => {
                "OTel collector: UNREACHABLE — live traces + lake telemetry may be dropping. \
                 Logs still reach Cloud Logging via stdout (dual-path), so no log line is lost. \
                 Check the otel-collector Deployment + the GMP drop alerts."
            }
            Self::NotConfigured => {
                "OTel collector: not configured (OTEL_EXPORTER_OTLP_ENDPOINT unset)."
            }
        }
    }
}

/// Parse `host:port` from an OTLP endpoint URL for a reachability probe. Strips
/// the scheme and any path; defaults the port to the OTLP gRPC `4317`. Pure, so
/// the parsing is unit-tested without a socket.
fn collector_addr_from_endpoint(endpoint: &str) -> Option<String> {
    let e = endpoint.trim();
    if e.is_empty() {
        return None;
    }
    let after_scheme = e.rsplit("://").next().unwrap_or(e);
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    if host_port.is_empty() {
        None
    } else if host_port.contains(':') {
        Some(host_port.to_string())
    } else {
        Some(format!("{host_port}:4317"))
    }
}

/// Best-effort TCP reachability probe of the collector. A 3s-bounded connect;
/// any error (or no endpoint) is reported, never propagated — the heartbeat
/// must still send even when the collector is down.
async fn probe_collector<F: Fn(&str) -> Option<String>>(get: F) -> CollectorStatus {
    let Some(endpoint) = get("OTEL_EXPORTER_OTLP_ENDPOINT").filter(|s| !s.is_empty()) else {
        return CollectorStatus::NotConfigured;
    };
    let Some(addr) = collector_addr_from_endpoint(&endpoint) else {
        return CollectorStatus::NotConfigured;
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => CollectorStatus::Reachable,
        _ => CollectorStatus::Unreachable,
    }
}

#[restate_sdk::workflow]
#[name = "Heartbeat"]
pub trait Heartbeat {
    async fn run(req: Json<RunRequest>) -> Result<Json<HeartbeatReport>, HandlerError>;
}

/// Service registered with the Restate endpoint. Holds only the worker-side
/// [`EmailService`] (for the notify step) — there is deliberately nothing else
/// to hold, because the whole point is to depend on nothing. Same shape as
/// `archives`'s `ArchivesService` and `billing-workflows`'s
/// `BillingCanaryService`.
#[derive(Clone)]
pub struct HeartbeatService {
    email: Arc<dyn EmailService>,
}

impl HeartbeatService {
    #[must_use]
    pub fn new(email: Arc<dyn EmailService>) -> Self {
        Self { email }
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

        // Step 2 — "notify": email the journaled beat, journaled separately so
        // a beat-step retry never re-sends and a notify-step retry never
        // re-reads the clock. Reading step 1's journaled output here is the
        // durability this workflow exists to prove.
        let links = OpsLinks::from_env(|k| std::env::var(k).ok());
        // Best-effort collector reachability for the email — informational,
        // never fails the beat (the engine-liveness claim depends on nothing).
        let collector = probe_collector(|k| std::env::var(k).ok()).await;
        let email = build_heartbeat_email(
            &report,
            &notify_recipient(|k| std::env::var(k).ok()),
            &links,
            collector,
        );
        let svc = Arc::clone(&self.email);
        ctx.run(move || async move {
            svc.send(email)
                .await
                .map(|_| ())
                .map_err(HandlerError::from)
        })
        .name("notify")
        .await?;

        Ok(Json(report))
    }
}

/// The heartbeat recipient: `HEARTBEAT_NOTIFY_EMAIL`, else the default. Takes a
/// `key -> value` lookup so it is unit-testable without mutating process env.
/// Env-pinned by design (legal-council ask) — the recipient is never derived
/// from any client or matter, so the signal can only ever reach firm ops.
fn notify_recipient<F: Fn(&str) -> Option<String>>(get: F) -> String {
    get("HEARTBEAT_NOTIFY_EMAIL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string())
}

/// Render the "Where to look" block: the concrete Restate Cloud + GCP console
/// destinations and the manual debugging chain. Pure and exposed so the link
/// resolution is unit-tested. Degrades gracefully — an unset env yields the
/// generic console hostname plus instructions rather than a broken deep link.
#[must_use]
pub fn render_ops_links(report: &HeartbeatReport, links: &OpsLinks) -> String {
    use std::fmt::Write as _;

    let mut out = String::from("Where to look if a heartbeat is missing:\n\n");
    let inv = &report.invocation_id;

    // 1) Restate Cloud — the invocation journal (did the worker run it?).
    match &links.restate_console_url {
        Some(base) => {
            let _ = write!(
                out,
                "  • Restate Cloud invocations: {base}\n    \
                 Filter service = Heartbeat; this run is invocation {inv}.\n"
            );
        }
        None => {
            let _ = write!(
                out,
                "  • Restate Cloud invocations: https://cloud.restate.dev \
                 (open your env → Invocations, filter service = Heartbeat).\n    \
                 This run is invocation {inv}.\n"
            );
        }
    }

    // 2) GCP Cloud Logging — the worker pod's logs (did a step error?).
    // 3) GCP GKE Workloads — is the workflows-service pod Running?
    match &links.gcp_project {
        Some(project) => {
            let _ = write!(
                out,
                "  • GCP worker logs: https://console.cloud.google.com/logs/query?project={project}\n    \
                 Filter to container_name=\"workflows-service\".\n  \
                 • GCP workloads: \
                 https://console.cloud.google.com/kubernetes/workload/overview?project={project}\n    \
                 Confirm workflows-service is Running.\n"
            );
        }
        None => out.push_str(
            "  • GCP console: https://console.cloud.google.com/ \
             (set NAVIGATOR_GCP_PROJECT_ID for deep links to logs + workloads).\n",
        ),
    }

    // 4) The manual chain — works regardless of which links resolved.
    out.push_str(
        "  • Manual chain (docs/durable-workflows.md):\n    \
         kubectl -n navigator get cronjob heartbeat-trigger   # did the trigger fire?\n    \
         401 at the ingress = stale RESTATE_AUTH_TOKEN; 404 = the service needs re-registering\n    \
         (a newly shipped Heartbeat 404s until `navigator restate register` re-snapshots the worker).\n",
    );
    out
}

/// Build the plain-text heartbeat email for a completed run. Pure — exposed
/// for unit-testing the rendered subject/body. The subject claims only engine
/// liveness ("Durable execution OK"), never data correctness.
#[must_use]
pub fn build_heartbeat_email(
    report: &HeartbeatReport,
    recipient: &str,
    links: &OpsLinks,
    collector: CollectorStatus,
) -> OutboundEmail {
    let beat = report.beat_at.format("%Y-%m-%d %H:%M UTC");
    let subject = format!("Durable execution OK — heartbeat {beat}");
    let body = format!(
        "The durable-execution heartbeat ran end to end.\n\n\
         Beat recorded at: {beat}\n\
         Restate invocation: {}\n\
         Cadence: every 6 hours (00:00 / 06:00 / 12:00 / 18:00 UTC).\n\n\
         This is a two-step Restate workflow that depends on nothing — no \
         database, no object storage, no third-party API. A green run means \
         the engine accepted the invocation, journaled step one, and ran step \
         two (this email) to completion. It does NOT assert that any backup or \
         data write succeeded — only that durable execution itself is alive.\n\n\
         {}\n\n\
         {}",
        report.invocation_id,
        collector.line(),
        render_ops_links(report, links),
    );
    // Wrap the same body in the firm-branded HTML layout so the ops email
    // carries the Neon Law logo; the plain-text body stays the fallback part.
    let html = workflows::email::render_email_html(
        &body,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    OutboundEmail::new(recipient.to_string(), subject, body).with_html(html)
}

#[cfg(test)]
mod tests {
    use super::{
        build_heartbeat_email, collector_addr_from_endpoint, notify_recipient, render_ops_links,
        CollectorStatus, HeartbeatReport, OpsLinks,
    };
    use chrono::{TimeZone, Utc};

    fn sample_report() -> HeartbeatReport {
        HeartbeatReport {
            invocation_id: "inv_abc123".into(),
            beat_at: Utc.with_ymd_and_hms(2026, 6, 12, 18, 0, 0).unwrap(),
        }
    }

    #[test]
    fn heartbeat_email_states_liveness_carries_invocation_and_cadence() {
        let email = build_heartbeat_email(
            &sample_report(),
            "ops@example.com",
            &OpsLinks::default(),
            CollectorStatus::Reachable,
        );
        assert_eq!(email.to, "ops@example.com");
        assert!(
            email.subject.contains("Durable execution OK"),
            "subject asserts engine liveness: {}",
            email.subject
        );
        assert!(email.subject.contains("2026-06-12 18:00 UTC"));
        assert!(email.body.contains("inv_abc123"));
        assert!(email.body.contains("every 6 hours"));
        // It must not over-claim — only engine liveness, never data success.
        assert!(email.body.contains("does NOT assert that any backup"));
        // It is branded HTML (firm logo) with the plain-text fallback retained.
        assert!(email.html_body.is_some());
    }

    #[test]
    fn heartbeat_email_reports_collector_status_without_changing_the_subject() {
        // The collector line appears in the body, but the subject still claims
        // ONLY engine liveness — an unreachable collector never demotes the
        // heartbeat's core signal.
        for (status, marker) in [
            (CollectorStatus::Reachable, "OTel collector: reachable."),
            (CollectorStatus::Unreachable, "UNREACHABLE"),
            (CollectorStatus::NotConfigured, "not configured"),
        ] {
            let email = build_heartbeat_email(
                &sample_report(),
                "ops@example.com",
                &OpsLinks::default(),
                status,
            );
            assert!(
                email.subject.contains("Durable execution OK"),
                "subject unchanged"
            );
            assert!(
                email.body.contains(marker),
                "body reports {status:?}: {}",
                email.body
            );
        }
        // The unreachable note reassures that logs are not lost (dual-path).
        let down = build_heartbeat_email(
            &sample_report(),
            "ops@example.com",
            &OpsLinks::default(),
            CollectorStatus::Unreachable,
        );
        assert!(down.body.contains("no log line is lost"));
    }

    #[test]
    fn collector_addr_parsing() {
        assert_eq!(
            collector_addr_from_endpoint("http://otel-collector.navigator.svc.cluster.local:4317"),
            Some("otel-collector.navigator.svc.cluster.local:4317".to_string())
        );
        // Scheme stripped, path dropped, default port applied when absent.
        assert_eq!(
            collector_addr_from_endpoint("https://collector.example"),
            Some("collector.example:4317".to_string())
        );
        assert_eq!(
            collector_addr_from_endpoint("otel:4317/v1/traces"),
            Some("otel:4317".to_string())
        );
        assert_eq!(collector_addr_from_endpoint("   "), None);
    }

    #[test]
    fn ops_links_deep_link_when_env_is_set() {
        let links = OpsLinks::from_env(|k| match k {
            "RESTATE_CLOUD_CONSOLE_URL" => {
                Some("https://navigator-prod.env.us.restate.cloud".into())
            }
            "NAVIGATOR_GCP_PROJECT_ID" => Some("neon-law-420305".into()),
            _ => None,
        });
        let block = render_ops_links(&sample_report(), &links);
        assert!(block.contains("https://navigator-prod.env.us.restate.cloud"));
        assert!(block.contains("invocation inv_abc123"));
        assert!(block.contains("project=neon-law-420305"));
        assert!(block.contains("workflows-service"));
        // The manual chain is always present.
        assert!(block.contains("kubectl -n navigator get cronjob heartbeat-trigger"));
        assert!(block.contains("re-register"));
    }

    #[test]
    fn ops_links_degrade_gracefully_when_env_is_unset() {
        let block = render_ops_links(&sample_report(), &OpsLinks::default());
        assert!(block.contains("https://cloud.restate.dev"));
        assert!(block.contains("https://console.cloud.google.com/"));
        assert!(block.contains("NAVIGATOR_GCP_PROJECT_ID"));
        // Still actionable without any env: the manual chain + invocation id.
        assert!(block.contains("inv_abc123"));
        assert!(block.contains("kubectl -n navigator get cronjob heartbeat-trigger"));
    }

    #[test]
    fn notify_recipient_defaults_then_honors_env() {
        assert_eq!(notify_recipient(|_| None), "nick@neonlaw.com");
        assert_eq!(
            notify_recipient(|_| Some(String::new())),
            "nick@neonlaw.com"
        );
        assert_eq!(
            notify_recipient(
                |k| (k == "HEARTBEAT_NOTIFY_EMAIL").then(|| "ops@example.com".to_string())
            ),
            "ops@example.com"
        );
    }
}
