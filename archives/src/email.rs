//! Diagnostic email renderer for the nightly `archives` workflow.
//!
//! Builds a plain-text [`OutboundEmail`] from a [`DiagnosticReport`].
//! Body sections, in order: header (run date + Restate invocation
//! lookup) → `BigQuery` query template → Iceberg snapshot keys →
//! snapshot failures (only when present) → drift findings (only when
//! present) → footer. Cancer's call on the council: lead with the
//! query a 2 a.m. reader can paste before anything else.
//!
//! The header carries the Restate invocation id (and, when
//! `RESTATE_CLOUD_CONSOLE_URL` is set, a deep link) so the operator
//! can pull the run up in Restate Cloud and replay a failed step.

use std::fmt::Write as _;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use workflows::OutboundEmail;

use billing::gcp_cost::CostRow;

use crate::drift::DriftDecision;
use crate::runner::TableFailure;

/// One table's snapshot outcome, as the email needs it. Serializable
/// because it is part of the journaled snapshot-phase output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub table: String,
    pub rows: usize,
    pub bytes: usize,
    pub key: String,
    pub drift: DriftDecision,
}

/// The data a completed workflow run hands to [`render_diagnostic`].
#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    pub run_date: NaiveDate,
    pub recipient: String,
    pub bucket: String,
    /// Restate invocation id for this run — the journal key the
    /// operator searches on in Restate Cloud.
    pub invocation_id: String,
    /// Optional Restate Cloud console base URL; when set, the email
    /// renders a deep link to the invocation.
    pub restate_console_url: Option<String>,
    pub bigquery_project: Option<String>,
    pub bigquery_dataset: Option<String>,
    pub snapshots: Vec<SnapshotEntry>,
    pub failures: Vec<TableFailure>,
    /// GCP cost-by-service for the trailing window. Empty when the
    /// cost phase did not run (`BILLING_EXPORT_TABLE` unset).
    pub cost: Vec<CostRow>,
    /// Pre-rendered lines for the telemetry-Iceberg promotion step, one per
    /// `otel_*` table (e.g. `otel_logs v3`, `otel_traces (no data)`,
    /// `otel_metrics FAILED: …`). Empty when the step authored nothing.
    pub iceberg_telemetry: Vec<String>,
}

impl DiagnosticReport {
    fn total_rows(&self) -> usize {
        self.snapshots.iter().map(|s| s.rows).sum()
    }

    fn total_bytes(&self) -> usize {
        self.snapshots.iter().map(|s| s.bytes).sum()
    }

    fn drifted(&self) -> Vec<&SnapshotEntry> {
        self.snapshots
            .iter()
            .filter(|s| matches!(s.drift, DriftDecision::Added(_)))
            .collect()
    }
}

/// Build the `OutboundEmail` for a completed nightly run.
#[must_use]
pub fn render_diagnostic(report: &DiagnosticReport) -> OutboundEmail {
    let subject = format!(
        "[archives] Nightly export — {} ({} tables, {})",
        report.run_date,
        report.snapshots.len(),
        format_bytes(report.total_bytes())
    );
    let body = render_body(report);
    // Carry the Neon Law logo: wrap the same body in the firm-branded
    // HTML layout (the nightly export is a firm-internal diagnostic).
    let html = workflows::email::render_email_html(
        &body,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    OutboundEmail::new(report.recipient.clone(), subject, body).with_html(html)
}

fn render_body(report: &DiagnosticReport) -> String {
    let mut out = String::with_capacity(4096);

    let _ = writeln!(out, "Run date: {}", report.run_date);
    let _ = writeln!(out, "Tables snapshotted: {}", report.snapshots.len());
    let _ = writeln!(out, "Rows total: {}", report.total_rows());
    let _ = writeln!(out, "Bytes total: {}", format_bytes(report.total_bytes()));
    let _ = writeln!(out, "Restate invocation: {}", report.invocation_id);
    if let Some(base) = &report.restate_console_url {
        let _ = writeln!(
            out,
            "  {}/{}",
            base.trim_end_matches('/'),
            report.invocation_id
        );
    }
    out.push('\n');

    out.push_str("---\n");
    out.push_str("BIGQUERY\n\n");
    if let (Some(project), Some(dataset)) = (&report.bigquery_project, &report.bigquery_dataset) {
        let _ = writeln!(out, "  Dataset: `{project}.{dataset}`\n");
        out.push_str("  Paste-ready query (most recent rows from documents):\n\n");
        let _ = writeln!(out, "    SELECT * FROM `{project}.{dataset}.documents`");
        out.push_str("    ORDER BY inserted_at DESC\n");
        out.push_str("    LIMIT 100;\n");
    } else {
        out.push_str(
            "  (BIGQUERY_PROJECT / BIGQUERY_DATASET unset; query template omitted.\n   \
             External tables read the Parquet files directly — no refresh needed.)\n",
        );
    }
    out.push('\n');

    if !report.cost.is_empty() {
        out.push_str("---\n");
        out.push_str("GCP COST (trailing window, by service)\n\n");
        for row in &report.cost {
            let _ = writeln!(out, "  {:32}  ${:>10.2}", row.service, row.cost);
        }
        let total: f64 = report.cost.iter().map(|r| r.cost).sum();
        let _ = writeln!(out, "  {:32}  ${:>10.2}", "TOTAL", total);
        out.push('\n');
    }

    out.push_str("---\n");
    let _ = writeln!(out, "ICEBERG SNAPSHOTS  (bucket: gs://{})\n", report.bucket);
    for entry in &report.snapshots {
        let _ = writeln!(
            out,
            "  {:24}  {:>7} rows  {:>10}  {}",
            entry.table,
            entry.rows,
            format_bytes(entry.bytes),
            format_drift(&entry.drift),
        );
        let _ = writeln!(out, "    gs://{}/{}", report.bucket, entry.key);
    }
    out.push('\n');

    if !report.iceberg_telemetry.is_empty() {
        out.push_str("---\n");
        out.push_str("ICEBERG TELEMETRY  (otel_* tables promoted over the day's Parquet)\n\n");
        for line in &report.iceberg_telemetry {
            let _ = writeln!(out, "  {line}");
        }
        out.push('\n');
    }

    if !report.failures.is_empty() {
        out.push_str("---\n");
        out.push_str("SNAPSHOT FAILURES\n\n");
        for f in &report.failures {
            let _ = writeln!(out, "  {:24}  {}", f.table, f.error);
        }
        out.push('\n');
    }

    let drifted = report.drifted();
    if !drifted.is_empty() {
        out.push_str("---\n");
        out.push_str("DRIFT (columns added since last run)\n\n");
        for entry in drifted {
            if let DriftDecision::Added(cols) = &entry.drift {
                let _ = writeln!(out, "  {}: {:?}", entry.table, cols);
            }
        }
        out.push('\n');
    }

    out.push_str("---\n");
    out.push_str(
        "This email is the final durable step of the `Archives` Restate workflow,\n\
         fired nightly by the `archives-trigger` CronJob in the navigator namespace.\n\
         Source: archives/src/workflow.rs. Search the invocation id above in Restate\n\
         Cloud to inspect or replay any step.\n",
    );

    out
}

fn format_bytes(n: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    const GB: usize = MB * 1024;
    if n >= GB {
        #[allow(clippy::cast_precision_loss)]
        return format!("{:.1} GB", n as f64 / GB as f64);
    }
    if n >= MB {
        #[allow(clippy::cast_precision_loss)]
        return format!("{:.1} MB", n as f64 / MB as f64);
    }
    if n >= KB {
        #[allow(clippy::cast_precision_loss)]
        return format!("{:.1} KB", n as f64 / KB as f64);
    }
    format!("{n} B")
}

fn format_drift(d: &DriftDecision) -> &'static str {
    match d {
        DriftDecision::Unchanged => "Unchanged",
        DriftDecision::Added(_) => "Added",
    }
}

#[cfg(test)]
mod tests {
    use super::{render_diagnostic, DiagnosticReport, SnapshotEntry};
    use crate::drift::DriftDecision;
    use crate::runner::TableFailure;
    use billing::gcp_cost::CostRow;
    use chrono::NaiveDate;

    fn sample(snapshots: Vec<SnapshotEntry>, failures: Vec<TableFailure>) -> DiagnosticReport {
        DiagnosticReport {
            run_date: NaiveDate::from_ymd_opt(2026, 5, 28).unwrap(),
            recipient: "nick@neonlaw.com".into(),
            bucket: "YOUR_PROJECT_ID-exports".into(),
            invocation_id: "inv_01HYWABCDEF".into(),
            restate_console_url: None,
            bigquery_project: Some("YOUR_PROJECT_ID".into()),
            bigquery_dataset: Some("navigator_bi".into()),
            snapshots,
            failures,
            cost: Vec::new(),
            iceberg_telemetry: Vec::new(),
        }
    }

    fn snap(table: &str, rows: usize, bytes: usize, drift: DriftDecision) -> SnapshotEntry {
        SnapshotEntry {
            table: table.into(),
            rows,
            bytes,
            key: format!("iceberg/{table}/data/2026-05-28/part-abc.parquet"),
            drift,
        }
    }

    #[test]
    fn subject_carries_run_date_and_table_count() {
        let r = sample(
            vec![snap("persons", 10, 1024, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(email.subject.contains("2026-05-28"));
        assert!(email.subject.contains("1 tables"));
    }

    #[test]
    fn recipient_is_carried_through() {
        let r = sample(
            vec![snap("persons", 10, 1024, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert_eq!(email.to, "nick@neonlaw.com");
    }

    #[test]
    fn body_lists_every_table_in_snapshots() {
        let r = sample(
            vec![
                snap("persons", 312, 8_400, DriftDecision::Unchanged),
                snap("documents", 1_204, 42_100, DriftDecision::Unchanged),
            ],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(email.body.contains("persons"));
        assert!(email.body.contains("documents"));
    }

    #[test]
    fn body_carries_restate_invocation_id() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(email.body.contains("Restate invocation: inv_01HYWABCDEF"));
    }

    #[test]
    fn body_renders_console_deep_link_when_url_set() {
        let mut r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        r.restate_console_url = Some("https://cloud.restate.dev/acct/invocations".into());
        let email = render_diagnostic(&r);
        assert!(email
            .body
            .contains("https://cloud.restate.dev/acct/invocations/inv_01HYWABCDEF"));
    }

    #[test]
    fn body_omits_console_link_when_url_unset() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(!email.body.contains("https://"));
    }

    #[test]
    fn body_includes_paste_ready_bigquery_query() {
        let r = sample(
            vec![snap("documents", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(
            email
                .body
                .contains("`YOUR_PROJECT_ID.navigator_bi.documents`"),
            "body should contain a fully-qualified BQ table reference"
        );
        assert!(email.body.contains("SELECT"));
        assert!(email.body.contains("LIMIT 100"));
    }

    #[test]
    fn body_includes_iceberg_object_keys() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(email
            .body
            .contains("gs://YOUR_PROJECT_ID-exports/iceberg/persons/data/"));
    }

    #[test]
    fn body_omits_drift_section_when_all_unchanged() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(
            !email.body.contains("DRIFT"),
            "drift section should be hidden when no table reports Added"
        );
    }

    #[test]
    fn body_includes_drift_section_when_columns_added() {
        let r = sample(
            vec![snap(
                "persons",
                1,
                1,
                DriftDecision::Added(vec!["nickname".into()]),
            )],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(email.body.contains("DRIFT"));
        assert!(email.body.contains("nickname"));
    }

    #[test]
    fn body_omits_cost_section_when_no_cost_rows() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(!email.body.contains("GCP COST"));
    }

    #[test]
    fn body_renders_cost_section_with_total_when_present() {
        let mut r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        r.cost = vec![
            CostRow {
                service: "Compute Engine".into(),
                cost: 31.42,
            },
            CostRow {
                service: "Cloud SQL".into(),
                cost: 12.0,
            },
        ];
        let email = render_diagnostic(&r);
        assert!(email.body.contains("GCP COST"));
        assert!(email.body.contains("Compute Engine"));
        assert!(email.body.contains("31.42"));
        assert!(email.body.contains("TOTAL"));
        assert!(email.body.contains("43.42"));
    }

    #[test]
    fn body_omits_failures_section_when_none() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![],
        );
        let email = render_diagnostic(&r);
        assert!(!email.body.contains("SNAPSHOT FAILURES"));
    }

    #[test]
    fn body_lists_snapshot_failures_when_present() {
        let r = sample(
            vec![snap("persons", 1, 1, DriftDecision::Unchanged)],
            vec![TableFailure {
                table: "documents".into(),
                error: "connection reset".into(),
            }],
        );
        let email = render_diagnostic(&r);
        assert!(email.body.contains("SNAPSHOT FAILURES"));
        assert!(email.body.contains("documents"));
        assert!(email.body.contains("connection reset"));
    }
}
