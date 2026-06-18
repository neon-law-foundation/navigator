//! The `Archives` Restate workflow.
//!
//! Hosted by the `workflows-service` worker (which binds it alongside
//! the `Notation` virtual object — all workflows live on that one
//! endpoint, so there is no separate always-on archives pod).
//!
//! One invocation == one nightly export. Four durable steps:
//!
//! 1. `ctx.run("snapshot", …)` — open the database + object storage,
//!    snapshot every registered table to Parquet on GCS, return a
//!    journaled [`SnapshotSummary`]. A transient failure (database or
//!    GCS unreachable) replays just this step.
//! 2. `ctx.run("cost", …)` — when `BILLING_EXPORT_TABLE` is set, query
//!    the GCP billing export for trailing-window spend by service and
//!    snapshot it to the export lake as `gcp_cost`. A clean no-op when
//!    the env var is unset (KIND / dev / OSS forks).
//! 3. `ctx.run("iceberg_telemetry", …)` — promote the day's telemetry
//!    Parquet (`iceberg/otel_*/data/dt=<date>/`) to Iceberg tables via
//!    the entity-table writer ([`crate::author_iceberg_for_prefix`]).
//!    Infallible: a telemetry-lake hiccup never fails the export; a
//!    no-op until the collector's OTLP→Parquet shim writes those files.
//! 4. `ctx.run("email", …)` — render the diagnostic email (snapshot
//!    outcomes + cost + telemetry-Iceberg summary) and send it through
//!    the worker's [`EmailService`]. Each step is journaled, so a retry
//!    re-uses the cached prior results rather than re-snapshotting.
//!
//! The `CronJob` `archives-trigger` POSTs to the Restate ingress to start
//! one invocation per night; Restate owns the retry schedule and the
//! invocation history the diagnostic email links back to.

use std::sync::Arc;

use chrono::NaiveDate;
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::Instrument;
use workflows::EmailService;

use billing::gcp_cost::CostReport;

use crate::email::{render_diagnostic, DiagnosticReport};
use crate::runner::{cost_phase, open_resources, snapshot_all, SnapshotSummary};

/// Default diagnostic-email recipient when `ARCHIVES_NOTIFY_EMAIL`
/// is unset.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// Request body for `Archives::run`. Empty today — the trigger only
/// needs to start the workflow — but kept as a struct (rather than
/// `()`) so fields like an override run-date can be threaded later
/// without breaking the handler signature.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// Summary returned to the caller (and visible in Restate Cloud as
/// the invocation's output).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunReport {
    pub run_date: NaiveDate,
    pub invocation_id: String,
    pub tables: usize,
    pub rows: usize,
    pub failures: usize,
}

#[restate_sdk::workflow]
#[name = "Archives"]
pub trait Archives {
    async fn run(req: Json<RunRequest>) -> Result<Json<RunReport>, HandlerError>;
}

/// Service struct registered with the Restate endpoint. Holds only
/// the worker-side [`EmailService`]; the database and object-storage
/// handles are opened inside the snapshot step so no connection is
/// held idle between nightly runs.
#[derive(Clone)]
pub struct ArchivesService {
    email: Arc<dyn EmailService>,
}

impl ArchivesService {
    #[must_use]
    pub fn new(email: Arc<dyn EmailService>) -> Self {
        Self { email }
    }
}

impl Archives for ArchivesService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: Json<RunRequest>,
    ) -> Result<Json<RunReport>, HandlerError> {
        let invocation_id = ctx.invocation_id().to_string();

        // Join the caller's trace: extract the W3C `traceparent` the trigger
        // injected (telemetry) from the invocation headers and parent this
        // run's span on it, so a `web`-initiated "run nightly export now" and
        // its durable steps appear as one trace. A no-op when none is present.
        let span = tracing::info_span!("archives.run", invocation_id = %invocation_id);
        {
            let headers = ctx.headers();
            telemetry::set_span_parent(
                &span,
                headers.get("traceparent").map(String::as_str),
                headers.get("tracestate").map(String::as_str),
            );
        }

        // `.instrument(span)` rather than `span.enter()`: the worker runs on a
        // multi-thread runtime, where a guard held across `.await` is the
        // documented footgun (it can leak to another task). Instrumenting the
        // future keeps the span attached only while this future is polled.
        async {
            // Phase 1 — snapshot. The whole loop is one journaled step:
            // open the handles fresh, snapshot every table, return the
            // serializable summary. `?` on `open_resources` yields a
            // retryable HandlerError so a database/GCS outage replays the
            // step.
            let summary: SnapshotSummary = ctx
                .run(|| async {
                    let (db, storage) = open_resources().await?;
                    Ok(Json(snapshot_all(&db, storage.as_ref()).await))
                })
                .name("snapshot")
                .await?
                .into_inner();

            // Phase 2 — GCP cost summary (no-op unless BILLING_EXPORT_TABLE
            // is set). Journaled like the snapshot.
            let cost: Option<CostReport> = ctx
                .run(|| async { Ok(Json(cost_phase(|k| std::env::var(k).ok()).await?)) })
                .name("cost")
                .await?
                .into_inner();

            // Phase 3 — promote the day's telemetry Parquet (otel_*) to
            // Iceberg tables, reusing the entity-table writer. Journaled and
            // infallible: a telemetry-lake hiccup never fails the export.
            let iceberg_telemetry: Vec<String> = ctx
                .run(|| async {
                    Ok::<_, HandlerError>(Json(
                        promote_telemetry(summary.run_date, |k| std::env::var(k).ok()).await,
                    ))
                })
                .name("iceberg_telemetry")
                .await?
                .into_inner();

            // Phase 4 — diagnostic email, rendered from the journaled
            // results so a retry re-sends without re-running prior steps.
            let mut report = build_report(&summary, cost.as_ref(), &invocation_id, |k| {
                std::env::var(k).ok()
            });
            report.iceberg_telemetry = iceberg_telemetry;
            let email = render_diagnostic(&report);
            let svc = Arc::clone(&self.email);
            // Discard the send receipt: the diagnostic email's delivery is
            // fire-and-forget, and keeping the durable step's output `()`
            // avoids journaling a `SendReceipt` SendGrid message id we
            // never read.
            ctx.run(move || async move {
                svc.send(email)
                    .await
                    .map(|_| ())
                    .map_err(HandlerError::from)
            })
            .name("email")
            .await?;

            Ok(Json(RunReport {
                run_date: summary.run_date,
                invocation_id,
                tables: summary.entries.len(),
                rows: summary.entries.iter().map(|e| e.rows).sum(),
                failures: summary.failures.len(),
            }))
        }
        .instrument(span)
        .await
    }
}

/// Assemble the [`DiagnosticReport`] from the journaled summary and
/// the environment. Takes a `key -> value` lookup seam so the mapping
/// is unit-testable without mutating process env.
fn build_report<F: Fn(&str) -> Option<String>>(
    summary: &SnapshotSummary,
    cost: Option<&CostReport>,
    invocation_id: &str,
    get: F,
) -> DiagnosticReport {
    let non_empty = |k: &str| get(k).filter(|s| !s.is_empty());
    DiagnosticReport {
        run_date: summary.run_date,
        recipient: non_empty("ARCHIVES_NOTIFY_EMAIL")
            .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string()),
        bucket: non_empty("NAVIGATOR_STORAGE_BUCKET").unwrap_or_else(|| "<unset>".to_string()),
        invocation_id: invocation_id.to_string(),
        restate_console_url: non_empty("RESTATE_CLOUD_CONSOLE_URL"),
        bigquery_project: non_empty("BIGQUERY_PROJECT"),
        bigquery_dataset: non_empty("BIGQUERY_DATASET"),
        snapshots: summary.entries.clone(),
        failures: summary.failures.clone(),
        cost: cost.map(|c| c.rows.clone()).unwrap_or_default(),
        iceberg_telemetry: Vec::new(),
    }
}

/// The `otel_*` tables promoted from the telemetry lake's daily Parquet.
const TELEMETRY_TABLES: &[&str] = &["otel_logs", "otel_traces", "otel_metrics"];

/// Promote the day's telemetry Parquet (`iceberg/otel_*/data/dt=<date>/`) to
/// Iceberg tables, reusing the entity-table writer ([`crate::author_iceberg_for_prefix`]).
///
/// **Infallible by design** — a telemetry-lake hiccup must never fail the
/// nightly export of binding records — so it returns one human-readable line
/// per table for the diagnostic email rather than a `Result`. A clean no-op
/// ("no data") until the collector's OTLP→Parquet shim writes Parquet under
/// these prefixes; in dev/KIND `exports_from_env` is `FsStorage` and lists
/// nothing.
async fn promote_telemetry<F: Fn(&str) -> Option<String>>(
    run_date: NaiveDate,
    get: F,
) -> Vec<String> {
    let storage = match cloud::exports_from_env().await {
        Ok(s) => s,
        Err(e) => {
            return vec![format!(
                "(telemetry promotion skipped — storage unavailable: {e})"
            )]
        }
    };
    let bucket = get("NAVIGATOR_STORAGE_BUCKET")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "exports".to_string());
    let location_base = format!("gs://{bucket}");
    // Stamped once inside the journaled step (so a replay reuses the cached
    // result, not a new clock read).
    let now_ms = chrono::Utc::now().timestamp_millis();
    // 30-day cutoff for the short-retention tables (traces/metrics); their GCS
    // lifecycle deletes data at 30d, so the snapshot log is pruned to match.
    let cutoff_30d_ms = now_ms - 30 * 24 * 60 * 60 * 1000;

    let mut lines = Vec::with_capacity(TELEMETRY_TABLES.len());
    for (i, &table) in TELEMETRY_TABLES.iter().enumerate() {
        let snapshot_id = now_ms.saturating_add(i64::try_from(i).unwrap_or(0));
        // otel_logs keeps its full snapshot log (10-year, content-free);
        // otel_traces / otel_metrics prune to the 30-day lifecycle window.
        let expire_before_ms = (table != "otel_logs").then_some(cutoff_30d_ms);
        match crate::author_iceberg_for_prefix(
            storage.as_ref(),
            table,
            &location_base,
            run_date,
            snapshot_id,
            now_ms,
            expire_before_ms,
        )
        .await
        {
            Ok(Some(authored)) => lines.push(format!("{table} v{}", authored.version)),
            Ok(None) => lines.push(format!("{table} (no data)")),
            Err(e) => lines.push(format!("{table} FAILED: {e}")),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{build_report, SnapshotSummary};
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    fn empty_summary() -> SnapshotSummary {
        SnapshotSummary {
            run_date: NaiveDate::from_ymd_opt(2026, 5, 29).unwrap(),
            entries: Vec::new(),
            failures: Vec::new(),
        }
    }

    #[test]
    fn build_report_defaults_recipient_when_env_unset() {
        let report = build_report(&empty_summary(), None, "inv_1", |_| None);
        assert_eq!(report.recipient, "nick@neonlaw.com");
        assert_eq!(report.invocation_id, "inv_1");
        assert!(report.bigquery_project.is_none());
        assert!(report.restate_console_url.is_none());
        assert!(report.cost.is_empty());
    }

    #[test]
    fn build_report_threads_env_through() {
        let report = build_report(
            &empty_summary(),
            None,
            "inv_2",
            lookup(&[
                ("ARCHIVES_NOTIFY_EMAIL", "ops@example.com"),
                ("NAVIGATOR_STORAGE_BUCKET", "proj-exports"),
                ("BIGQUERY_PROJECT", "proj"),
                ("BIGQUERY_DATASET", "navigator_bi"),
                ("RESTATE_CLOUD_CONSOLE_URL", "https://cloud.restate.dev/a/i"),
            ]),
        );
        assert_eq!(report.recipient, "ops@example.com");
        assert_eq!(report.bucket, "proj-exports");
        assert_eq!(report.bigquery_project.as_deref(), Some("proj"));
        assert_eq!(
            report.restate_console_url.as_deref(),
            Some("https://cloud.restate.dev/a/i")
        );
    }

    #[test]
    fn build_report_treats_empty_env_as_unset() {
        let report = build_report(
            &empty_summary(),
            None,
            "inv_3",
            lookup(&[("BIGQUERY_PROJECT", "")]),
        );
        assert!(report.bigquery_project.is_none());
    }
}
