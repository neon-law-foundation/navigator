//! The `BillingDigest` Restate workflow — a daily internal email reporting
//! trailing-window GCP cost by service (gross / credits / net), the
//! free-trial credit burned to date, and an honest "what becomes real cost
//! when the trial credits expire" line.
//!
//! Two durable steps, each journaled independently (the reason this is a
//! Restate workflow and not a one-shot batch — a retry of the BigQuery step
//! must not re-send the email, and an email-send retry must not re-bill a
//! BigQuery scan):
//!
//! 1. `ctx.run("query")` — read the billing export three ways: the current
//!    trailing window split by credit type, the prior window (days 31–60) for
//!    a per-service trend, and the all-time PROMOTION applied to date. The
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

use billing::gcp_cost::{adc_token_provider, BillingClient, ServiceCost};
use chrono::{DateTime, NaiveDate, Utc};
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
    /// Instant the query step ran (journaled), so days-to-expiry is computed
    /// against a stable "now" on replay rather than re-reading the clock.
    pub as_of: DateTime<Utc>,
    /// Current-window cost by service, gross-descending.
    pub current: Vec<ServiceCost>,
    /// Prior-window (days `window..2*window`) cost by service, for the trend.
    pub prior: Vec<ServiceCost>,
    /// All-time PROMOTION (free-trial) credit applied to date — negative in
    /// the export; the renderer takes the magnitude.
    pub promo_applied: f64,
    /// Free-trial promotion expiry date, when configured
    /// (`BILLING_PROMO_EXPIRY`). The export carries no grant ceiling, so the
    /// expiry is operator-supplied, not derived.
    pub promo_expiry: Option<NaiveDate>,
}

impl BillingDigestReport {
    fn gross_total(&self) -> f64 {
        self.current.iter().map(|c| c.gross).sum()
    }

    fn credit_total(&self) -> f64 {
        self.current
            .iter()
            .map(|c| c.promo_credit + c.discount_credit)
            .sum()
    }

    fn net_total(&self) -> f64 {
        self.current.iter().map(|c| c.net).sum()
    }

    fn promo_total(&self) -> f64 {
        self.current.iter().map(|c| c.promo_credit).sum()
    }

    fn discount_total(&self) -> f64 {
        self.current.iter().map(|c| c.discount_credit).sum()
    }

    /// What spend becomes real when the free-trial PROMOTION credit expires:
    /// gross minus only the perpetual free-tier DISCOUNT (the PROMOTION offset
    /// disappears, the DISCOUNT does not). `discount_total` is negative, so
    /// this adds it back to gross.
    fn real_cost_when_promo_expires(&self) -> f64 {
        self.gross_total() + self.discount_total()
    }

    /// Days from `as_of` to the configured promo expiry, when set.
    fn days_to_expiry(&self) -> Option<i64> {
        self.promo_expiry
            .map(|e| (e - self.as_of.date_naive()).num_days())
    }
}

#[restate_sdk::workflow]
#[name = "BillingDigest"]
pub trait BillingDigest {
    async fn run(req: Json<RunRequest>) -> Result<Json<DigestOutcome>, HandlerError>;
}

/// Invocation output: whether a digest was sent, and the headline net figure
/// when it was. `sent == false` is the unconfigured no-op (no export).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DigestOutcome {
    pub sent: bool,
    pub net_total: f64,
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
                net_total: 0.0,
                services: 0,
            }));
        };

        // Step 1 — query the billing export. A missing ADC credential or a
        // BigQuery error surfaces as a retryable HandlerError so Restate
        // replays just this step (without re-sending the email below).
        let window = cfg.window_days;
        let expiry = cfg.promo_expiry;
        let report: BillingDigestReport = ctx
            .run(|| async move {
                let token = adc_token_provider().await?;
                let client = BillingClient::new(query.project, token);
                let current = client.cost_with_credits(&query.table, window, 0).await?;
                // Prior period = days [window, 2*window): the window immediately
                // before the current one, for a like-for-like trend.
                let prior = client
                    .cost_with_credits(&query.table, window * 2, window)
                    .await?;
                let promo_applied = client.promo_applied_to_date(&query.table).await?;
                Ok(Json(BillingDigestReport {
                    window_days: window,
                    as_of: Utc::now(),
                    current,
                    prior,
                    promo_applied,
                    promo_expiry: expiry,
                }))
            })
            .name("query")
            .await?
            .into_inner();

        // Step 2 — render + send, journaled separately so a query retry never
        // re-sends and a send retry never re-scans BigQuery.
        let outcome = DigestOutcome {
            sent: true,
            net_total: report.net_total(),
            services: report.current.len(),
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
    promo_expiry: Option<NaiveDate>,
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
        let promo_expiry = non_empty("BILLING_PROMO_EXPIRY")
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());
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
            promo_expiry,
            query,
        }
    }
}

/// Format a dollar figure with an explicit sign on credits (negative values),
/// so a `-$114.44` credit never reads like a `$-114.44` typo.
fn dollars(v: f64) -> String {
    if v < 0.0 {
        format!("-${:.2}", -v)
    } else {
        format!("${v:.2}")
    }
}

/// Build the daily billing-digest email. Pure — exposed so the rendered
/// subject/body is unit-tested without a worker or BigQuery.
#[must_use]
pub fn build_digest_email(report: &BillingDigestReport, recipient: &str) -> OutboundEmail {
    use std::fmt::Write as _;

    let date = report.as_of.format("%Y-%m-%d");
    let net = report.net_total();
    let gross = report.gross_total();
    let subject = format!(
        "GCP cost — {date}: {} net of {} gross (trailing {}d)",
        dollars(net),
        dollars(gross),
        report.window_days,
    );

    let mut out = String::with_capacity(2048);
    let _ = writeln!(
        out,
        "Trailing {}-day GCP cost as of {date} UTC, firm-wide by service.\n",
        report.window_days
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
        // Service | Gross | Credits | Net, gross-descending, with a totals row.
        out.push_str("---\nCOST BY SERVICE\n\n");
        let _ = writeln!(
            out,
            "  {:30}  {:>12}  {:>12}  {:>12}",
            "Service", "Gross", "Credits", "Net"
        );
        let prior_by_service = report
            .prior
            .iter()
            .map(|c| (c.service.as_str(), c.gross))
            .collect::<std::collections::HashMap<_, _>>();
        for c in &report.current {
            let _ = writeln!(
                out,
                "  {:30}  {:>12}  {:>12}  {:>12}",
                truncate(&c.service, 30),
                dollars(c.gross),
                dollars(c.promo_credit + c.discount_credit),
                dollars(c.net),
            );
            // Trend: gross delta vs the same service in the prior window.
            if let Some(prev) = prior_by_service.get(c.service.as_str()) {
                let delta = c.gross - prev;
                let _ = writeln!(
                    out,
                    "  {:30}    vs prior {}d: {}{}",
                    "",
                    report.window_days,
                    if delta >= 0.0 { "+" } else { "" },
                    dollars(delta),
                );
            }
        }
        let _ = writeln!(
            out,
            "  {:30}  {:>12}  {:>12}  {:>12}",
            "TOTAL",
            dollars(gross),
            dollars(report.credit_total()),
            dollars(net),
        );
        out.push('\n');
    }

    // Credit-burn block — PROMOTION applied to date + expiry + the honesty
    // caveat. Consumption is all the export knows; the granted ceiling (and
    // therefore "remaining") is Console-only, so we never guess it.
    out.push_str("---\nFREE-TRIAL CREDIT (PROMOTION)\n\n");
    let _ = writeln!(
        out,
        "  Applied to date (all-time): {}",
        dollars(report.promo_applied.abs())
    );
    let _ = writeln!(
        out,
        "  This {}-day window:          {}",
        report.window_days,
        dollars(report.promo_total().abs())
    );
    match (report.promo_expiry, report.days_to_expiry()) {
        (Some(expiry), Some(days)) => {
            let _ = writeln!(out, "  Expires: {expiry} ({days} days from now).");
        }
        _ => {
            out.push_str("  Expiry: set BILLING_PROMO_EXPIRY to show days-to-expiry.\n");
        }
    }
    out.push_str(
        "  Exact remaining balance is Console-only: the BigQuery export records credit \
         *consumption*, not the granted ceiling, so \"remaining\" is not derivable here.\n\n",
    );

    // The honest bottom line: what the bill becomes once the trial credit is
    // gone — gross minus only the perpetual free-tier DISCOUNT.
    out.push_str("---\nWHEN THE FREE-TRIAL CREDIT EXPIRES\n\n");
    let _ = writeln!(
        out,
        "  Perpetual free-tier (DISCOUNT, non-expiring): {}",
        dollars(report.discount_total().abs())
    );
    let _ = writeln!(
        out,
        "  Real cost when the trial credit expires:      {}  (gross minus the non-expiring \
         free-tier)",
        dollars(report.real_cost_when_promo_expires())
    );
    out.push_str(
        "\n  Today's net is fully/partly offset by the free-trial PROMOTION credit; the figure \
         above is what the same usage costs once only the perpetual free-tier remains.\n",
    );

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

#[cfg(test)]
mod tests {
    use super::{build_digest_email, BillingDigestReport, DigestConfig};
    use billing::gcp_cost::ServiceCost;
    use chrono::{DateTime, NaiveDate, Utc};

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn sample_report() -> BillingDigestReport {
        BillingDigestReport {
            window_days: 30,
            as_of: ts("2026-06-15T13:00:00Z"),
            current: vec![
                ServiceCost {
                    service: "Kubernetes Engine".into(),
                    gross: 114.44,
                    promo_credit: -114.44,
                    discount_credit: 0.0,
                    net: 0.0,
                },
                ServiceCost {
                    service: "Cloud SQL".into(),
                    gross: 19.32,
                    promo_credit: -15.00,
                    discount_credit: -4.32,
                    net: 0.0,
                },
            ],
            prior: vec![ServiceCost {
                service: "Kubernetes Engine".into(),
                gross: 100.00,
                promo_credit: -100.00,
                discount_credit: 0.0,
                net: 0.0,
            }],
            promo_applied: -182.50,
            promo_expiry: Some(NaiveDate::from_ymd_opt(2026, 8, 23).unwrap()),
        }
    }

    #[test]
    fn digest_renders_table_totals_credit_burn_and_real_cost() {
        let email = build_digest_email(&sample_report(), "ops@example.com");
        assert_eq!(email.to, "ops@example.com");
        // Subject carries net + gross + window.
        assert!(
            email.subject.contains("net of"),
            "subject: {}",
            email.subject
        );
        assert!(email.subject.contains("trailing 30d"));

        let b = &email.body;
        // Table: header + both services + a totals row.
        assert!(b.contains("Gross") && b.contains("Credits") && b.contains("Net"));
        assert!(b.contains("Kubernetes Engine"));
        assert!(b.contains("$114.44")); // gross
        assert!(b.contains("-$114.44")); // credit, explicitly signed
        assert!(b.contains("TOTAL"));
        // Gross total = 114.44 + 19.32 = 133.76.
        assert!(b.contains("$133.76"), "gross total missing: {b}");

        // Credit-burn block: applied-to-date magnitude + expiry + days + caveat.
        assert!(b.contains("Applied to date"));
        assert!(b.contains("$182.50"));
        assert!(b.contains("2026-08-23"));
        assert!(b.contains("69 days from now"), "days-to-expiry: {b}");
        assert!(b.contains("Console-only"));

        // "When the free-trial credit expires" — real cost = gross + discount
        // total = 133.76 + (-4.32) = 129.44; free-tier shown separately.
        assert!(b.contains("Real cost when the trial credit expires"));
        assert!(b.contains("$129.44"), "real-cost figure: {b}");
        assert!(b.contains("Perpetual free-tier"));
        assert!(b.contains("$4.32"));

        // Trend line for the service present in both windows: 114.44 - 100 = +14.44.
        assert!(b.contains("+$14.44"), "trend delta: {b}");

        // Branded HTML retained with the plain-text fallback.
        assert!(email.html_body.is_some());
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
    fn digest_without_configured_expiry_prompts_for_the_env_var() {
        let mut report = sample_report();
        report.promo_expiry = None;
        let email = build_digest_email(&report, "ops@example.com");
        assert!(email.body.contains("set BILLING_PROMO_EXPIRY"));
        assert!(!email.body.contains("days from now"));
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
            "BILLING_PROMO_EXPIRY" => Some("2026-08-23".into()),
            _ => None,
        });
        let query = full.query.expect("both set → query configured");
        assert_eq!(query.table, "p.ds.t");
        assert_eq!(query.project, "test-proj");
        assert_eq!(full.recipient, "billing@neonlaw.com");
        assert_eq!(full.window_days, 7);
        assert_eq!(
            full.promo_expiry,
            Some(NaiveDate::from_ymd_opt(2026, 8, 23).unwrap())
        );
    }
}
