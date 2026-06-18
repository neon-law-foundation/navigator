//! `statutes-sync` — a manual/dev entrypoint for the NRS scrape.
//!
//! The scheduled weekly run is the `Statutes` Restate workflow (see
//! `crate::workflow`), started by the `statutes-trigger` `CronJob`. This
//! binary runs the same scrape directly, without the broker — handy for a
//! local `cargo run` or a one-off reconcile: connect to Postgres, ensure
//! the schema, fetch every configured chapter, reconcile each into the
//! insert-only `statutes` / `statute_revisions` tables, then print a
//! summary and exit. Idempotent — a re-run is a no-op for unchanged
//! sections. Exits non-zero only when more than a small threshold of
//! chapters fail, so a transient single-chapter blip doesn't mark the run
//! failed while a real outage does.
//!
//! Tunables (all optional, sensible defaults):
//! - `STATUTES_NRS_BASE_URL` — override the source base (OSS / testing).
//! - `STATUTES_FETCH_DELAY_SECS` — polite inter-chapter pause (default 2).
//! - `STATUTES_FAILURE_THRESHOLD` — failed-chapter count that fails the
//!   run (default 3).

use std::time::Duration;

use anyhow::{Context, Result};
use statutes::fetch::Fetcher;
use statutes::{run_sync, CHAPTERS, DEFAULT_NRS_BASE_URL};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(".devx/env");
    // One observability seam for every binary: stdout logs (JSON when an
    // OTLP endpoint is set) plus OTLP traces + metrics. Held to end of main
    // so the drop flushes any batched export before the process exits.
    let _telemetry = telemetry::init("navigator-statutes-sync");

    let base_url =
        std::env::var("STATUTES_NRS_BASE_URL").unwrap_or_else(|_| DEFAULT_NRS_BASE_URL.to_string());
    let delay_secs = env_u64("STATUTES_FETCH_DELAY_SECS", 2);
    let failure_threshold = usize::try_from(env_u64("STATUTES_FAILURE_THRESHOLD", 3)).unwrap_or(3);

    let cfg = store::config::DbConfig::from_env().context("read DATABASE_URL")?;
    let db = store::connect(&cfg).await.context("connect to Postgres")?;
    store::migrate(&db).await.context("apply migrations")?;

    let fetcher = Fetcher::new(Duration::from_secs(delay_secs)).context("build HTTP fetcher")?;
    let run_at = chrono::Utc::now().to_rfc3339();

    tracing::info!(
        chapters = CHAPTERS.len(),
        %base_url,
        delay_secs,
        "starting NRS sync"
    );

    let summary = run_sync(&db, &fetcher, &base_url, CHAPTERS, &run_at).await;

    println!(
        "NRS sync complete: {} ok, {} absent, {} failed | sections: {} seen, {} created, \
         {} revised, {} repealed",
        summary.chapters_ok,
        summary.chapters_absent,
        summary.chapters_failed,
        summary.sections_seen,
        summary.sections_created,
        summary.sections_revised,
        summary.sections_repealed,
    );

    if summary.chapters_failed > failure_threshold {
        tracing::error!(
            failed = summary.chapters_failed,
            threshold = failure_threshold,
            "too many chapters failed; marking run failed"
        );
        std::process::exit(1);
    }
    Ok(())
}

/// Read a `u64` env var, falling back to `default` when unset or
/// unparseable.
fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
