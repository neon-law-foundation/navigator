//! Cloud-provider abstractions for the Navigator workspace.
//!
//! This is the one crate that depends on a cloud-provider SDK
//! (`google-cloud-storage`). Everything else in the workspace
//! depends on the [`StorageService`] trait and stays
//! provider-agnostic.
//!
//! Two backends ship behind the trait:
//!
//! - [`FsStorage`] (in [`fs`]) writes to a filesystem directory —
//!   the default, used by local dev, the integration test rig,
//!   and small production deployments where a single PVC is
//!   enough.
//! - [`GcsStorage`] (in [`gcs`]) writes to Google Cloud Storage.
//!   For local development against a GCS emulator
//!   (`fake-gcs-server`), set `NAVIGATOR_STORAGE_ENDPOINT` to the
//!   emulator URL; for real GCP, leave it unset and the crate
//!   uses Application Default Credentials.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use thiserror::Error;

pub mod drive;
pub mod fs;
pub mod gcs;
pub mod redirect;
pub mod speech;

pub use drive::{DriveAuth, DriveError};
pub use fs::FsStorage;
pub use gcs::{GcsStorage, GcsStorageConfig};
pub use speech::{GoogleSpeechConfig, GoogleSpeechTranscriptProvider, SpeechError};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error on {key}: {source}")]
    Io {
        key: String,
        #[source]
        source: std::io::Error,
    },
    #[error("object not found: {0}")]
    NotFound(String),
    #[error("missing required env var: {0}")]
    MissingEnv(&'static str),
    #[error("gcs error on {key}: {message}")]
    Gcs { key: String, message: String },
    /// The backend does not support this operation. Returned by
    /// [`FsStorage::signed_url`] — local filesystem objects don't
    /// have a network address to sign. Callers fall back to
    /// proxying the bytes through the app.
    #[error("operation not supported on this storage backend: {0}")]
    Unsupported(&'static str),
}

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub key: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// One object returned by [`StorageService::list`] — its key and byte size,
/// without the bytes. Enough for the nightly Iceberg authoring to build a
/// manifest entry (path + `file_size_in_bytes`) per data file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectListing {
    pub key: String,
    pub size_bytes: u64,
}

#[async_trait]
pub trait StorageService: Send + Sync {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), StorageError>;

    /// Like [`put`](Self::put), but also stamps an HTTP `Cache-Control`
    /// directive on the stored object (e.g. `public, max-age=604800`).
    ///
    /// The default implementation ignores `cache_control` and delegates
    /// to [`put`](Self::put), so backends with no notion of HTTP cache
    /// metadata — [`FsStorage`], used by dev and tests — need no change.
    /// Only [`GcsStorage`] overrides it to set the header on the
    /// uploaded object, which is what lets the public assets bucket
    /// serve photos under a bounded TTL without a cache-bust token.
    async fn put_cached(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: &str,
        cache_control: &str,
    ) -> Result<(), StorageError> {
        let _ = cache_control;
        self.put(key, bytes, content_type).await
    }

    async fn get(&self, key: &str) -> Result<StoredObject, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// List objects whose key starts with `prefix`, with their byte sizes.
    /// Used by the nightly Iceberg authoring to discover the day's Parquet
    /// data files under `iceberg/<table>/data/dt=<date>/`. Order is
    /// unspecified. The default returns [`StorageError::Unsupported`]; the
    /// real backends ([`FsStorage`], [`GcsStorage`]) override it.
    async fn list(&self, prefix: &str) -> Result<Vec<ObjectListing>, StorageError> {
        let _ = prefix;
        Err(StorageError::Unsupported("list"))
    }

    /// Whether an object exists at `key`, without downloading it.
    ///
    /// The default implementation does a full [`get`](Self::get) and maps
    /// [`StorageError::NotFound`] to `Ok(false)`; any other error
    /// propagates. Backends override it with a metadata-only HEAD when one
    /// is cheaper than a full fetch — [`GcsStorage`] does. Used as a cheap
    /// readiness probe before a downstream step reads the object (e.g.
    /// confirming the worker has rendered + persisted a notation's PDF
    /// before dispatching it for signature).
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        match self.get(key).await {
            Ok(_) => Ok(true),
            Err(StorageError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Generate a time-limited URL that lets a client (typically a
    /// browser) fetch an object directly from the backend without
    /// proxying through the app. `expires_in` is the validity
    /// window; the caller picks a duration short enough that link
    /// sharing isn't a concern.
    ///
    /// Backends that have no concept of a signed URL (i.e.
    /// [`FsStorage`]) return [`StorageError::Unsupported`] so the
    /// caller knows to fall back to streaming the bytes.
    async fn signed_url(&self, key: &str, expires_in: Duration) -> Result<String, StorageError>;
}

/// Pick a backend based on `NAVIGATOR_STORAGE_BACKEND` (default `fs`).
///
/// The GCS bucket is the documents-preferred one
/// ([`GcsStorageConfig::from_env`]): `NAVIGATOR_DOCUMENTS_BUCKET` when set,
/// else `NAVIGATOR_STORAGE_BUCKET`. This is what `web` and the worker's
/// `document_open__*` render lane use. The Archives snapshot lane wants the
/// exports bucket instead — see [`exports_from_env`].
pub async fn from_env() -> Result<Arc<dyn StorageService>, StorageError> {
    backend_from_env(GcsStorageConfig::from_env).await
}

/// Like [`from_env`], but the GCS bucket comes from
/// `NAVIGATOR_STORAGE_BUCKET` ONLY ([`GcsStorageConfig::exports_from_env`]).
///
/// For the Archives exports lane on the `workflows-service` worker, which
/// also carries `NAVIGATOR_DOCUMENTS_BUCKET` for its document-render lane:
/// the two must resolve to different buckets on the same pod. The `fs`
/// backend is identical to [`from_env`] — dev/KIND keep one storage root.
pub async fn exports_from_env() -> Result<Arc<dyn StorageService>, StorageError> {
    backend_from_env(GcsStorageConfig::exports_from_env).await
}

/// Shared backend selection: `fs` unless `NAVIGATOR_STORAGE_BACKEND` names
/// GCS, in which case the bucket comes from `gcs_config` (which lane).
async fn backend_from_env<F>(gcs_config: F) -> Result<Arc<dyn StorageService>, StorageError>
where
    F: FnOnce() -> Result<GcsStorageConfig, StorageError>,
{
    let backend = std::env::var("NAVIGATOR_STORAGE_BACKEND").unwrap_or_else(|_| "fs".to_string());
    match backend.as_str() {
        "gcs" | "google" => Ok(Arc::new(GcsStorage::new_from_config(gcs_config()?).await?)),
        _ => {
            let root = std::env::var("NAVIGATOR_STORAGE_FS_ROOT")
                .unwrap_or_else(|_| "./var/storage".to_string());
            Ok(Arc::new(FsStorage::new(root).await?))
        }
    }
}

/// A key that never exists — [`wait_until_ready`] probes it so the check
/// forces a real round-trip to the backend without depending on any object
/// having been written yet.
const READINESS_PROBE_KEY: &str = "__navigator_readiness_probe__";

/// Block until the object store answers a probe, or `timeout` elapses.
///
/// Boot-time guard. In KIND the `web` pod can start before
/// `fake-gcs-server` is reachable, and the canonical seed writes template
/// bodies as blobs to the store — so without this, the first seed fails on
/// a connection error and the pod crash-loops (with a growing backoff)
/// until the dependency happens to come up. That is exactly what made the
/// KIND e2e flake: the suite runs seconds after bring-up, while `web` is
/// still in `CrashLoopBackOff`.
///
/// [`exists`](StorageService::exists) round-trips the backend and maps a
/// missing key to `Ok(false)`, so any `Ok` means the store is reachable; an
/// `Err` means not-yet, retried with a short fixed backoff until the
/// deadline, after which the last error propagates. The filesystem backend
/// answers instantly, so this is a no-op for local/`fs` dev.
pub async fn wait_until_ready(
    storage: &Arc<dyn StorageService>,
    timeout: Duration,
) -> Result<(), StorageError> {
    wait_until_ready_with(storage, timeout, Duration::from_millis(1500)).await
}

/// [`wait_until_ready`] with an explicit retry backoff, so tests can drive
/// the retry loop without real-time sleeps.
async fn wait_until_ready_with(
    storage: &Arc<dyn StorageService>,
    timeout: Duration,
    backoff: Duration,
) -> Result<(), StorageError> {
    let deadline = Instant::now() + timeout;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match storage.exists(READINESS_PROBE_KEY).await {
            Ok(_) => {
                if attempt > 1 {
                    tracing::info!(attempt, "object storage ready");
                }
                return Ok(());
            }
            Err(e) => {
                if Instant::now() >= deadline {
                    return Err(e);
                }
                tracing::warn!(attempt, error = %e, "object storage not ready yet, retrying");
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

#[cfg(test)]
mod ready_tests {
    use super::{wait_until_ready_with, StorageError, StorageService, StoredObject};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A storage whose readiness probe (`exists` → `get`) errors with a
    /// connection-like `Gcs` error for the first `fail_for` probes, then
    /// reports "object absent" (`NotFound` → `exists` returns `Ok(false)`).
    /// Models fake-gcs-server coming up partway through web boot.
    struct FlakyStore {
        probes: AtomicU32,
        fail_for: u32,
    }

    #[async_trait::async_trait]
    impl StorageService for FlakyStore {
        async fn get(&self, key: &str) -> Result<StoredObject, StorageError> {
            let n = self.probes.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_for {
                Err(StorageError::Gcs {
                    key: key.to_string(),
                    message: "connection refused".to_string(),
                })
            } else {
                Err(StorageError::NotFound(key.to_string()))
            }
        }
        async fn put(&self, _: &str, _: &[u8], _: &str) -> Result<(), StorageError> {
            unimplemented!()
        }
        async fn delete(&self, _: &str) -> Result<(), StorageError> {
            unimplemented!()
        }
        async fn signed_url(&self, _: &str, _: Duration) -> Result<String, StorageError> {
            unimplemented!()
        }
    }

    fn store(fail_for: u32) -> Arc<dyn StorageService> {
        Arc::new(FlakyStore {
            probes: AtomicU32::new(0),
            fail_for,
        })
    }

    #[tokio::test]
    async fn returns_ok_once_the_store_answers() {
        // Errors twice, then ready — wait should ride out the retries.
        let s = store(2);
        let r = wait_until_ready_with(&s, Duration::from_secs(5), Duration::from_millis(1)).await;
        assert!(r.is_ok(), "expected ready after retries, got {r:?}");
    }

    #[tokio::test]
    async fn ready_on_first_probe_returns_immediately() {
        let s = store(0);
        assert!(
            wait_until_ready_with(&s, Duration::from_secs(5), Duration::from_millis(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn times_out_with_the_last_error_when_never_ready() {
        // Never answers — the probe key is irrelevant; we just need the
        // deadline to win and the connection error to propagate.
        let s = store(u32::MAX);
        let err = wait_until_ready_with(&s, Duration::from_millis(20), Duration::from_millis(1))
            .await
            .expect_err("never-ready store must time out");
        assert!(
            matches!(err, StorageError::Gcs { .. }),
            "expected the last connection error to propagate, got {err:?}"
        );
    }
}
