//! Snapshot orchestration — the work the old `archives nightly`
//! subcommand did, now a plain library function the Restate workflow
//! drives inside a journaled `ctx.run("snapshot", …)` step.
//!
//! [`snapshot_all`] walks [`crate::tables::ALL_TABLES`], encodes each
//! non-empty table to Parquet, uploads it under the canonical
//! `iceberg/<table>/data/<date>/part-<uuid>.parquet` key, and applies
//! the add-only [`crate::drift`] policy. Per-table failures are
//! collected into [`SnapshotSummary::failures`] rather than aborting
//! the run, so the diagnostic email still reports what did and didn't
//! succeed. Only a failure to acquire the database / storage handles
//! propagates as an error (Restate retries the whole step).

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use cloud::{StorageError, StorageService};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use serde::{Deserialize, Serialize};

use billing::gcp_cost::{adc_token_provider, BillingClient, CostReport};

use crate::tables::fetch_batch;
use crate::{
    batch_from_rows, classify, encode_parquet, fingerprint, fingerprint_key, snapshot_key,
    DriftDecision, SnapshotConfig, SnapshotEntry, StoredFingerprint, ALL_TABLES,
};

/// One table that failed to snapshot, with the rendered error so the
/// diagnostic email can surface it. Serializable because the whole
/// summary is the journaled output of the snapshot workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableFailure {
    pub table: String,
    pub error: String,
}

/// The journaled result of the snapshot phase. `run_date` is captured
/// here (inside the `ctx.run` step) so it is recorded once and stays
/// stable across Restate replays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSummary {
    pub run_date: NaiveDate,
    pub entries: Vec<SnapshotEntry>,
    pub failures: Vec<TableFailure>,
}

/// Open the Postgres connection and the object-storage backend the
/// snapshot phase needs. A failure here propagates so the workflow
/// step retries — the database or GCS being unreachable is transient.
pub async fn open_resources() -> Result<(DatabaseConnection, Arc<dyn StorageService>)> {
    let db_url = store::config::DbConfig::from_env()
        .context("read DATABASE_URL")?
        .to_url();
    let db = Database::connect(ConnectOptions::new(db_url))
        .await
        .context("open database connection")?;
    // Exports lane: nightly Parquet lands in the dedicated exports bucket
    // (`NAVIGATOR_STORAGE_BUCKET`), never the documents bucket — even
    // though the worker pod also carries `NAVIGATOR_DOCUMENTS_BUCKET` for
    // its document-render lane.
    let storage = cloud::exports_from_env()
        .await
        .context("open object storage")?;
    Ok((db, storage))
}

/// Snapshot every registered table. Returns a [`SnapshotSummary`]
/// even when individual tables fail; only the inability to acquire
/// the shared handles is an error.
pub async fn snapshot_all(
    db: &DatabaseConnection,
    storage: &dyn StorageService,
) -> SnapshotSummary {
    let run_date = Utc::now().date_naive();
    let mut entries: Vec<SnapshotEntry> = Vec::new();
    let mut failures: Vec<TableFailure> = Vec::new();
    for table in ALL_TABLES {
        match snapshot_table(db, storage, table).await {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => tracing::info!(table, "skipped (empty)"),
            Err(err) => {
                tracing::error!(table, error = ?err, "snapshot failed");
                failures.push(TableFailure {
                    table: (*table).to_string(),
                    error: format!("{err:#}"),
                });
            }
        }
    }
    SnapshotSummary {
        run_date,
        entries,
        failures,
    }
}

/// Snapshot one table to Parquet on object storage. `Ok(None)` for an
/// empty table.
async fn snapshot_table(
    db: &DatabaseConnection,
    storage: &dyn StorageService,
    table: &str,
) -> Result<Option<SnapshotEntry>> {
    let Some(batch) = fetch_batch(db, table).await? else {
        return Ok(None);
    };
    let rows = batch.num_rows();

    let current_fp = fingerprint(&batch);
    let prev_fp = read_fingerprint(storage, table).await?;
    let decision = classify(prev_fp.as_ref(), &current_fp)?;

    let bytes = encode_parquet(&batch)?;
    let cfg = SnapshotConfig::now(table);
    let key = snapshot_key(&cfg);
    storage
        .put(&key, &bytes, "application/vnd.apache.parquet")
        .await
        .with_context(|| format!("upload {key}"))?;

    if needs_fingerprint_write(prev_fp.as_ref(), &decision) {
        write_fingerprint(
            storage,
            &StoredFingerprint {
                table: table.to_string(),
                columns: current_fp,
            },
        )
        .await?;
    }

    tracing::info!(
        table,
        rows,
        key = %key,
        bytes = bytes.len(),
        decision = ?decision,
        "snapshot uploaded"
    );
    Ok(Some(SnapshotEntry {
        table: table.to_string(),
        rows,
        bytes: bytes.len(),
        key,
        drift: decision,
    }))
}

/// The GCP-cost phase. Env-gated on `BILLING_EXPORT_TABLE`: unset →
/// `Ok(None)` (KIND / dev / OSS forks skip it cleanly, needing no
/// `BigQuery` credentials). When set, query the billing export for
/// trailing-window cost by service, snapshot the result to the export
/// lake as the `gcp_cost` table (so it is queryable in `BigQuery` like
/// the data tables), and return it for the diagnostic email.
pub async fn cost_phase<F: Fn(&str) -> Option<String>>(get: F) -> Result<Option<CostReport>> {
    let non_empty = |k: &str| get(k).filter(|s| !s.is_empty());
    let Some(table) = non_empty("BILLING_EXPORT_TABLE") else {
        return Ok(None);
    };
    let project = non_empty("BIGQUERY_PROJECT").context(
        "BILLING_EXPORT_TABLE is set but BIGQUERY_PROJECT is not — both are required to query \
         the billing export",
    )?;
    let days: u32 = non_empty("ARCHIVES_COST_WINDOW_DAYS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let token = adc_token_provider().await?;
    let rows = BillingClient::new(project, token)
        .cost_by_service(&table, days)
        .await
        .context("query billing export")?;

    // Snapshot the cost rows to the export lake the same way data
    // tables are written, so `gcp_cost` is queryable in BigQuery too.
    let key = match batch_from_rows(&rows)? {
        Some(batch) => {
            let storage = cloud::exports_from_env()
                .await
                .context("open object storage for cost snapshot")?;
            let bytes = encode_parquet(&batch)?;
            let cfg = SnapshotConfig::now("gcp_cost");
            let key = snapshot_key(&cfg);
            storage
                .put(&key, &bytes, "application/vnd.apache.parquet")
                .await
                .with_context(|| format!("upload {key}"))?;
            Some(key)
        }
        None => None,
    };
    Ok(Some(CostReport { rows, key }))
}

fn needs_fingerprint_write(previous: Option<&StoredFingerprint>, decision: &DriftDecision) -> bool {
    matches!(decision, DriftDecision::Added(_)) || previous.is_none()
}

async fn read_fingerprint(
    storage: &dyn StorageService,
    table: &str,
) -> Result<Option<StoredFingerprint>> {
    let key = fingerprint_key(table);
    match storage.get(&key).await {
        Ok(obj) => {
            let parsed: StoredFingerprint = serde_json::from_slice(&obj.bytes)
                .with_context(|| format!("parse stored fingerprint at {key}"))?;
            Ok(Some(parsed))
        }
        Err(StorageError::NotFound(_)) => Ok(None),
        Err(other) => Err(other).with_context(|| format!("read fingerprint at {key}")),
    }
}

async fn write_fingerprint(storage: &dyn StorageService, fp: &StoredFingerprint) -> Result<()> {
    let key = fingerprint_key(&fp.table);
    let bytes = serde_json::to_vec_pretty(fp).context("serialize fingerprint")?;
    storage
        .put(&key, &bytes, "application/json")
        .await
        .with_context(|| format!("write fingerprint at {key}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{needs_fingerprint_write, SnapshotSummary, TableFailure};
    use crate::{DriftDecision, StoredFingerprint};

    #[test]
    fn first_run_writes_fingerprint_even_if_unchanged_decision() {
        assert!(needs_fingerprint_write(
            None,
            &DriftDecision::Added(vec!["id".into()])
        ));
    }

    #[test]
    fn added_decision_writes_fingerprint() {
        let prev = StoredFingerprint {
            table: "persons".into(),
            columns: vec!["id".into()],
        };
        assert!(needs_fingerprint_write(
            Some(&prev),
            &DriftDecision::Added(vec!["email".into()])
        ));
    }

    #[test]
    fn unchanged_decision_skips_fingerprint_rewrite() {
        let prev = StoredFingerprint {
            table: "persons".into(),
            columns: vec!["id".into()],
        };
        assert!(!needs_fingerprint_write(
            Some(&prev),
            &DriftDecision::Unchanged
        ));
    }

    #[test]
    fn snapshot_summary_round_trips_through_serde() {
        // The whole summary is the output of `ctx.run("snapshot", …)`,
        // so it must round-trip through serde for Restate to journal
        // and replay it. A live database-backed `snapshot_all` runs in
        // the KIND smoke test (RUNBOOK); the per-binary testcontainers
        // cost isn't worth it for a thin loop over the per-table
        // dispatch already covered in `tables.rs`.
        let summary = SnapshotSummary {
            run_date: chrono::NaiveDate::from_ymd_opt(2026, 5, 29).unwrap(),
            entries: Vec::new(),
            failures: vec![TableFailure {
                table: "persons".into(),
                error: "boom".into(),
            }],
        };
        let json = serde_json::to_string(&summary).unwrap();
        let back: SnapshotSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.failures.len(), 1);
        assert_eq!(back.run_date, summary.run_date);
    }
}
