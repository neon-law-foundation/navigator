//! Per-chapter sync orchestration with failure isolation.
//!
//! For each configured chapter: fetch → parse → reconcile every section
//! into Postgres via the insert-only `store::statutes` helpers, then mark
//! any section that vanished from the chapter as repealed. A failed
//! chapter (fetch error or unparseable body) is logged and skipped — it
//! never aborts the run, and it never triggers a spurious repeal (we only
//! repeal against a chapter we actually parsed). A 404 is a soft "absent"
//! skip, not a failure: the NRS numbering has gaps.
//!
//! [`run_sync`] is generic over [`ChapterSource`] so it runs against the
//! live [`crate::fetch::Fetcher`] in production and a fixture stub in
//! tests, exercising the real reconcile path without the network.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::fetch::{ChapterSource, FetchOutcome, Fetcher};
use crate::parse::parse_chapter;
use crate::{chapter_url, ChapterSpec, CODE, JURISDICTION};
use store::statutes::{mark_missing_repealed, upsert_section, Outcome, SectionUpsert};
use store::Db;

/// What happened to one chapter this run. Serializable because it is the
/// journaled output of a per-chapter `ctx.run` step in the `Statutes`
/// workflow (a retry replays the cached result instead of re-scraping).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChapterOutcome {
    /// Parsed and reconciled. Counts are for this chapter alone.
    Synced {
        sections: usize,
        created: usize,
        revised: usize,
        repealed: u64,
    },
    /// HTTP 404 — the chapter does not exist. Skipped, not a failure.
    Absent,
    /// Fetch or parse failure. The message is logged; counts the chapter
    /// toward the run's failure threshold.
    Failed(String),
}

/// One chapter's result, tagged with its product for the run log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterResult {
    pub chapter: String,
    pub product: String,
    pub outcome: ChapterOutcome,
}

/// Aggregate counts across the whole run — what the bin prints and uses
/// to choose its exit code.
#[derive(Debug, Clone, Default)]
pub struct SyncSummary {
    pub chapters_ok: usize,
    pub chapters_absent: usize,
    pub chapters_failed: usize,
    pub sections_seen: usize,
    pub sections_created: usize,
    pub sections_revised: usize,
    pub sections_repealed: u64,
    pub results: Vec<ChapterResult>,
}

impl SyncSummary {
    /// Fold one chapter's result into the running totals and log the
    /// outcome. Shared by [`run_sync`] (the whole-corpus dev path) and the
    /// per-chapter `Statutes` Restate workflow, which accumulates journaled
    /// [`scrape_one_chapter`] results, so both aggregate identically.
    pub fn record(&mut self, result: ChapterResult) {
        match &result.outcome {
            ChapterOutcome::Synced {
                sections,
                created,
                revised,
                repealed,
            } => {
                self.chapters_ok += 1;
                self.sections_seen += sections;
                self.sections_created += created;
                self.sections_revised += revised;
                self.sections_repealed += repealed;
                tracing::info!(
                    chapter = %result.chapter,
                    product = %result.product,
                    sections,
                    created,
                    revised,
                    repealed,
                    "chapter synced"
                );
            }
            ChapterOutcome::Absent => {
                self.chapters_absent += 1;
                tracing::info!(chapter = %result.chapter, "chapter absent (404), skipped");
            }
            ChapterOutcome::Failed(msg) => {
                self.chapters_failed += 1;
                tracing::warn!(chapter = %result.chapter, error = %msg, "chapter failed, skipped");
            }
        }
        self.results.push(result);
    }
}

/// Run the sync over `chapters`, fetching from `source` and writing to
/// `db`. `base_url` is the NRS base; `run_at` is the shared RFC 3339 run
/// timestamp. Never returns `Err` for a single bad chapter — those land
/// in the summary so the run is resilient and fully reported.
pub async fn run_sync<S: ChapterSource>(
    db: &Db,
    source: &S,
    base_url: &str,
    chapters: &[ChapterSpec],
    run_at: &str,
) -> SyncSummary {
    let mut summary = SyncSummary::default();

    for (i, spec) in chapters.iter().enumerate() {
        if i > 0 {
            source.pause().await;
        }
        let url = chapter_url(base_url, spec.chapter);
        let outcome = sync_one_chapter(db, source, &url, spec, run_at).await;
        summary.record(ChapterResult {
            chapter: spec.chapter.to_string(),
            product: spec.product.to_string(),
            outcome,
        });
    }

    summary
}

/// Scrape and reconcile **one** chapter against the live source, opening a
/// fresh short-lived Postgres connection and HTTP fetcher for just this
/// chapter. This is the per-chapter durable step of the `Statutes` Restate
/// workflow: each call is one journaled `ctx.run`, so a worker crash or
/// inactivity-timeout retry resumes at the first un-journaled chapter
/// instead of re-scraping the whole corpus (the bug this entry point
/// exists to fix).
///
/// Failure isolation is preserved at the value level: a fetch/parse/db
/// problem for this chapter rides home inside [`ChapterOutcome::Failed`],
/// never as an `Err`, so one bad chapter is journaled and skipped rather
/// than aborting the run. `Err` is reserved for failing to *acquire* the
/// database handle, which Restate replays as a transient step failure.
///
/// No inter-chapter pause happens inside the step — the workflow spaces
/// chapters with a durable `ctx.sleep`, so the fetcher is built with a
/// zero delay (its own [`Fetcher::pause`] is never called for a single
/// fetch). Migrations are not run here; the workflow's one-shot prepare
/// step applies them once per run.
///
/// # Errors
///
/// Returns `Err` only when the database configuration or connection
/// cannot be acquired, or the HTTP client cannot be built.
pub async fn scrape_one_chapter(
    spec: &ChapterSpec,
    base_url: &str,
    run_at: &str,
) -> anyhow::Result<ChapterResult> {
    let cfg = store::config::DbConfig::from_env().context("read DATABASE_URL")?;
    let db = store::connect(&cfg).await.context("connect to Postgres")?;
    let fetcher = Fetcher::new(std::time::Duration::ZERO).context("build HTTP fetcher")?;

    let url = chapter_url(base_url, spec.chapter);
    let outcome = sync_one_chapter(&db, &fetcher, &url, spec, run_at).await;
    Ok(ChapterResult {
        chapter: spec.chapter.to_string(),
        product: spec.product.to_string(),
        outcome,
    })
}

async fn sync_one_chapter<S: ChapterSource>(
    db: &Db,
    source: &S,
    url: &str,
    spec: &ChapterSpec,
    run_at: &str,
) -> ChapterOutcome {
    let html = match source.fetch(url).await {
        Ok(FetchOutcome::Page(html)) => html,
        Ok(FetchOutcome::NotFound) => return ChapterOutcome::Absent,
        Err(e) => return ChapterOutcome::Failed(e.to_string()),
    };

    let parsed = match parse_chapter(&html) {
        Ok(p) => p,
        Err(e) => return ChapterOutcome::Failed(e.to_string()),
    };

    let mut created = 0;
    let mut revised = 0;
    let mut present: Vec<String> = Vec::with_capacity(parsed.sections.len());

    for section in &parsed.sections {
        // Section "649.005" → anchor "NRS649Sec005".
        let (chap_prefix, frac) = section
            .section
            .split_once('.')
            .unwrap_or((&section.section, ""));
        let source_url = format!("{url}#NRS{chap_prefix}Sec{frac}");

        let upsert = SectionUpsert {
            jurisdiction: JURISDICTION,
            code: CODE,
            chapter: spec.chapter,
            chapter_title: &parsed.chapter_title,
            section: &section.section,
            source_url: &source_url,
            section_title: &section.section_title,
            body: &section.body,
            body_sha256: &section.body_sha256,
            history_note: section.history_note.as_deref(),
        };

        match upsert_section(db, &upsert, run_at).await {
            Ok((_, Outcome::Created)) => created += 1,
            Ok((_, Outcome::Revised)) => revised += 1,
            Ok((_, Outcome::Unchanged)) => {}
            Err(e) => return ChapterOutcome::Failed(format!("db upsert {}: {e}", section.section)),
        }
        present.push(section.section.clone());
    }

    let repealed = match mark_missing_repealed(db, CODE, spec.chapter, &present, run_at).await {
        Ok(n) => n,
        Err(e) => return ChapterOutcome::Failed(format!("db repeal sweep: {e}")),
    };

    ChapterOutcome::Synced {
        sections: parsed.sections.len(),
        created,
        revised,
        repealed,
    }
}

#[cfg(test)]
mod tests {
    use super::run_sync;
    use crate::fetch::{ChapterSource, FetchError, FetchOutcome};
    use crate::ChapterSpec;
    use std::collections::HashMap;

    /// Fixture-backed source: maps a URL to a canned outcome.
    struct StubSource {
        pages: HashMap<String, FetchOutcome>,
    }

    impl ChapterSource for StubSource {
        async fn fetch(&self, url: &str) -> Result<FetchOutcome, FetchError> {
            match self.pages.get(url) {
                Some(o) => Ok(o.clone()),
                None => Ok(FetchOutcome::NotFound),
            }
        }
    }

    const FIXTURE: &str = include_str!("../tests/fixtures/nrs-649-excerpt.html");
    const BASE: &str = "https://example.test/NRS/";

    #[test]
    fn record_accumulates_counts_across_outcomes() {
        use super::{ChapterOutcome, ChapterResult, SyncSummary};
        // The workflow folds journaled per-chapter results in via `record`;
        // prove the aggregate matches one synced + one absent + one failed.
        let mut summary = SyncSummary::default();
        summary.record(ChapterResult {
            chapter: "86".into(),
            product: "Nest".into(),
            outcome: ChapterOutcome::Synced {
                sections: 40,
                created: 3,
                revised: 1,
                repealed: 2,
            },
        });
        summary.record(ChapterResult {
            chapter: "999".into(),
            product: "None".into(),
            outcome: ChapterOutcome::Absent,
        });
        summary.record(ChapterResult {
            chapter: "118A".into(),
            product: "Tenant defense".into(),
            outcome: ChapterOutcome::Failed("timeout".into()),
        });

        assert_eq!(summary.chapters_ok, 1);
        assert_eq!(summary.chapters_absent, 1);
        assert_eq!(summary.chapters_failed, 1);
        assert_eq!(summary.sections_seen, 40);
        assert_eq!(summary.sections_created, 3);
        assert_eq!(summary.sections_revised, 1);
        assert_eq!(summary.sections_repealed, 2);
        assert_eq!(summary.results.len(), 3);
    }

    fn stub_with_649() -> StubSource {
        let mut pages = HashMap::new();
        pages.insert(
            crate::chapter_url(BASE, "649"),
            FetchOutcome::Page(FIXTURE.to_string()),
        );
        StubSource { pages }
    }

    #[tokio::test]
    async fn syncs_a_chapter_then_reruns_idempotently() {
        let db = store::test_support::pg().await;
        let source = stub_with_649();
        let chapters = [ChapterSpec {
            chapter: "649",
            product: "Nautilus",
        }];

        let s1 = run_sync(&db, &source, BASE, &chapters, "2026-06-07T10:00:00Z").await;
        assert_eq!(s1.chapters_ok, 1);
        assert_eq!(s1.sections_seen, 2);
        assert_eq!(s1.sections_created, 2);
        assert_eq!(s1.sections_revised, 0);

        // Re-run with identical input: nothing created or revised.
        let s2 = run_sync(&db, &source, BASE, &chapters, "2026-06-14T10:00:00Z").await;
        assert_eq!(s2.sections_created, 0);
        assert_eq!(s2.sections_revised, 0);

        // The data landed and is readable through the store layer.
        let cur = store::statutes::section(&db, "NRS", "649.005")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cur.statute.chapter, "649");
        assert_eq!(cur.statute.chapter_title, "COLLECTION AGENCIES");
        assert!(cur
            .statute
            .source_url
            .ends_with("NRS-649.html#NRS649Sec005"));
        assert!(cur.revision.body.starts_with("As used in this chapter"));
    }

    #[tokio::test]
    async fn missing_chapter_is_absent_not_failed() {
        let db = store::test_support::pg().await;
        let source = stub_with_649();
        // 999 isn't in the stub → NotFound → Absent.
        let chapters = [ChapterSpec {
            chapter: "999",
            product: "None",
        }];
        let s = run_sync(&db, &source, BASE, &chapters, "2026-06-07T10:00:00Z").await;
        assert_eq!(s.chapters_absent, 1);
        assert_eq!(s.chapters_failed, 0);
        assert_eq!(s.chapters_ok, 0);
    }

    #[tokio::test]
    async fn unparseable_page_is_failed_and_isolated() {
        let db = store::test_support::pg().await;
        let mut pages = std::collections::HashMap::new();
        // chapter 78 returns a body with no NRS chapter heading
        pages.insert(
            crate::chapter_url(BASE, "78"),
            FetchOutcome::Page("<html><body>nope</body></html>".to_string()),
        );
        pages.insert(
            crate::chapter_url(BASE, "649"),
            FetchOutcome::Page(FIXTURE.to_string()),
        );
        let source = StubSource { pages };
        let chapters = [
            ChapterSpec {
                chapter: "78",
                product: "Nest",
            },
            ChapterSpec {
                chapter: "649",
                product: "Nautilus",
            },
        ];
        let s = run_sync(&db, &source, BASE, &chapters, "2026-06-07T10:00:00Z").await;
        // 78 failed, but 649 still synced — isolation holds.
        assert_eq!(s.chapters_failed, 1);
        assert_eq!(s.chapters_ok, 1);
        assert_eq!(s.sections_created, 2);
    }
}
