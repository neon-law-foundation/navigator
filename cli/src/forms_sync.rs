//! `navigator forms sync` — push the vendored government-form blanks
//! to the public assets bucket.
//!
//! The repo's `notation_templates/forms/` tree (bundled into the `forms`
//! registry) is the canonical copy; the bucket carries a serving copy
//! at each ledger `object_path` so the website and external readers
//! can fetch blanks without the binary. Idempotent: a key whose bytes
//! already exist is skipped (`StorageService::exists`), and revisions
//! are append-only — a re-vendor lands at a new path, old bytes stay.

use std::process::ExitCode;

use cloud::{GcsStorage, GcsStorageConfig, StorageService};

/// Long-lived cache: a vendored form revision is immutable (a refresh
/// gets a new `object_path`), so downstream caches may hold it.
const FORM_CACHE_CONTROL: &str = "public, max-age=604800";

/// Entry point for `cli forms sync`. `bucket` defaults to the
/// `NAVIGATOR_ASSETS_BUCKET` env var — the public `<project>-assets`
/// bucket, deliberately distinct from the documents bucket so blanks
/// never land in the confidential lane (and vice versa).
pub fn run_sync(bucket: Option<String>) -> ExitCode {
    let bucket = match bucket.or_else(|| std::env::var("NAVIGATOR_ASSETS_BUCKET").ok()) {
        Some(b) if !b.trim().is_empty() => b,
        _ => {
            eprintln!(
                "navigator: forms sync: no bucket — pass --bucket or set NAVIGATOR_ASSETS_BUCKET"
            );
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: forms sync: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        let cfg = GcsStorageConfig {
            bucket: bucket.clone(),
            endpoint: std::env::var("NAVIGATOR_STORAGE_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        };
        let storage = match GcsStorage::new_from_config(cfg).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: forms sync: open bucket `{bucket}`: {e}");
                return ExitCode::from(2);
            }
        };
        match sync(&storage).await {
            Ok((uploaded, skipped)) => {
                println!(
                    "navigator: forms sync: {uploaded} uploaded, {skipped} already present \
                     in gs://{bucket}/forms"
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("navigator: forms sync: {e:#}");
                ExitCode::from(2)
            }
        }
    })
}

/// Upload every registry form to its ledger `object_path`, skipping
/// keys that already exist (revisions are immutable, so presence is
/// sufficient). Returns `(uploaded, skipped)`.
async fn sync(storage: &dyn StorageService) -> anyhow::Result<(usize, usize)> {
    let forms = forms::registry()?;
    let (mut uploaded, mut skipped) = (0, 0);
    for form in &forms {
        let key = &form.meta.object_path;
        if storage.exists(key).await? {
            skipped += 1;
            continue;
        }
        storage
            .put_cached(key, form.bytes, "application/pdf", FORM_CACHE_CONTROL)
            .await?;
        println!("  {key} ({} bytes)", form.bytes.len());
        uploaded += 1;
    }
    Ok((uploaded, skipped))
}

#[cfg(test)]
mod tests {
    use super::sync;

    #[tokio::test]
    async fn sync_is_idempotent_against_fs_storage() {
        let storage = cloud::FsStorage::new(std::env::temp_dir().join("navigator-forms-sync-test"))
            .await
            .expect("temp FsStorage");
        let (uploaded, skipped) = sync(&storage).await.expect("first sync");
        assert!(uploaded + skipped == 3, "all three packets accounted for");
        let (second_uploaded, second_skipped) = sync(&storage).await.expect("second sync");
        assert_eq!(second_uploaded, 0, "second pass uploads nothing");
        assert_eq!(second_skipped, 3);
    }
}
