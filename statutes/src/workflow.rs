//! The `Statutes` Restate workflow — the weekly NRS sync.
//!
//! Hosted by the `workflows-service` worker (which binds it alongside the
//! `Notation`, `Archives`, and `BillingCanary` services — all workflows
//! live on that one endpoint, so there is no separate statutes pod).
//!
//! One invocation == one weekly run, journaled step by step — which is
//! exactly why the run is a Restate workflow and not a one-shot batch: a
//! retry must resume where it stopped, not re-scrape the legislature's
//! site or re-send the summary email. The steps:
//!
//! 1. `ctx.run("prepare", …)` — open Postgres, apply migrations once, and
//!    capture the stable RFC 3339 run timestamp. The handle is dropped at
//!    the step boundary; only the journaled timestamp crosses into the
//!    per-chapter steps.
//! 2. `ctx.run("scrape-<chapter>", …)` — **one durable step per chapter**,
//!    run back-to-back with no `ctx.sleep` between them. Each opens its own
//!    short-lived connection, fetches + parses + reconciles a single chapter
//!    into the insert-only `statutes` / `statute_revisions` tables via
//!    [`scrape_one_chapter`], and returns a journaled per-chapter result. A
//!    bad chapter rides home inside its `ChapterOutcome` (recorded + skipped,
//!    never fatal); only a failure to acquire the database handle propagates,
//!    replaying just that chapter. Splitting the scrape per chapter is the
//!    fix for the original single ~2-3 min step that tripped the worker's 1m
//!    inactivity timeout and retried from chapter 0 forever. There is
//!    deliberately no inter-chapter `ctx.sleep`: a sleep makes every chapter
//!    boundary a Restate suspend/resume, and on the GKE + Envoy-sidecar
//!    deployment the resume leg is cut as an RT0010 transport error, wedging
//!    the run. With no suspensions the whole sweep is one short (~10-15s)
//!    leg that streams cleanly; politeness comes from the sequential
//!    one-pass + identifying User-Agent, and journaling already removes the
//!    retry storm that was the real source of hammering.
//! 3. `ctx.run("email", …)` — render the Foundation-branded run-summary
//!    email from the accumulated [`ScrapeReport`] and send it through the
//!    worker's [`EmailService`].
//!
//! The `CronJob` `statutes-trigger` POSTs to the Restate ingress to start
//! one invocation per week; Restate owns the retry schedule.

use std::sync::Arc;

use anyhow::Context;
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use workflows::email::base_url_from_env;
use workflows::EmailService;

use crate::email::{build_summary_email, notify_recipient};
use crate::sync::SyncSummary;
use crate::{scrape_one_chapter, CHAPTERS, DEFAULT_NRS_BASE_URL};

/// Request body for `Statutes::run`. Empty today — the trigger only needs
/// to start the workflow — but kept as a struct (rather than `()`) so a
/// field like an override base URL can be threaded later without breaking
/// the handler signature.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {}

/// One chapter that failed to scrape, with the rendered error so the
/// summary email can surface it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterFailure {
    pub chapter: String,
    pub error: String,
}

/// The journaled result of the scrape step — the aggregate run counts plus
/// any per-chapter failures. Serializable because it is the output of the
/// `scrape` workflow step and the input to the `email` step, so a Restate
/// replay re-uses it rather than re-scraping. Also the invocation's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeReport {
    /// RFC 3339 run timestamp, captured once inside the scrape step so it
    /// stays stable across replays.
    pub run_at: String,
    pub chapters_ok: usize,
    pub chapters_absent: usize,
    pub chapters_failed: usize,
    pub sections_seen: usize,
    pub sections_created: usize,
    pub sections_revised: usize,
    pub sections_repealed: u64,
    pub failures: Vec<ChapterFailure>,
}

impl ScrapeReport {
    /// Project a [`SyncSummary`] (plus the run timestamp) into the
    /// serializable, journal-friendly report. Pulls the failure detail out
    /// of the per-chapter results so the email can list what broke.
    #[must_use]
    pub fn from_summary(summary: &SyncSummary, run_at: &str) -> Self {
        let failures = summary
            .results
            .iter()
            .filter_map(|r| match &r.outcome {
                crate::sync::ChapterOutcome::Failed(error) => Some(ChapterFailure {
                    chapter: r.chapter.clone(),
                    error: error.clone(),
                }),
                _ => None,
            })
            .collect();
        Self {
            run_at: run_at.to_string(),
            chapters_ok: summary.chapters_ok,
            chapters_absent: summary.chapters_absent,
            chapters_failed: summary.chapters_failed,
            sections_seen: summary.sections_seen,
            sections_created: summary.sections_created,
            sections_revised: summary.sections_revised,
            sections_repealed: summary.sections_repealed,
            failures,
        }
    }

    /// Sections that changed this run — new plus amended. Drives the email
    /// subject's "N sections changed".
    #[must_use]
    pub fn sections_changed(&self) -> usize {
        self.sections_created + self.sections_revised
    }
}

#[restate_sdk::workflow]
#[name = "Statutes"]
pub trait Statutes {
    async fn run(req: Json<RunRequest>) -> Result<Json<ScrapeReport>, HandlerError>;
}

/// Service struct registered with the Restate endpoint. Holds only the
/// worker-side [`EmailService`]; the database handle and HTTP fetcher are
/// built inside the scrape step so nothing is held idle between weekly
/// runs. Same shape as `archives`'s `ArchivesService`.
#[derive(Clone)]
pub struct StatutesService {
    email: Arc<dyn EmailService>,
}

impl StatutesService {
    #[must_use]
    pub fn new(email: Arc<dyn EmailService>) -> Self {
        Self { email }
    }
}

impl Statutes for StatutesService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        _req: Json<RunRequest>,
    ) -> Result<Json<ScrapeReport>, HandlerError> {
        let base_url = std::env::var("STATUTES_NRS_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_NRS_BASE_URL.to_string());

        // Step 0 — prepare: connect, apply migrations once, and capture the
        // stable run timestamp. The database handle is dropped at the step
        // boundary (it isn't serializable); only the journaled `run_at`
        // string crosses into the per-chapter steps, so every chapter and
        // the summary email share one timestamp across replays.
        let run_at: String = ctx
            .run(|| async { Ok(Json(prepare().await?)) })
            .name("prepare")
            .await?
            .into_inner();

        // Steps 1..=N — one durable `ctx.run` step per chapter, executed
        // back-to-back with NO `ctx.sleep` between them. Each step is short
        // (one fetch + reconcile, well inside the 1m inactivity timeout) and
        // journaled, so a crash resumes at the first un-journaled chapter
        // instead of re-scraping from chapter 0 (the original bug). Crucially
        // there is no inter-chapter *suspension*: an inter-chapter `ctx.sleep`
        // makes every chapter boundary a Restate suspend/resume, and on this
        // GKE + Envoy-sidecar deployment the resume leg is cut as an RT0010
        // transport error (the response stream back to Restate Cloud drops),
        // wedging the run after the first suspension. Without sleeps the whole
        // sweep runs in one suspension-free leg (~10-15s for 32 chapters),
        // which streams cleanly. Politeness is preserved by the sequential
        // one-pass with an identifying User-Agent — the thing that actually
        // hammered the source was the *retry storm*, which per-chapter
        // journaling already eliminates. A single bad chapter rides home
        // inside its `ChapterOutcome` (recorded + skipped), never aborting.
        let mut summary = SyncSummary::default();
        for spec in CHAPTERS {
            let base_url = base_url.clone();
            let run_at = run_at.clone();
            let result = ctx
                .run(move || async move {
                    Ok(Json(scrape_one_chapter(spec, &base_url, &run_at).await?))
                })
                .name(format!("scrape-{}", spec.chapter))
                .await?
                .into_inner();
            summary.record(result);
        }
        let report = ScrapeReport::from_summary(&summary, &run_at);

        // Final step — Foundation-branded summary email, rendered from the
        // journaled report so an email-send retry re-sends without
        // re-scraping. Discard the send receipt: delivery is
        // fire-and-forget and keeping the step output `()` avoids
        // journaling a SendGrid message id we never read.
        let email = build_summary_email(
            &report,
            &notify_recipient(|k| std::env::var(k).ok()),
            &base_url_from_env(),
        );
        let svc = Arc::clone(&self.email);
        ctx.run(move || async move {
            svc.send(email)
                .await
                .map(|_| ())
                .map_err(HandlerError::from)
        })
        .name("email")
        .await?;

        Ok(Json(report))
    }
}

/// Once-per-run setup, journaled as the workflow's first step: open
/// Postgres, apply migrations once (not per chapter), and capture the
/// stable RFC 3339 run timestamp. The database handle is dropped here —
/// only the timestamp crosses the step boundary, and each per-chapter step
/// opens its own short-lived connection. `anyhow::Result` so the step's `?`
/// maps an acquisition failure to a retryable `HandlerError`.
async fn prepare() -> anyhow::Result<String> {
    let cfg = store::config::DbConfig::from_env().context("read DATABASE_URL")?;
    let db = store::connect(&cfg).await.context("connect to Postgres")?;
    store::migrate(&db).await.context("apply migrations")?;
    tracing::info!(
        chapters = CHAPTERS.len(),
        "starting NRS sync (per-chapter steps)"
    );
    Ok(chrono::Utc::now().to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::ScrapeReport;
    use crate::sync::{ChapterOutcome, ChapterResult, SyncSummary};

    fn summary_with_failure() -> SyncSummary {
        SyncSummary {
            chapters_ok: 2,
            chapters_absent: 1,
            chapters_failed: 1,
            sections_seen: 40,
            sections_created: 3,
            sections_revised: 1,
            sections_repealed: 0,
            results: vec![
                ChapterResult {
                    chapter: "86".into(),
                    product: "Nest".into(),
                    outcome: ChapterOutcome::Synced {
                        sections: 40,
                        created: 3,
                        revised: 1,
                        repealed: 0,
                    },
                },
                ChapterResult {
                    chapter: "118A".into(),
                    product: "Tenant defense".into(),
                    outcome: ChapterOutcome::Failed("timeout".into()),
                },
            ],
        }
    }

    #[test]
    fn from_summary_copies_counts_and_extracts_failures() {
        let report = ScrapeReport::from_summary(&summary_with_failure(), "2026-06-07T10:00:00Z");
        assert_eq!(report.run_at, "2026-06-07T10:00:00Z");
        assert_eq!(report.chapters_ok, 2);
        assert_eq!(report.chapters_failed, 1);
        assert_eq!(report.sections_created, 3);
        assert_eq!(report.sections_changed(), 4);
        // Only the Failed chapter lands in the failures list, with its error.
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].chapter, "118A");
        assert_eq!(report.failures[0].error, "timeout");
    }

    #[test]
    fn from_summary_with_no_failures_yields_empty_list() {
        let mut summary = summary_with_failure();
        summary.results.pop(); // drop the failed chapter
        summary.chapters_failed = 0;
        let report = ScrapeReport::from_summary(&summary, "2026-06-07T10:00:00Z");
        assert!(report.failures.is_empty());
    }
}
