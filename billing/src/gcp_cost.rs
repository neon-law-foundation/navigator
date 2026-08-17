//! `BigQuery` query client for the GCP **billing export**, plus the
//! Application Default Credentials token provider the cost readers use.
//!
//! This lives in the `billing` crate (not `archives`) so both the nightly
//! `Archives` workflow and the daily `BillingDigest` workflow can reach the
//! same query plumbing: `billing-workflows` already depends on `billing` but
//! not `archives`, and depending on `archives` from `billing-workflows` would
//! be backwards layering. `archives` re-exports these types so its own
//! `cost_phase` keeps the same call sites.
//!
//! The nightly `Archives` workflow optionally summarizes the trailing window
//! of GCP spend by service (read from the billing-export `BigQuery` table) so
//! the diagnostic email and the export lake carry a cost snapshot beside the
//! data snapshot. Env-gated on `BILLING_EXPORT_TABLE`: unset (KIND / dev / OSS
//! forks) → the cost step is a clean no-op and needs no `BigQuery`
//! credentials.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Supplies a bearer token for the `BigQuery` REST API.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    async fn token(&self) -> Result<String>;
}

/// Always returns the same string. Test-only.
pub struct StaticToken(pub String);

#[async_trait]
impl TokenProvider for StaticToken {
    async fn token(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}

/// One service's trailing-window cost. Serializable so the cost step
/// can journal it, render it in the email, and snapshot it to Parquet
/// through the same `batch_from_rows` path the data tables use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostRow {
    pub service: String,
    pub cost: f64,
}

/// One GCP **project**'s trailing-window cost. The billing export is written
/// per billing account and carries every project linked to it, so a digest that
/// only groups by service reports an all-project total without ever naming
/// which project spent it. These rows are that split.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectCost {
    pub project: String,
    pub cost: f64,
}

/// Outcome of the cost phase: the rows, plus the object key the
/// Parquet snapshot was written to (when non-empty).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostReport {
    pub rows: Vec<CostRow>,
    pub key: Option<String>,
}

impl CostReport {
    #[must_use]
    pub fn total(&self) -> f64 {
        self.rows.iter().map(|r| r.cost).sum()
    }
}

/// Minimal `BigQuery` client that runs one `SELECT` against the billing
/// export via the synchronous `jobs.query` API.
pub struct BillingClient {
    project: String,
    token: Arc<dyn TokenProvider>,
    http: reqwest::Client,
    base_url: String,
}

impl BillingClient {
    pub fn new(project: impl Into<String>, token: Arc<dyn TokenProvider>) -> Self {
        Self {
            project: project.into(),
            token,
            http: reqwest::Client::new(),
            base_url: "https://bigquery.googleapis.com".into(),
        }
    }

    /// Override the API base URL — used by tests.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Trailing-`days` cost by service, highest first.
    pub async fn cost_by_service(&self, table: &str, days: u32) -> Result<Vec<CostRow>> {
        let json = self
            .run_query(&format_cost_window_sql(table, days, 0))
            .await?;
        parse_cost_rows(&json)
    }

    /// Cost by service over the `[now-older_days, now-newer_days)` window,
    /// highest first. `newer_days == 0` is the trailing window up to now.
    pub async fn cost_by_service_window(
        &self,
        table: &str,
        older_days: u32,
        newer_days: u32,
    ) -> Result<Vec<CostRow>> {
        let json = self
            .run_query(&format_cost_window_sql(table, older_days, newer_days))
            .await?;
        parse_cost_rows(&json)
    }

    /// Cost by **project** over the `[now-older_days, now-newer_days)` window,
    /// highest first. The export covers every project linked to the billing
    /// account, so this is the per-project split of the same spend
    /// [`Self::cost_by_service_window`] aggregates by service.
    pub async fn cost_by_project_window(
        &self,
        table: &str,
        older_days: u32,
        newer_days: u32,
    ) -> Result<Vec<ProjectCost>> {
        let json = self
            .run_query(&format_cost_by_project_window_sql(
                table, older_days, newer_days,
            ))
            .await?;
        parse_project_costs(&json)
    }

    /// Credit-split cost by service over the `[now-older_days, now-newer_days)`
    /// window, gross-descending. `newer_days == 0` is the trailing window up to
    /// now; a positive `newer_days` bounds both ends for the prior-period trend
    /// query (days 31–60 → `older=60, newer=30`).
    pub async fn cost_with_credits(
        &self,
        table: &str,
        older_days: u32,
        newer_days: u32,
    ) -> Result<Vec<ServiceCost>> {
        let json = self
            .run_query(&format_cost_with_credits_sql(table, older_days, newer_days))
            .await?;
        parse_service_costs(&json)
    }

    /// All-time PROMOTION (free-trial) credit applied to date — a single
    /// scalar. Magnitude only is meaningful (credits are negative).
    pub async fn promo_applied_to_date(&self, table: &str) -> Result<f64> {
        let json = self.run_query(&format_promo_applied_sql(table)).await?;
        Ok(parse_scalar_amount(&json))
    }

    /// Run one `jobs.query` `SELECT` and return the parsed JSON response.
    /// Shared by every cost reader so the auth, error, and `jobComplete`
    /// handling live in one place.
    pub(crate) async fn run_query(&self, query: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "query": query, "useLegacySql": false });
        let token = self
            .token
            .token()
            .await
            .context("acquire BigQuery access token")?;
        let url = format!(
            "{}/bigquery/v2/projects/{}/queries",
            self.base_url, self.project
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .with_context(|| format!("read response body from {url}"))?;
        if !status.is_success() {
            bail!("BigQuery cost query returned {status}: {text}");
        }
        let json: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("parse BigQuery response: {text}"))?;
        if json.get("jobComplete").and_then(serde_json::Value::as_bool) != Some(true) {
            bail!("BigQuery returned jobComplete=false for the cost query");
        }
        Ok(json)
    }
}

/// Build the cost-by-service SQL. Pulled out so a unit test can pin
/// the exact form without a live `BigQuery`.
#[must_use]
pub fn format_cost_sql(table: &str, days: u32) -> String {
    format_cost_window_sql(table, days, 0)
}

/// Build the cost-by-service SQL for `[now-older_days, now-newer_days)`.
/// `newer_days == 0` leaves the recent end open for the trailing window.
#[must_use]
pub fn format_cost_window_sql(table: &str, older_days: u32, newer_days: u32) -> String {
    format!(
        "SELECT service.description AS service, ROUND(SUM(cost), 2) AS cost \
         FROM `{table}` \
         WHERE {} \
         GROUP BY service ORDER BY cost DESC",
        partition_window(older_days, newer_days)
    )
}

/// Label for spend the export records against no project — account-level
/// charges, taxes, and adjustments. Without it those rows would collapse into a
/// single unnamed bucket and the per-project table would not sum to the total.
pub const UNATTRIBUTED_PROJECT: &str = "(unattributed)";

/// Split a `BILLING_EXPORT_TABLE` value into one table per **billing account**.
///
/// Cloud Billing writes one export table per billing account, so a deployment
/// that spans several accounts cannot be described by a single table name. The
/// env var is therefore a comma-separated list; a single table (the common
/// case) parses to a one-element list and needs no separator. Blank entries and
/// surrounding whitespace are dropped so a trailing comma is harmless.
#[must_use]
pub fn billing_export_tables(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Recover the billing-account id from an export table name.
///
/// Cloud Billing names the table `gcp_billing_export_<kind>_v1_<ACCOUNT>` with
/// the account's hyphens rewritten as underscores, so
/// `…gcp_billing_export_resource_v1_013469_2BBE03_532C72` identifies account
/// `013469-2BBE03-532C72`. Deriving the label means a deploy naming its tables
/// never has to also name its accounts and cannot mislabel one. A table that
/// doesn't match the convention falls back to its own name — wrong-looking in
/// the digest, but never silently attributed to the wrong account.
#[must_use]
pub fn billing_account_from_table(table: &str) -> String {
    let leaf = table.rsplit('.').next().unwrap_or(table);
    leaf.rsplit_once("_v1_")
        .map(|(_, account)| account.replace('_', "-"))
        .filter(|account| !account.is_empty())
        .unwrap_or_else(|| table.to_string())
}

/// Build the cost-by-**project** SQL for `[now-older_days, now-newer_days)`,
/// the per-project sibling of [`format_cost_window_sql`] over the same rows and
/// the same window.
#[must_use]
pub fn format_cost_by_project_window_sql(table: &str, older_days: u32, newer_days: u32) -> String {
    format!(
        "SELECT IFNULL(project.id, '{UNATTRIBUTED_PROJECT}') AS project, \
         ROUND(SUM(cost), 2) AS cost \
         FROM `{table}` \
         WHERE {} \
         GROUP BY project ORDER BY cost DESC",
        partition_window(older_days, newer_days)
    )
}

/// The `_PARTITIONTIME` predicate for `[now-older_days, now-newer_days)`.
/// `newer_days == 0` leaves the recent end open (the trailing window up to
/// now); a positive `newer_days` bounds both ends, which is how the prior
/// period (days 31–60 → `older=60, newer=30`) is expressed for trend. Shared by
/// every cost query so the by-service, by-project, and credit-split reads can
/// never drift onto different windows.
fn partition_window(older_days: u32, newer_days: u32) -> String {
    let mut window =
        format!("_PARTITIONTIME >= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL {older_days} DAY)");
    if newer_days > 0 {
        window.push_str(&format!(
            " AND _PARTITIONTIME < TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL {newer_days} DAY)"
        ));
    }
    window
}

/// Parse a `jobs.query` response body into cost rows. The response
/// shape is `{ "rows": [ { "f": [ {"v": "<service>"}, {"v": "<cost>"} ] } ] }`.
pub fn parse_cost_rows(resp: &serde_json::Value) -> Result<Vec<CostRow>> {
    Ok(parse_label_amount_rows(resp)?
        .into_iter()
        .map(|(service, cost)| CostRow { service, cost })
        .collect())
}

/// Parse a `jobs.query` response body into per-project cost rows. Same
/// two-cell `[label, amount]` shape as [`parse_cost_rows`], keyed by project.
pub fn parse_project_costs(resp: &serde_json::Value) -> Result<Vec<ProjectCost>> {
    Ok(parse_label_amount_rows(resp)?
        .into_iter()
        .map(|(project, cost)| ProjectCost { project, cost })
        .collect())
}

/// Parse the shared `{ "rows": [ { "f": [ {"v": "<label>"}, {"v": "<amount>"}
/// ] } ] }` shape every grouped cost query returns. A missing `rows` field is
/// an empty window, not an error; an unparseable amount is 0.
fn parse_label_amount_rows(resp: &serde_json::Value) -> Result<Vec<(String, f64)>> {
    let Some(rows) = resp.get("rows").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new()); // no rows field → empty window
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let cells = row
            .get("f")
            .and_then(serde_json::Value::as_array)
            .context("billing row missing `f` cell array")?;
        let label = cell_str(cells, 0).unwrap_or("(unknown)").to_string();
        let amount = cell_str(cells, 1)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        out.push((label, amount));
    }
    Ok(out)
}

/// One service's trailing-window cost split into gross spend and the
/// credits applied against it, by credit *type*.
///
/// The GCP billing export records credits as a repeated field per usage
/// row; only **PROMOTION** credits are the consumable free-trial balance
/// that disappears when the trial ends, while **DISCOUNT** credits are the
/// perpetual free-tier offset. Splitting them is what lets the digest show
/// an honest "what becomes real cost when the trial credits expire" figure
/// (gross minus the non-expiring DISCOUNT), rather than the fully-offset
/// `net` that hides the looming bill. Credits are negative in the export, so
/// `net = gross + promo_credit + discount_credit`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceCost {
    pub service: String,
    /// Gross spend before any credit (`SUM(cost)`), always ≥ 0.
    pub gross: f64,
    /// PROMOTION (free-trial) credit applied — negative, disappears when the
    /// trial ends.
    pub promo_credit: f64,
    /// DISCOUNT (free-tier) credit applied — negative, perpetual.
    pub discount_credit: f64,
    /// Net cost after every credit (`gross + all credits`).
    pub net: f64,
}

/// Build the credit-aware cost-by-service SQL over a `_PARTITIONTIME`
/// window `[now-older_days, now-newer_days)`. `newer_days == 0` leaves the
/// window open at the recent end (the trailing-`older_days` window up to
/// now); a positive `newer_days` bounds it on both sides, which is how the
/// prior-period window (e.g. days 31–60: `older=60, newer=30`) is expressed
/// for trend.
///
/// Per service: gross `SUM(cost)`, plus the credits unnested and summed
/// split by `credits.type` (PROMOTION vs DISCOUNT), plus `net` = gross + all
/// credits. `IFNULL(..., 0)` so a usage row with no credits contributes 0,
/// never NULL. Pinned by a unit test so the shape can't drift without notice.
#[must_use]
pub fn format_cost_with_credits_sql(table: &str, older_days: u32, newer_days: u32) -> String {
    let mut window =
        format!("_PARTITIONTIME >= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL {older_days} DAY)");
    if newer_days > 0 {
        window.push_str(&format!(
            " AND _PARTITIONTIME < TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL {newer_days} DAY)"
        ));
    }
    format!(
        "SELECT service.description AS service, \
         ROUND(SUM(cost), 2) AS gross, \
         ROUND(SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c \
         WHERE c.type = 'PROMOTION')), 2) AS promo_credit, \
         ROUND(SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c \
         WHERE c.type = 'DISCOUNT')), 2) AS discount_credit, \
         ROUND(SUM(cost) + SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c)), 2) AS net \
         FROM `{table}` \
         WHERE {window} \
         GROUP BY service ORDER BY gross DESC"
    )
}

/// Build the all-time **PROMOTION-applied-to-date** SQL: one scalar, the sum
/// of every PROMOTION credit ever recorded in the export (no partition
/// filter). This is *consumption* to date, not the granted ceiling — the
/// export carries no grant total, so "remaining" is Console-only and the
/// digest says so rather than guessing. Credits are negative, so the result
/// is ≤ 0; the renderer takes the magnitude.
#[must_use]
pub fn format_promo_applied_sql(table: &str) -> String {
    format!(
        "SELECT ROUND(SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c \
         WHERE c.type = 'PROMOTION')), 2) AS promo_applied \
         FROM `{table}`"
    )
}

/// Parse a `jobs.query` response into credit-split service costs. Cells are
/// in select order: `[service, gross, promo_credit, discount_credit, net]`.
/// A missing `rows` field → empty window; an unparseable numeric cell → 0.
pub fn parse_service_costs(resp: &serde_json::Value) -> Result<Vec<ServiceCost>> {
    let Some(rows) = resp.get("rows").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let cells = row
            .get("f")
            .and_then(serde_json::Value::as_array)
            .context("billing row missing `f` cell array")?;
        let cell_f64 = |i: usize| cell_str(cells, i).and_then(|s| s.parse::<f64>().ok());
        out.push(ServiceCost {
            service: cell_str(cells, 0).unwrap_or("(unknown)").to_string(),
            gross: cell_f64(1).unwrap_or(0.0),
            promo_credit: cell_f64(2).unwrap_or(0.0),
            discount_credit: cell_f64(3).unwrap_or(0.0),
            net: cell_f64(4).unwrap_or(0.0),
        });
    }
    Ok(out)
}

/// Parse a single-cell scalar (e.g. the promo-applied total) from a
/// `jobs.query` response. No rows (empty export) → 0.0.
pub fn parse_scalar_amount(resp: &serde_json::Value) -> f64 {
    resp.get("rows")
        .and_then(serde_json::Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("f"))
        .and_then(serde_json::Value::as_array)
        .and_then(|cells| cell_str(cells, 0))
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Read the `v` string of the `i`th cell in a `jobs.query` `f` array.
fn cell_str(cells: &[serde_json::Value], i: usize) -> Option<&str> {
    cells
        .get(i)
        .and_then(|c| c.get("v"))
        .and_then(serde_json::Value::as_str)
}

/// Application Default Credentials → Workload Identity in production.
/// Honors `BILLING_FAKE_TOKEN` (or the legacy `ARCHIVES_FAKE_TOKEN`) for
/// tests / local runs without ADC.
pub async fn adc_token_provider() -> Result<Arc<dyn TokenProvider>> {
    if std::env::var_os("BILLING_FAKE_TOKEN").is_some()
        || std::env::var_os("ARCHIVES_FAKE_TOKEN").is_some()
    {
        return Ok(Arc::new(StaticToken("unused".into())));
    }
    Ok(Arc::new(AdcToken::new().await?))
}

struct AdcToken {
    source: Arc<dyn google_cloud_token::TokenSource>,
}

impl AdcToken {
    async fn new() -> Result<Self> {
        let scopes: [&str; 1] = ["https://www.googleapis.com/auth/bigquery"];
        let config = google_cloud_auth::project::Config::default().with_scopes(&scopes);
        let provider = google_cloud_auth::token::DefaultTokenSourceProvider::new(config)
            .await
            .context("acquire Application Default Credentials for BigQuery")?;
        Ok(Self {
            source: google_cloud_token::TokenSourceProvider::token_source(&provider),
        })
    }
}

#[async_trait]
impl TokenProvider for AdcToken {
    async fn token(&self) -> Result<String> {
        let raw = self
            .source
            .token()
            .await
            .map_err(|e| anyhow::anyhow!("ADC token: {e}"))?;
        Ok(raw.strip_prefix("Bearer ").unwrap_or(&raw).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_cost_by_project_window_sql, format_cost_sql, format_cost_window_sql,
        format_cost_with_credits_sql, format_promo_applied_sql, parse_cost_rows,
        parse_project_costs, parse_scalar_amount, parse_service_costs, CostRow, ProjectCost,
        ServiceCost,
    };
    use serde_json::json;

    #[test]
    fn cost_sql_aggregates_by_service_over_the_window() {
        let sql = format_cost_sql("proj.billing_export.gcp_billing_export_v1_X", 30);
        assert!(sql.contains("SUM(cost)"));
        assert!(sql.contains("INTERVAL 30 DAY"));
        assert!(sql.contains("`proj.billing_export.gcp_billing_export_v1_X`"));
        assert!(sql.contains("GROUP BY service"));
    }

    #[test]
    fn cost_window_sql_bounds_prior_period_when_newer_days_is_set() {
        let sql = format_cost_window_sql("p.ds.t", 60, 30);
        assert!(sql.contains("SUM(cost)"));
        assert!(sql.contains(">= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 60 DAY)"));
        assert!(sql.contains("< TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY)"));
        assert!(sql.contains("GROUP BY service ORDER BY cost DESC"));
    }

    #[test]
    fn parse_extracts_service_and_cost_pairs() {
        let resp = json!({
            "jobComplete": true,
            "rows": [
                { "f": [ {"v": "Compute Engine"}, {"v": "31.42"} ] },
                { "f": [ {"v": "Cloud SQL"},      {"v": "12.00"} ] }
            ]
        });
        let rows = parse_cost_rows(&resp).unwrap();
        assert_eq!(
            rows,
            vec![
                CostRow {
                    service: "Compute Engine".into(),
                    cost: 31.42
                },
                CostRow {
                    service: "Cloud SQL".into(),
                    cost: 12.0
                },
            ]
        );
    }

    #[test]
    fn parse_treats_missing_rows_as_empty() {
        let rows = parse_cost_rows(&json!({ "jobComplete": true })).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn parse_defaults_unparseable_cost_to_zero() {
        let resp = json!({ "rows": [ { "f": [ {"v": "Weird"}, {"v": "n/a"} ] } ] });
        let rows = parse_cost_rows(&resp).unwrap();
        assert!(rows[0].cost.abs() < f64::EPSILON);
    }

    #[test]
    fn project_sql_groups_by_project_over_the_same_window_as_the_service_query() {
        let sql = format_cost_by_project_window_sql("p.ds.t", 30, 0);
        assert!(sql.contains("SUM(cost)"));
        assert!(sql.contains("GROUP BY project ORDER BY cost DESC"));
        // Same window predicate as the by-service read, or the two tables in
        // one digest would cover different days and disagree on the total.
        assert!(sql.contains(">= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY)"));
        assert!(!sql.contains(" < TIMESTAMP_SUB"));
    }

    #[test]
    fn project_sql_labels_account_level_rows_and_bounds_the_prior_window() {
        // Account-level charges carry no project.id; without IFNULL they would
        // parse as an unnamed bucket.
        assert!(format_cost_by_project_window_sql("p.ds.t", 30, 0)
            .contains("IFNULL(project.id, '(unattributed)')"));
        let prior = format_cost_by_project_window_sql("p.ds.t", 60, 30);
        assert!(prior.contains(">= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 60 DAY)"));
        assert!(prior.contains("< TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY)"));
    }

    #[test]
    fn parse_project_costs_extracts_project_and_cost_pairs() {
        let resp = json!({
            "jobComplete": true,
            "rows": [
                { "f": [ {"v": "neon-law-420305"}, {"v": "269.67"} ] },
                { "f": [ {"v": "neon-law-stg"},  {"v": "0.13"} ] }
            ]
        });
        assert_eq!(
            parse_project_costs(&resp).unwrap(),
            vec![
                ProjectCost {
                    project: "neon-law-420305".into(),
                    cost: 269.67
                },
                ProjectCost {
                    project: "neon-law-stg".into(),
                    cost: 0.13
                },
            ]
        );
        // Same empty-window and unparseable-cell defaults as the service read.
        assert!(parse_project_costs(&json!({ "jobComplete": true }))
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn cost_by_project_window_posts_to_jobs_query_and_parses_the_response() {
        use super::{BillingClient, StaticToken};
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/bigquery/v2/projects/test-proj/queries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jobComplete": true,
                "rows": [ { "f": [ {"v": "ghcr"}, {"v": "0.02"} ] } ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = BillingClient::new("test-proj", Arc::new(StaticToken("fake-token".into())))
            .with_base_url(server.uri());
        let rows = client
            .cost_by_project_window("p.ds.t", 30, 0)
            .await
            .expect("query succeeds");
        assert_eq!(
            rows,
            vec![ProjectCost {
                project: "ghcr".into(),
                cost: 0.02
            }]
        );
    }

    #[test]
    fn export_tables_parse_one_entry_per_billing_account() {
        use super::billing_export_tables;

        // The common single-account case needs no separator.
        assert_eq!(billing_export_tables("p.ds.t"), vec!["p.ds.t".to_string()]);
        // Several accounts, tolerating whitespace and a trailing comma.
        assert_eq!(
            billing_export_tables(" a.ds.one , b.ds.two ,"),
            vec!["a.ds.one".to_string(), "b.ds.two".to_string()]
        );
        // Unset / blank → nothing configured, which callers treat as a no-op.
        assert!(billing_export_tables("").is_empty());
        assert!(billing_export_tables("  ,  ").is_empty());
    }

    #[test]
    fn billing_account_is_recovered_from_the_export_table_name() {
        use super::billing_account_from_table;

        // The real production table, resource-level export.
        assert_eq!(
            billing_account_from_table(
                "neon-law-420305.billing_export.gcp_billing_export_resource_v1_013469_2BBE03_532C72"
            ),
            "013469-2BBE03-532C72"
        );
        // Standard (non-resource) export names the same way.
        assert_eq!(
            billing_account_from_table("p.ds.gcp_billing_export_v1_01528F_251BB0_54E54F"),
            "01528F-251BB0-54E54F"
        );
        // Off-convention table → its own name, never a wrong account.
        assert_eq!(billing_account_from_table("p.ds.custom"), "p.ds.custom");
    }

    #[test]
    fn credits_sql_unnests_and_splits_promotion_from_discount() {
        let sql = format_cost_with_credits_sql("p.ds.gcp_billing_export_v1_X", 30, 0);
        // Gross is plain SUM(cost); credits come from UNNEST(credits)...
        assert!(sql.contains("SUM(cost)"));
        assert!(sql.contains("UNNEST(credits)"));
        // ...split by the two credit types that matter for the digest.
        assert!(sql.contains("c.type = 'PROMOTION'"));
        assert!(sql.contains("c.type = 'DISCOUNT'"));
        // net + the four named columns are present and grouped by service.
        assert!(sql.contains("AS gross"));
        assert!(sql.contains("AS promo_credit"));
        assert!(sql.contains("AS discount_credit"));
        assert!(sql.contains("AS net"));
        assert!(sql.contains("GROUP BY service ORDER BY gross DESC"));
        // Trailing 30d window, open at the recent end (no upper bound).
        assert!(sql.contains("INTERVAL 30 DAY"));
        assert!(!sql.contains(" < TIMESTAMP_SUB"));
    }

    #[test]
    fn credits_sql_prior_window_bounds_both_ends() {
        // Days 31–60 for the trend comparison: lower bound 60d, upper bound 30d.
        let sql = format_cost_with_credits_sql("p.ds.t", 60, 30);
        assert!(sql.contains(">= TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 60 DAY)"));
        assert!(sql.contains("< TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY)"));
    }

    #[test]
    fn promo_applied_sql_sums_all_time_promotion_with_no_partition_filter() {
        let sql = format_promo_applied_sql("p.ds.t");
        assert!(sql.contains("c.type = 'PROMOTION'"));
        assert!(sql.contains("AS promo_applied"));
        // "All-time" — no trailing-window filter, or the burn-to-date is wrong.
        assert!(!sql.contains("_PARTITIONTIME"));
    }

    #[test]
    fn parse_service_costs_extracts_gross_credits_and_net() {
        let resp = json!({
            "jobComplete": true,
            "rows": [
                { "f": [
                    {"v": "Kubernetes Engine"}, {"v": "114.44"},
                    {"v": "-114.44"}, {"v": "0"}, {"v": "0"}
                ] },
                { "f": [
                    {"v": "Cloud SQL"}, {"v": "19.32"},
                    {"v": "-15.00"}, {"v": "-4.32"}, {"v": "0"}
                ] }
            ]
        });
        let rows = parse_service_costs(&resp).unwrap();
        assert_eq!(
            rows[0],
            ServiceCost {
                service: "Kubernetes Engine".into(),
                gross: 114.44,
                promo_credit: -114.44,
                discount_credit: 0.0,
                net: 0.0,
            }
        );
        assert_eq!(rows[1].discount_credit, -4.32);
    }

    #[test]
    fn parse_service_costs_defaults_missing_or_unparseable_cells_to_zero() {
        // A row with only service + gross (credit columns absent) and one with
        // an unparseable credit both default the missing numbers to 0 — exactly
        // what IFNULL produces server-side, but parsed defensively too.
        let resp = json!({ "rows": [
            { "f": [ {"v": "Networking"}, {"v": "23.16"} ] },
            { "f": [ {"v": "Weird"}, {"v": "x"}, {"v": "n/a"}, {"v": ""}, {"v": "?"} ] }
        ] });
        let rows = parse_service_costs(&resp).unwrap();
        assert_eq!(rows[0].gross, 23.16);
        assert_eq!(rows[0].promo_credit, 0.0);
        assert_eq!(rows[0].net, 0.0);
        assert_eq!(rows[1].gross, 0.0);
    }

    #[test]
    fn parse_service_costs_treats_missing_rows_as_empty() {
        assert!(parse_service_costs(&json!({ "jobComplete": true }))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn parse_scalar_amount_reads_single_cell_then_defaults_to_zero() {
        let resp = json!({ "rows": [ { "f": [ {"v": "-182.50"} ] } ] });
        assert!((parse_scalar_amount(&resp) - -182.50).abs() < f64::EPSILON);
        // No rows (empty export / lagging) → 0, not an error.
        assert_eq!(parse_scalar_amount(&json!({ "jobComplete": true })), 0.0);
    }

    #[tokio::test]
    async fn cost_with_credits_posts_to_jobs_query_and_parses_the_response() {
        use super::{BillingClient, StaticToken};
        use std::sync::Arc;
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/bigquery/v2/projects/test-proj/queries"))
            .and(header("authorization", "Bearer fake-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jobComplete": true,
                "rows": [
                    { "f": [
                        {"v": "Compute Engine"}, {"v": "31.42"},
                        {"v": "-31.42"}, {"v": "0"}, {"v": "0"}
                    ] }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = BillingClient::new("test-proj", Arc::new(StaticToken("fake-token".into())))
            .with_base_url(server.uri());
        let rows = client
            .cost_with_credits("p.ds.t", 30, 0)
            .await
            .expect("query succeeds");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].service, "Compute Engine");
        assert_eq!(rows[0].gross, 31.42);
        assert_eq!(rows[0].promo_credit, -31.42);
        assert_eq!(rows[0].net, 0.0);
    }
}
