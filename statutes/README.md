# statutes

Weekly Nevada Revised Statutes scraper plus the public-reference data layer it feeds. The library fetches the
practice-relevant NRS chapters, parses each into sections, and reconciles them into Postgres via the insert-only
`store::statutes` helpers. Scraping is idempotent, so a mid-run crash loses nothing and a re-run is a no-op for
unchanged sections.

The scheduled run is the two-step `Statutes` Restate workflow — `scrape` (this library's `run_sync`) then `email` (a
Foundation-branded run summary) — hosted by the `workflows-service` worker and started by the thin `statutes-trigger`
`CronJob`, the same shape as `archives`. Design notes: [`docs/cronjobs.md`](../docs/cronjobs.md).

## What it provides

- `parse_chapter` → `ParsedChapter` / `ParsedSection` — pure HTML → sections, tested against a saved fixture.
- `Fetcher` / `ChapterSource` / `decode` — polite, rate-limited HTTP with windows-1252 decoding and a courteous
  `User-Agent` (`NeonLawFoundationBot/1.0`).
- `run_sync` → `SyncSummary` / `ChapterResult` — per-chapter orchestration with failure isolation (one bad chapter
  doesn't sink the run).
- `CHAPTERS` / `ChapterSpec` / `chapter_url` / `product_for` — the curated set of practice-relevant chapters and how
  each maps to a firm product. Jurisdiction is fixed `NV`, code `NRS`.

## Layout

- `src/lib.rs` — the scraper + reference library.
- `src/workflow.rs` — the two-step `Statutes` Restate workflow (scrape → email), bound by `workflows-service`.
- `src/email.rs` — the Foundation-branded weekly run-summary email.
- `src/bin/trigger.rs` — the `statutes-trigger` `CronJob` entrypoint, shipped as the `navigator-statutes-trigger`
  image (see the [`power-push`](../docs/cloud-operations.md) trigger-image note).
- `src/bin/statutes_sync.rs` — a manual/dev entrypoint that runs the same scrape directly, without the broker.

## Getting started

```bash
# Fixture-backed HTML parse + sync reconciliation against a testcontainers Postgres. No network needed.
cargo test -p statutes
```
