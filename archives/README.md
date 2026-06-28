# archives

Nightly archival: snapshot every Postgres table to Parquet on object storage, email a diagnostic summary, repeat — now
driven by a Restate workflow. The snapshot driver is generic (serde over every `store::entity::*` table) with add-only
schema-drift handling, so a new column never breaks the export and a mid-run crash loses nothing on retry.

The crate is split into a long-running worker and a thin trigger:

- the `Archives` Restate service (`serve`) is compiled into and hosted by `workflows-service` — there is no separate
  archives pod;
- `src/bin/trigger.rs` is the thin `trigger` binary the nightly `CronJob` runs to start one invocation, shipped as the
  `navigator-archives-trigger` image (see the [`ship`](../docs/cloud-operations.md) trigger-image note).

Each phase is a durable `ctx.run(...)` step, so Restate owns retries and partial-failure recovery.

## What it provides

- `Archives` / `ArchivesService` / `RunReport` — the Restate workflow surface bound by `workflows-service`.
- `snapshot_all` / `SnapshotSummary` / `TableFailure` — the per-table snapshot runner with failure isolation.
- `encode_parquet`, `fetch_batch`, `ALL_TABLES` — the table → Arrow `RecordBatch` → Parquet path.
- `classify` / `DriftDecision` / `StoredFingerprint` — add-only schema-drift detection.
- `cost_phase` + `billing::{CostReport, CostRow}` — the GCP cost step (env-gated on `BILLING_EXPORT_TABLE`) that writes
  `gcp_cost` and the email's COST section. Note: this `archives::billing` is the **GCP cost-export client**, unrelated
  to the `billing` matter-fee crate.
- `render_diagnostic` / `DiagnosticReport` — the nightly summary email.

A `BigQuery` refresh phase used to exist; it was removed because BigLake external tables over the GCS Parquet re-scan
their `uris` glob at query time, so no metadata refresh is needed. Iceberg metadata authorship is a reserved follow-up
(the `iceberg/<table>/metadata/` prefix is held for it).

## Getting started

```bash
# Snapshot encoding + drift classification + email rendering against a testcontainers Postgres.
cargo test -p archives
```
