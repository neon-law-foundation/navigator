//! The `BillingDigest` Restate workflow — a daily internal notification
//! reporting trailing-window GCP cost by service.
//!
//! Two durable steps, each journaled independently (the reason this is a
//! Restate workflow and not a one-shot batch — a retry of the BigQuery step
//! must not re-send the email, and an email-send retry must not re-bill a
//! BigQuery scan):
//!
//! 1. `ctx.run("query")` — read the billing export for the current trailing
//!    window and the prior window (days 31–60) for a per-service trend. The
//!    query plumbing is `billing::gcp_cost` (shared with `archives`).
//! 2. `ctx.run("email")` — render the digest and send it. Reads step 1's
//!    *journaled* report, so a crash between the steps replays the query from
//!    the journal rather than re-scanning BigQuery.
//!
//! **Gating (per the legal-council review — internal financial data, not
//! client data):** the recipient is env-pinned to a firm-internal alias
//! (`BILLING_DIGEST_NOTIFY_EMAIL`), never derived from any client or matter,
//! and the report is firm-wide by service — never broken down per client,
//! matter, or tenant, so it can't hint at any client's volume.
//!
//! **No-op without an export:** `BILLING_EXPORT_TABLE` / `BIGQUERY_PROJECT`
//! unset (KIND / dev / OSS forks) → the workflow logs and returns without
//! sending, so a fork that has no billing export emails nothing rather than an
//! empty shell. A *configured* deploy whose window is empty (lagging export)
//! still sends — with a "no rows" note instead of a misleading $0 table.

use std::sync::Arc;

use billing::gcp_cost::{adc_token_provider, BillingClient, CostRow};
use chrono::{DateTime, Utc};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use workflows::{EmailService, OutboundEmail};

/// Default digest recipient when `BILLING_DIGEST_NOTIFY_EMAIL` is unset.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// Default trailing window in days when `BILLING_DIGEST_WINDOW_DAYS` is unset.
const DEFAULT_WINDOW_DAYS: u32 = 30;

/// Request body for `BillingDigest::run`. Empty — the trigger only starts the
/// workflow — but kept as a struct so fields can be threaded later without
/// changing the handler signature.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// The journaled result of the query step, carried into the email render. A
/// pure value (no clients, no env) so the renderer is unit-testable.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BillingDigestReport {
    /// Trailing window the costs cover, in days.
    pub window_days: u32,
    /// Instant the query step ran (journaled), so the rendered date is stable
    /// on replay rather than re-reading the clock.
    pub as_of: DateTime<Utc>,
    /// Current-window cost by service, highest first.
    pub current: Vec<CostRow>,
    /// Prior-window (days `window..2*window`) cost by service, for the trend.
    pub prior: Vec<CostRow>,
}

impl BillingDigestReport {
    fn cost_total(&self) -> f64 {
        visible_cost_total(&self.current)
    }

    fn prior_cost_total(&self) -> f64 {
        visible_cost_total(&self.prior)
    }

    fn service_count(&self) -> usize {
        self.current
            .iter()
            .filter(|c| is_visible_cost(c.cost))
            .count()
    }
}

#[restate_sdk::workflow]
#[name = "BillingDigest"]
pub trait BillingDigest {
    async fn run(req: Json<RunRequest>) -> Result<Json<DigestOutcome>, HandlerError>;
}

/// Invocation output: whether a digest was sent, and the headline cost figure
/// when it was. `sent == false` is the unconfigured no-op (no export).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DigestOutcome {
    pub sent: bool,
    pub cost_total: f64,
    pub services: usize,
}

/// Service registered with the Restate endpoint. Holds the worker-side
/// [`EmailService`]; the BigQuery client is built from env inside the query
/// step so no token or HTTP client is held idle between runs. Same shape as
/// `BillingCanaryService` and `archives`'s `ArchivesService`.
#[derive(Clone)]
pub struct BillingDigestService {
    email: Arc<dyn EmailService>,
}

impl BillingDigestService {
    #[must_use]
    pub fn new(email: Arc<dyn EmailService>) -> Self {
        Self { email }
    }
}

impl BillingDigest for BillingDigestService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: Json<RunRequest>,
    ) -> Result<Json<DigestOutcome>, HandlerError> {
        let cfg = DigestConfig::from_env(|k| std::env::var(k).ok());

        // No export configured (KIND / dev / OSS fork) → clean no-op: log and
        // return without sending, so a fork with no billing export emails
        // nothing rather than an empty shell.
        let Some(query) = cfg.query.clone() else {
            tracing::info!(
                "BILLING_EXPORT_TABLE / BIGQUERY_PROJECT unset; skipping billing digest (no send)"
            );
            return Ok(Json(DigestOutcome {
                sent: false,
                cost_total: 0.0,
                services: 0,
            }));
        };

        // Step 1 — query the billing export. A missing ADC credential or a
        // BigQuery error surfaces as a retryable HandlerError so Restate
        // replays just this step (without re-sending the email below).
        let window = cfg.window_days;
        let report: BillingDigestReport = ctx
            .run(|| async move {
                let token = adc_token_provider().await?;
                let client = BillingClient::new(query.project, token);
                let current = client
                    .cost_by_service_window(&query.table, window, 0)
                    .await?;
                // Prior period = days [window, 2*window): the window immediately
                // before the current one, for a like-for-like trend.
                let prior = client
                    .cost_by_service_window(&query.table, window * 2, window)
                    .await?;
                Ok(Json(BillingDigestReport {
                    window_days: window,
                    as_of: Utc::now(),
                    current,
                    prior,
                }))
            })
            .name("query")
            .await?
            .into_inner();

        // Step 2 — render + send, journaled separately so a query retry never
        // re-sends and a send retry never re-scans BigQuery.
        let outcome = DigestOutcome {
            sent: true,
            cost_total: report.cost_total(),
            services: report.service_count(),
        };
        let email = build_digest_email(&report, &cfg.recipient);
        let svc = Arc::clone(&self.email);
        ctx.run(move || async move {
            svc.send(email)
                .await
                .map(|_| ())
                .map_err(HandlerError::from)
        })
        .name("email")
        .await?;

        Ok(Json(outcome))
    }
}

/// Resolved configuration for one digest run.
#[derive(Debug, Clone, PartialEq)]
struct DigestConfig {
    recipient: String,
    window_days: u32,
    /// `Some` only when both the export table and project are configured;
    /// `None` is the unconfigured no-op (no send).
    query: Option<QueryConfig>,
}

#[derive(Debug, Clone, PartialEq)]
struct QueryConfig {
    table: String,
    project: String,
}

impl DigestConfig {
    /// Resolve from a `key -> value` lookup (`std::env::var` in production) so
    /// the gating is unit-testable without mutating process env.
    fn from_env<F: Fn(&str) -> Option<String>>(get: F) -> Self {
        let non_empty = |k: &str| get(k).filter(|s| !s.is_empty());
        let recipient = non_empty("BILLING_DIGEST_NOTIFY_EMAIL")
            .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string());
        let window_days = non_empty("BILLING_DIGEST_WINDOW_DAYS")
            .and_then(|s| s.parse().ok())
            .filter(|d| *d > 0)
            .unwrap_or(DEFAULT_WINDOW_DAYS);
        // Both table AND project, or we can't query — treat either-unset as the
        // unconfigured no-op so a half-configured fork doesn't crash-loop.
        let query = match (
            non_empty("BILLING_EXPORT_TABLE"),
            non_empty("BIGQUERY_PROJECT"),
        ) {
            (Some(table), Some(project)) => Some(QueryConfig { table, project }),
            _ => None,
        };
        Self {
            recipient,
            window_days,
            query,
        }
    }
}

fn dollars(v: f64) -> String {
    let cents = rounded_cents(v);
    if cents < 0 {
        format!("-${:.2}", cents.unsigned_abs() as f64 / 100.0)
    } else {
        format!("${:.2}", cents as f64 / 100.0)
    }
}

fn rounded_cents(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

fn is_visible_cost(v: f64) -> bool {
    rounded_cents(v) != 0
}

fn visible_cost_total(rows: &[CostRow]) -> f64 {
    rows.iter()
        .filter(|c| is_visible_cost(c.cost))
        .map(|c| c.cost)
        .sum()
}

/// Build the daily billing-digest email. Pure — exposed so the rendered
/// subject/body is unit-tested without a worker or BigQuery.
#[must_use]
pub fn build_digest_email(report: &BillingDigestReport, recipient: &str) -> OutboundEmail {
    use std::fmt::Write as _;

    let date = report.as_of.format("%Y-%m-%d");
    let total = report.cost_total();
    let subject = format!(
        "💸 {} {}-day GCP cost — {date}",
        dollars(total),
        report.window_days
    );

    let mut out = String::with_capacity(2048);
    let _ = writeln!(
        out,
        "{} {}-day GCP cost as of {date} UTC.",
        dollars(total),
        report.window_days
    );
    let _ = writeln!(
        out,
        "Firm-wide usage by service; the current day may be partial because the billing export lags \
         by roughly 24 hours.\n"
    );

    if report.current.is_empty() {
        // Configured but no rows in the window — say so plainly rather than
        // render a misleading all-zero table (the export may be lagging ~24h).
        out.push_str(
            "No billing rows in the trailing window. The billing export lags ~24h, so this can \
             mean the most recent days haven't landed yet — check the BigQuery export freshness \
             if it persists.\n",
        );
    } else {
        let _ = writeln!(out, "GCP COST BY SERVICE ({} DAYS)\n", report.window_days);
        let _ = writeln!(out, "{:<30}  {:>12}  {:>12}", "Service", "Cost", "vs prior");
        let _ = writeln!(out, "{:-<30}  {:-<12}  {:-<12}", "", "", "");
        let prior_by_service = report
            .prior
            .iter()
            .map(|c| (c.service.as_str(), c.cost))
            .collect::<std::collections::HashMap<_, _>>();
        let display_rows = report.current.iter().filter(|c| is_visible_cost(c.cost));
        for c in display_rows {
            let delta = prior_by_service
                .get(c.service.as_str())
                .map(|prev| c.cost - prev);
            let _ = writeln!(
                out,
                "{:<30}  {:>12}  {:>12}",
                truncate(&c.service, 30),
                dollars(c.cost),
                delta.map_or_else(|| "n/a".to_string(), signed_dollars),
            );
        }
        let total_delta = total - report.prior_cost_total();
        let _ = writeln!(out, "{:-<30}  {:-<12}  {:-<12}", "", "", "");
        let _ = writeln!(
            out,
            "{:<30}  {:>12}  {:>12}",
            "TOTAL",
            dollars(total),
            signed_dollars(total_delta),
        );
        out.push('\n');
    }

    let html = workflows::email::render_email_html(
        &out,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    OutboundEmail::new(recipient.to_string(), subject, out).with_html(html)
}

/// Truncate a service name to `max` chars for the fixed-width table.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn signed_dollars(v: f64) -> String {
    match rounded_cents(v).cmp(&0) {
        std::cmp::Ordering::Greater => format!("+{}", dollars(v)),
        std::cmp::Ordering::Less => dollars(v),
        std::cmp::Ordering::Equal => "$0.00".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_digest_email, dollars, signed_dollars, BillingDigestReport, DigestConfig};
    use billing::gcp_cost::CostRow;
    use chrono::{DateTime, Utc};

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn sample_report() -> BillingDigestReport {
        BillingDigestReport {
            window_days: 30,
            as_of: ts("2026-06-15T13:00:00Z"),
            current: vec![
                CostRow {
                    service: "Kubernetes Engine".into(),
                    cost: 114.44,
                },
                CostRow {
                    service: "Cloud SQL".into(),
                    cost: 19.32,
                },
                CostRow {
                    service: "Artifact Registry".into(),
                    cost: 0.004,
                },
                CostRow {
                    service: "Cloud Trace".into(),
                    cost: 0.004,
                },
                CostRow {
                    service: "Invoice".into(),
                    cost: 0.004,
                },
            ],
            prior: vec![CostRow {
                service: "Kubernetes Engine".into(),
                cost: 100.00,
            }],
        }
    }

    #[test]
    fn digest_starts_with_money_and_renders_30_day_cost_table() {
        let report = sample_report();
        assert_eq!(report.service_count(), 2);
        assert_eq!(dollars(report.cost_total()), "$133.76");

        let email = build_digest_email(&report, "ops@example.com");
        assert_eq!(email.to, "ops@example.com");
        assert!(email.subject.starts_with("💸 $133.76 30-day GCP cost"));

        let b = &email.body;
        assert!(b.starts_with("$133.76 30-day GCP cost"), "body: {b}");
        assert!(b.contains("GCP COST BY SERVICE (30 DAYS)"));
        assert!(b.contains("Service") && b.contains("Cost") && b.contains("vs prior"));
        assert!(b.contains("Kubernetes Engine"));
        assert!(b.contains("Cloud SQL"));
        assert!(!b.contains("Artifact Registry"));
        assert!(!b.contains("Cloud Trace"));
        assert!(!b.contains("Invoice"));
        assert!(b.contains("$114.44"));
        assert!(b.contains("n/a"));
        assert!(b.contains("TOTAL"));
        assert!(b.contains("$133.76"), "gross total missing: {b}");
        assert!(b.contains("+$14.44"), "trend delta: {b}");
        assert!(b.contains("+$33.76"), "total delta: {b}");

        assert!(!b.contains("Credit"));
        assert!(!b.contains("credit"));
        assert!(!b.contains("trial"));
        assert!(!b.contains("PROMOTION"));
        assert!(!b.contains("Console-only"));
        assert!(!b.contains("-$0.00"));

        // Branded HTML retained with the plain-text fallback.
        assert!(email.html_body.is_some());
    }

    #[test]
    fn dollars_and_signed_dollars_round_near_zero_cleanly() {
        assert_eq!(dollars(-0.004), "$0.00");
        assert_eq!(dollars(0.004), "$0.00");
        assert_eq!(signed_dollars(-0.004), "$0.00");
        assert_eq!(signed_dollars(0.004), "$0.00");
        assert_eq!(signed_dollars(0.005), "+$0.01");
        assert_eq!(signed_dollars(-0.005), "-$0.01");
    }

    #[test]
    fn digest_with_no_rows_says_so_instead_of_a_zero_table() {
        let mut report = sample_report();
        report.current = Vec::new();
        let email = build_digest_email(&report, "ops@example.com");
        assert!(email
            .body
            .contains("No billing rows in the trailing window"));
        // No misleading totals table.
        assert!(!email.body.contains("COST BY SERVICE"));
    }

    #[test]
    fn config_skips_query_until_both_table_and_project_are_set() {
        // Nothing set → no query (clean no-op), default recipient + window.
        let none = DigestConfig::from_env(|_| None);
        assert!(none.query.is_none());
        assert_eq!(none.recipient, "nick@neonlaw.com");
        assert_eq!(none.window_days, 30);

        // Table without project is still a no-op (no crash-loop).
        let half =
            DigestConfig::from_env(|k| (k == "BILLING_EXPORT_TABLE").then(|| "p.ds.t".to_string()));
        assert!(half.query.is_none());

        // Both set → query configured; env overrides honored.
        let full = DigestConfig::from_env(|k| match k {
            "BILLING_EXPORT_TABLE" => Some("p.ds.t".into()),
            "BIGQUERY_PROJECT" => Some("test-proj".into()),
            "BILLING_DIGEST_NOTIFY_EMAIL" => Some("billing@neonlaw.com".into()),
            "BILLING_DIGEST_WINDOW_DAYS" => Some("7".into()),
            _ => None,
        });
        let query = full.query.expect("both set → query configured");
        assert_eq!(query.table, "p.ds.t");
        assert_eq!(query.project, "test-proj");
        assert_eq!(full.recipient, "billing@neonlaw.com");
        assert_eq!(full.window_days, 7);
    }
}
