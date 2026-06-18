//! `recurring-trigger` — the thin `CronJob` entrypoint for recurring
//! subscription billing.
//!
//! Fires one `RecurringBilling` workflow invocation against the Restate
//! ingress, then exits. The workflow key is the UTC run **date**
//! (`YYYY-MM-DD`), so a same-day re-fire is idempotent: Restate runs a
//! given workflow key at most once. A daily CronJob therefore runs the
//! workflow once per day; the workflow's own per-**month** period guard
//! (`last_invoiced_period`) is what bills each subscription exactly once
//! per month — so a subscription opened mid-month is picked up on the next
//! daily run, and extra fires are no-ops. The call is one-way (`/send`):
//! Restate runs the billing on the `workflows-service` worker and owns the
//! retry schedule.
//!
//! Auth is the shared [`workflows::start_workflow`] bearer handling —
//! attached only when `RESTATE_AUTH_TOKEN` is present (Restate Cloud);
//! absent in KIND. Identical shape to the `billing-canary` trigger.

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(".devx/env");
    // One observability seam for every binary: stdout logs (JSON when an
    // OTLP endpoint is set) plus OTLP traces + metrics. Held to end of main
    // so the drop flushes any batched export before the process exits.
    let _telemetry = telemetry::init("navigator-recurring-billing-trigger");

    let ingress = std::env::var("RESTATE_INGRESS_URL")
        .context("RESTATE_INGRESS_URL must be set (the Restate ingress endpoint)")?;
    let auth_token = std::env::var("RESTATE_AUTH_TOKEN").ok();
    // Workflow key = UTC run date. Restate admits at most one invocation
    // per key, so a duplicate same-day fire is a no-op; the workflow's
    // monthly period guard owns the actual once-per-month billing.
    let run_id = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let body = workflows::start_workflow(
        &ingress,
        auth_token.as_deref(),
        "RecurringBilling",
        &run_id,
        "run",
        // No period override — the workflow resolves the current UTC month
        // inside its journaled billing step.
        &serde_json::json!({}),
        true, // one-way: accept the invocation and exit; Restate runs it.
    )
    .await
    .context("triggering RecurringBilling workflow")?;

    tracing::info!(%run_id, response = %body, "recurring billing workflow triggered");
    println!("triggered RecurringBilling/{run_id}: {body}");
    Ok(())
}
