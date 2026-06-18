//! `navigator lsp publish` — push prebuilt `navigator-lsp` binaries to
//! the public assets bucket so the [`/lsp`] page can hand them
//! out as direct downloads.
//!
//! The binary is a **public** artifact (open-source tooling), so it
//! lands in the public `<project>-assets` bucket — the same lane `cli
//! assets upload` and `cli forms sync` use — deliberately distinct from
//! the confidential documents bucket. Each platform's binary lands at
//! [`lsp_binary_key`]'s `lsp/<triple>/navigator-lsp`, the same key the
//! page's download buttons resolve through `views::assets::asset_url`.
//! Upload and download therefore share one source of truth
//! ([`views::lsp::LSP_TARGETS`]) and can't drift.
//!
//! Input is a directory laid out by triple — `<dir>/<triple>/navigator-lsp`
//! — exactly what the cross-build recipe in `docs/lsp/zed.md` produces.
//! A target whose binary is absent is reported and skipped, never an
//! error: a macOS-only publish from a laptop is a valid partial release.
//!
//! [`/lsp`]: https://www.neonlaw.com/lsp

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use cloud::{GcsStorage, GcsStorageConfig, StorageService};
use views::lsp::{lsp_binary_key, LSP_TARGETS};

/// `Cache-Control` stamped on every uploaded binary. The download path
/// `lsp/<triple>/navigator-lsp` is a stable "latest" key (a re-publish
/// overwrites it), so this is **bounded**, never `immutable`: a new
/// release is picked up once the hour elapses, rather than being pinned
/// in shared caches forever. Shorter than the photo TTL — LSP fixes
/// ship more often than marketing imagery.
const LSP_CACHE_CONTROL: &str = "public, max-age=3600";

/// The filename every target directory holds.
const BINARY_NAME: &str = "navigator-lsp";

/// Entry point for `cli lsp publish`. `bucket` defaults to the
/// `NAVIGATOR_ASSETS_BUCKET` env var — the public `<project>-assets`
/// bucket. `dir` is the cross-build output root (`<dir>/<triple>/navigator-lsp`).
pub fn run_publish(dir: &Path, bucket: Option<String>) -> ExitCode {
    let bucket = match bucket.or_else(|| std::env::var("NAVIGATOR_ASSETS_BUCKET").ok()) {
        Some(b) if !b.trim().is_empty() => b,
        _ => {
            eprintln!(
                "navigator: lsp publish: no bucket — pass --bucket or set NAVIGATOR_ASSETS_BUCKET"
            );
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("navigator: lsp publish: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        // Honor the emulator endpoint override (fake-gcs in KIND) but
        // point at the assets bucket; ADC auth against real GCS
        // otherwise. An empty value resolves to `None` so a dev shell
        // that exports `NAVIGATOR_STORAGE_ENDPOINT=` to shadow the
        // `.devx/env` fake-gcs overlay publishes to the real bucket.
        let cfg = GcsStorageConfig {
            bucket: bucket.clone(),
            endpoint: std::env::var("NAVIGATOR_STORAGE_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        };
        let storage = match GcsStorage::new_from_config(cfg).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("navigator: lsp publish: open bucket `{bucket}`: {e}");
                return ExitCode::from(2);
            }
        };
        match publish(&storage, dir).await {
            Ok((uploaded, missing)) => {
                println!("navigator: lsp publish: {uploaded} binary(ies) → gs://{bucket}/lsp");
                for triple in &missing {
                    println!(
                        "  (skipped {triple} — no binary at {dir}/{triple}/{BINARY_NAME})",
                        dir = dir.display()
                    );
                }
                if uploaded == 0 {
                    eprintln!(
                        "navigator: lsp publish: nothing uploaded — build the binaries first \
                         (see docs/lsp/zed.md)"
                    );
                    return ExitCode::from(2);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("navigator: lsp publish: {e:#}");
                ExitCode::from(2)
            }
        }
    })
}

/// Upload every registry target whose binary is present under
/// `<dir>/<triple>/navigator-lsp` to its [`lsp_binary_key`]. Returns
/// `(uploaded_count, missing_triples)`. Decoupled from backend
/// construction so tests drive it against the `Fs` backend.
async fn publish(
    storage: &dyn StorageService,
    dir: &Path,
) -> anyhow::Result<(usize, Vec<&'static str>)> {
    let mut uploaded = 0usize;
    let mut missing = Vec::new();
    for target in LSP_TARGETS {
        let path: PathBuf = dir.join(target.triple).join(BINARY_NAME);
        if !path.is_file() {
            missing.push(target.triple);
            continue;
        }
        let bytes = std::fs::read(&path).with_context(|| format!("read `{}`", path.display()))?;
        let key = lsp_binary_key(target.triple);
        storage
            .put_cached(&key, &bytes, "application/octet-stream", LSP_CACHE_CONTROL)
            .await
            .with_context(|| format!("upload `{key}`"))?;
        println!("  → {key} ({} bytes)", bytes.len());
        uploaded += 1;
    }
    Ok((uploaded, missing))
}

#[cfg(test)]
mod tests {
    use super::publish;
    use cloud::{FsStorage, StorageService};
    use std::fs;
    use tempfile::TempDir;
    use views::lsp::{lsp_binary_key, LSP_TARGETS};

    #[tokio::test]
    async fn publishes_present_targets_and_reports_missing() {
        let tmp = TempDir::new().expect("tempdir");
        // Lay out only the first target's binary; the rest are "missing."
        let present = LSP_TARGETS[0].triple;
        let target_dir = tmp.path().join(present);
        fs::create_dir_all(&target_dir).expect("mkdir target");
        fs::write(target_dir.join("navigator-lsp"), b"#!/bin/echo fake\n").expect("write binary");

        let storage = FsStorage::new(tmp.path().join("store"))
            .await
            .expect("fs storage");
        let (uploaded, missing) = publish(&storage, tmp.path()).await.expect("publish");

        assert_eq!(uploaded, 1, "only the one present binary is uploaded");
        assert_eq!(missing.len(), LSP_TARGETS.len() - 1);
        assert!(!missing.contains(&present));
        // The uploaded object is fetchable at the shared key.
        let obj = storage
            .get(&lsp_binary_key(present))
            .await
            .expect("uploaded binary present at key");
        assert_eq!(obj.bytes, b"#!/bin/echo fake\n");
    }

    #[tokio::test]
    async fn publishes_nothing_when_dir_is_empty() {
        let tmp = TempDir::new().expect("tempdir");
        let storage = FsStorage::new(tmp.path().join("store"))
            .await
            .expect("fs storage");
        let (uploaded, missing) = publish(&storage, tmp.path()).await.expect("publish");
        assert_eq!(uploaded, 0);
        assert_eq!(missing.len(), LSP_TARGETS.len());
    }
}
