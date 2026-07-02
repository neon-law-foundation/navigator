//! Google Cloud Storage backend for [`StorageService`](crate::StorageService).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use google_cloud_storage::client::{Client as GcsClient, ClientConfig};
use google_cloud_storage::http::objects::delete::DeleteObjectRequest;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use google_cloud_storage::http::objects::list::ListObjectsRequest;
use google_cloud_storage::http::objects::patch::PatchObjectRequest;
use google_cloud_storage::http::objects::upload::{Media, UploadObjectRequest, UploadType};
use google_cloud_storage::http::objects::Object;
use google_cloud_storage::http::Error as GcsHttpError;
use google_cloud_storage::sign::SignedURLOptions;

use crate::{StorageError, StorageService, StoredObject};

/// Configuration for the GCS backend. The bucket name is resolved from
/// `NAVIGATOR_DOCUMENTS_BUCKET` (preferred) falling back to
/// `NAVIGATOR_STORAGE_BUCKET`; the endpoint override is
/// `NAVIGATOR_STORAGE_ENDPOINT`. ADC-based auth picks up
/// `GOOGLE_APPLICATION_CREDENTIALS` (the GCP convention) automatically.
#[derive(Debug, Clone)]
pub struct GcsStorageConfig {
    pub bucket: String,
    /// Override endpoint for emulators (`fake-gcs-server`). `None`
    /// uses the real GCS endpoint and ADC auth.
    pub endpoint: Option<String>,
}

impl GcsStorageConfig {
    pub fn from_env() -> Result<Self, StorageError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Exports-lane variant: resolves the bucket from
    /// `NAVIGATOR_STORAGE_BUCKET` ONLY, never `NAVIGATOR_DOCUMENTS_BUCKET`.
    ///
    /// The Archives snapshot workflow writes to the dedicated exports
    /// bucket and must stay there even on a pod that also carries
    /// `NAVIGATOR_DOCUMENTS_BUCKET` for its document-render lane (the
    /// `workflows-service` worker does both). Using [`from_env`] there
    /// would silently follow the documents-bucket preference and land
    /// nightly Parquet in the documents bucket.
    pub fn exports_from_env() -> Result<Self, StorageError> {
        Self::exports_from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, StorageError> {
        // Bucket name resolution has a precedence chain so a single
        // workload can name its bucket specifically without disturbing
        // the others that share `cloud::from_env()`:
        //
        // 1. `NAVIGATOR_DOCUMENTS_BUCKET` — the private documents bucket
        //    `web` (and the worker's `document_open__*` render lane) write
        //    client documents + `blobs/<sha>` to. Set on the `web` pod and
        //    the `workflows-service` worker.
        // 2. `NAVIGATOR_STORAGE_BUCKET` — the generic fallback. The
        //    `archives` exports lane (via `exports_from_env`) points it at
        //    the exports bucket; KIND / the `navigator` CLI points it at the fake-gcs
        //    `navigator` bucket.
        //
        // The split keeps client documents out of the public `-assets`
        // bucket: the documents var gives `web` + the worker's render lane
        // their own private bucket, and the fallback serves every other
        // caller.
        let bucket = get("NAVIGATOR_DOCUMENTS_BUCKET")
            .or_else(|| get("NAVIGATOR_STORAGE_BUCKET"))
            .ok_or(StorageError::MissingEnv(
                "NAVIGATOR_DOCUMENTS_BUCKET or NAVIGATOR_STORAGE_BUCKET",
            ))?;
        Ok(Self {
            bucket,
            endpoint: Self::endpoint(&get),
        })
    }

    /// Resolve the bucket from `NAVIGATOR_STORAGE_BUCKET` only — the
    /// exports lane. See [`exports_from_env`](Self::exports_from_env).
    pub fn exports_from_lookup<F: Fn(&str) -> Option<String>>(
        get: F,
    ) -> Result<Self, StorageError> {
        let bucket = get("NAVIGATOR_STORAGE_BUCKET")
            .ok_or(StorageError::MissingEnv("NAVIGATOR_STORAGE_BUCKET"))?;
        Ok(Self {
            bucket,
            endpoint: Self::endpoint(&get),
        })
    }

    /// Assets-lane variant: resolves the bucket from
    /// `NAVIGATOR_ASSETS_BUCKET` (the public `<project>-assets` bucket)
    /// falling back to `NAVIGATOR_STORAGE_BUCKET` — the single-bucket
    /// KIND/dev topology, where fake-gcs's `navigator` bucket carries
    /// every lane. `NAVIGATOR_DOCUMENTS_BUCKET` is deliberately NOT in
    /// this chain: the private documents bucket must never shadow the
    /// public assets one.
    pub fn assets_from_env() -> Result<Self, StorageError> {
        Self::assets_from_lookup(|k| std::env::var(k).ok())
    }

    /// See [`assets_from_env`](Self::assets_from_env).
    pub fn assets_from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self, StorageError> {
        let bucket = get("NAVIGATOR_ASSETS_BUCKET")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| get("NAVIGATOR_STORAGE_BUCKET"))
            .ok_or(StorageError::MissingEnv(
                "NAVIGATOR_ASSETS_BUCKET or NAVIGATOR_STORAGE_BUCKET",
            ))?;
        Ok(Self {
            bucket,
            endpoint: Self::endpoint(&get),
        })
    }

    /// The emulator endpoint override, treating an empty string as unset.
    ///
    /// A Kubernetes env var declared with no `value:` arrives as `""`, and
    /// `std::env::var` returns `Ok("")` for it — not absent. If we kept
    /// that as `Some("")`, `new_from_config` would take the emulator
    /// branch with a host-less `storage_endpoint`, and every GCS request
    /// would fail to build a URL ("builder error") before reaching the
    /// network. Only a real, non-empty override (the fake-gcs URL in KIND)
    /// selects the emulator.
    fn endpoint<F: Fn(&str) -> Option<String>>(get: &F) -> Option<String> {
        get("NAVIGATOR_STORAGE_ENDPOINT").filter(|s| !s.is_empty())
    }
}

/// Google Cloud Storage backend.
#[derive(Clone)]
pub struct GcsStorage {
    client: Arc<GcsClient>,
    bucket: Arc<String>,
    /// True when an endpoint override (emulator) is configured. The
    /// anonymous emulator client has no signing identity, so
    /// [`StorageService::signed_url`] reports `Unsupported` and callers
    /// fall back to streaming the bytes through the app.
    emulator: bool,
}

impl GcsStorage {
    pub async fn new_from_config(cfg: GcsStorageConfig) -> Result<Self, StorageError> {
        // If an endpoint override is set (emulator), skip auth
        // entirely; otherwise let the crate discover ADC.
        let client_config = if let Some(endpoint) = cfg.endpoint.clone() {
            ClientConfig {
                storage_endpoint: endpoint,
                ..ClientConfig::default()
            }
            .anonymous()
        } else {
            ClientConfig::default()
                .with_auth()
                .await
                .map_err(|e| StorageError::Gcs {
                    key: "<auth>".into(),
                    message: e.to_string(),
                })?
        };

        Ok(Self {
            client: Arc::new(GcsClient::new(client_config)),
            bucket: Arc::new(cfg.bucket),
            emulator: cfg.endpoint.is_some(),
        })
    }
}

#[async_trait]
impl StorageService for GcsStorage {
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<(), StorageError> {
        let mut media = Media::new(key.to_string());
        media.content_type = content_type.to_string().into();
        self.client
            .upload_object(
                &UploadObjectRequest {
                    bucket: (*self.bucket).clone(),
                    ..Default::default()
                },
                bytes.to_vec(),
                &UploadType::Simple(media),
            )
            .await
            .map_err(|e| StorageError::Gcs {
                key: key.to_string(),
                message: e.to_string(),
            })?;
        Ok(())
    }

    async fn put_cached(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: &str,
        cache_control: &str,
    ) -> Result<(), StorageError> {
        // A `Simple` upload (`Media`) carries no place for the
        // `Cache-Control` header. The crate's `Multipart` upload would,
        // but in google-cloud-storage 0.24 it sends the request as
        // `multipart/form-data` (reqwest's default), which the GCS JSON
        // upload API rejects — verified against a real bucket. So upload
        // the bytes via the proven simple path, then PATCH the object's
        // metadata to set `Cache-Control`. The brief window where the
        // object carries GCS's default cache directive is harmless for a
        // deploy that re-uploads the whole tree.
        self.put(key, bytes, content_type).await?;
        self.client
            .patch_object(&PatchObjectRequest {
                bucket: (*self.bucket).clone(),
                object: key.to_string(),
                metadata: Some(Object {
                    cache_control: Some(cache_control.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .map_err(|e| StorageError::Gcs {
                key: key.to_string(),
                message: e.to_string(),
            })?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<StoredObject, StorageError> {
        let metadata = self
            .client
            .get_object(&GetObjectRequest {
                bucket: (*self.bucket).clone(),
                object: key.to_string(),
                ..Default::default()
            })
            .await
            .map_err(|e| map_gcs_error(&e, key))?;
        let bytes = self
            .client
            .download_object(
                &GetObjectRequest {
                    bucket: (*self.bucket).clone(),
                    object: key.to_string(),
                    ..Default::default()
                },
                &Range::default(),
            )
            .await
            .map_err(|e| map_gcs_error(&e, key))?;
        Ok(StoredObject {
            key: key.to_string(),
            bytes,
            content_type: metadata
                .content_type
                .unwrap_or_else(|| "application/octet-stream".into()),
        })
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        // Metadata-only HEAD: `get_object` fetches just the object's
        // metadata (no `download_object`), so the readiness probe never
        // streams the PDF bytes back. A `NotFound` is the negative answer;
        // any other error propagates.
        match self
            .client
            .get_object(&GetObjectRequest {
                bucket: (*self.bucket).clone(),
                object: key.to_string(),
                ..Default::default()
            })
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => match map_gcs_error(&e, key) {
                StorageError::NotFound(_) => Ok(false),
                other => Err(other),
            },
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.client
            .delete_object(&DeleteObjectRequest {
                bucket: (*self.bucket).clone(),
                object: key.to_string(),
                ..Default::default()
            })
            .await
            .map_err(|e| StorageError::Gcs {
                key: key.to_string(),
                message: e.to_string(),
            })?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<crate::ObjectListing>, StorageError> {
        // Page through every object under `prefix` (GCS caps a page at ~1000,
        // and a busy day's telemetry can exceed that).
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let resp = self
                .client
                .list_objects(&ListObjectsRequest {
                    bucket: (*self.bucket).clone(),
                    prefix: Some(prefix.to_string()),
                    page_token: page_token.clone(),
                    ..Default::default()
                })
                .await
                .map_err(|e| StorageError::Gcs {
                    key: prefix.to_string(),
                    message: e.to_string(),
                })?;
            if let Some(items) = resp.items {
                out.extend(items.into_iter().map(|o| crate::ObjectListing {
                    key: o.name,
                    size_bytes: u64::try_from(o.size).unwrap_or(0),
                }));
            }
            match resp.next_page_token {
                Some(t) if !t.is_empty() => page_token = Some(t),
                _ => break,
            }
        }
        Ok(out)
    }

    async fn signed_url(&self, key: &str, expires_in: Duration) -> Result<String, StorageError> {
        // The anonymous emulator client (fake-gcs-server) has no
        // signing identity; report `Unsupported` so callers stream
        // the bytes through the app instead of failing the request.
        if self.emulator {
            return Err(StorageError::Unsupported(
                "signed URLs against an emulator endpoint",
            ));
        }
        // V4 signed URL caps at 7 days; the caller picks the window.
        let opts = SignedURLOptions {
            expires: expires_in,
            ..SignedURLOptions::default()
        };
        self.client
            .signed_url(&self.bucket, key, None, None, opts)
            .await
            .map_err(|e| StorageError::Gcs {
                key: key.to_string(),
                message: e.to_string(),
            })
    }
}

/// Translate a `google_cloud_storage::http::Error` into `StorageError`.
/// 404 / "No such object" → `NotFound`; everything else → `Gcs`.
/// The string fallback covers proxy / emulator paths that may surface
/// the response code only in the `message` field of the
/// `ErrorResponse` rather than the structured `.code` field.
fn map_gcs_error(e: &GcsHttpError, key: &str) -> StorageError {
    if let GcsHttpError::Response(resp) = e {
        if resp.code == 404 {
            return StorageError::NotFound(key.to_string());
        }
    }
    let msg = e.to_string();
    if msg.contains("No such object") || msg.contains("404") {
        return StorageError::NotFound(key.to_string());
    }
    StorageError::Gcs {
        key: key.to_string(),
        message: msg,
    }
}

#[cfg(test)]
mod tests {
    use super::{GcsStorage, GcsStorageConfig};
    use crate::{StorageError, StorageService};
    use std::time::Duration;

    #[test]
    fn gcs_config_reports_missing_bucket() {
        let err = GcsStorageConfig::from_lookup(|_| None).unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::MissingEnv("NAVIGATOR_DOCUMENTS_BUCKET or NAVIGATOR_STORAGE_BUCKET")
            ),
            "got {err:?}",
        );
    }

    #[test]
    fn gcs_config_reads_endpoint_override() {
        use std::collections::HashMap;
        let map: HashMap<&str, &str> = HashMap::from([
            ("NAVIGATOR_STORAGE_BUCKET", "navigator"),
            ("NAVIGATOR_STORAGE_ENDPOINT", "http://fake-gcs:4443"),
        ]);
        let cfg = GcsStorageConfig::from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.bucket, "navigator");
        assert_eq!(cfg.endpoint.as_deref(), Some("http://fake-gcs:4443"));
    }

    #[test]
    fn empty_endpoint_is_treated_as_unset() {
        use std::collections::HashMap;
        // A K8s env var declared with no `value:` arrives as `""`. It
        // must NOT select the emulator branch — otherwise the GCS
        // backend builds host-less URLs and every request fails with a
        // reqwest "builder error" before hitting the network.
        let map: HashMap<&str, &str> = HashMap::from([
            ("NAVIGATOR_DOCUMENTS_BUCKET", "proj-documents"),
            ("NAVIGATOR_STORAGE_ENDPOINT", ""),
        ]);
        let cfg = GcsStorageConfig::from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.endpoint, None, "empty endpoint must resolve to None");
    }

    #[test]
    fn documents_bucket_takes_precedence_over_storage_bucket() {
        use std::collections::HashMap;
        // `web` sets both vars (the generic one may linger from a
        // previous config); the documents-specific one must win so
        // client blobs never land in whatever `STORAGE_BUCKET` named.
        let map: HashMap<&str, &str> = HashMap::from([
            ("NAVIGATOR_DOCUMENTS_BUCKET", "proj-documents"),
            ("NAVIGATOR_STORAGE_BUCKET", "proj-assets"),
        ]);
        let cfg = GcsStorageConfig::from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.bucket, "proj-documents");
    }

    #[test]
    fn falls_back_to_storage_bucket_when_documents_unset() {
        use std::collections::HashMap;
        // `archives` / KIND / the `navigator` CLI sets only the generic var; the
        // fallback keeps them resolving their own bucket unchanged.
        let map: HashMap<&str, &str> =
            HashMap::from([("NAVIGATOR_STORAGE_BUCKET", "proj-exports")]);
        let cfg = GcsStorageConfig::from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.bucket, "proj-exports");
    }

    #[test]
    fn assets_lane_prefers_assets_bucket_and_ignores_documents_bucket() {
        use std::collections::HashMap;
        // The fill path pulls blank government forms from the PUBLIC
        // assets bucket; the private documents bucket must never shadow
        // it, even on a pod that sets all three vars.
        let map: HashMap<&str, &str> = HashMap::from([
            ("NAVIGATOR_ASSETS_BUCKET", "proj-assets"),
            ("NAVIGATOR_DOCUMENTS_BUCKET", "proj-documents"),
            ("NAVIGATOR_STORAGE_BUCKET", "navigator"),
        ]);
        let cfg =
            GcsStorageConfig::assets_from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.bucket, "proj-assets");
        // KIND/dev single-bucket topology: only the generic var is set.
        let map: HashMap<&str, &str> = HashMap::from([("NAVIGATOR_STORAGE_BUCKET", "navigator")]);
        let cfg =
            GcsStorageConfig::assets_from_lookup(|k| map.get(k).map(|s| (*s).to_string())).unwrap();
        assert_eq!(cfg.bucket, "navigator");
        let err = GcsStorageConfig::assets_from_lookup(|_| None).unwrap_err();
        assert!(matches!(
            err,
            StorageError::MissingEnv("NAVIGATOR_ASSETS_BUCKET or NAVIGATOR_STORAGE_BUCKET")
        ));
    }

    #[test]
    fn exports_lane_ignores_documents_bucket_on_a_shared_pod() {
        use std::collections::HashMap;
        // The worker carries BOTH vars: DOCUMENTS for the render lane,
        // STORAGE for the exports lane. The exports resolver must pin to
        // STORAGE_BUCKET so nightly Parquet never follows the document
        // preference into the documents bucket. (The default `from_lookup`
        // on the same map resolves to documents — proving the two lanes
        // split.)
        let map: HashMap<&str, &str> = HashMap::from([
            ("NAVIGATOR_DOCUMENTS_BUCKET", "proj-documents"),
            ("NAVIGATOR_STORAGE_BUCKET", "proj-exports"),
        ]);
        let lookup = |k: &str| map.get(k).map(|s| (*s).to_string());
        let exports = GcsStorageConfig::exports_from_lookup(lookup).unwrap();
        assert_eq!(exports.bucket, "proj-exports");
        let documents = GcsStorageConfig::from_lookup(lookup).unwrap();
        assert_eq!(documents.bucket, "proj-documents");
    }

    #[tokio::test]
    async fn signed_url_is_unsupported_against_an_emulator_endpoint() {
        // The KIND dev loop runs the GCS backend against fake-gcs-server,
        // which has no signing identity. `signed_url` must report
        // `Unsupported` (so `web` streams the bytes) instead of a signer
        // error the caller treats as a 500.
        let storage = GcsStorage::new_from_config(GcsStorageConfig {
            bucket: "navigator".into(),
            endpoint: Some("http://localhost:30443".into()),
        })
        .await
        .unwrap();
        let err = storage
            .signed_url("notations/x/document.pdf", Duration::from_mins(1))
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Unsupported(_)), "got {err:?}");
    }

    #[test]
    fn exports_lane_requires_storage_bucket() {
        // With only DOCUMENTS set, the exports lane has no bucket to fall
        // back to — it must error rather than silently borrow documents.
        let err = GcsStorageConfig::exports_from_lookup(|k| {
            (k == "NAVIGATOR_DOCUMENTS_BUCKET").then(|| "proj-documents".to_string())
        })
        .unwrap_err();
        assert!(matches!(err, StorageError::MissingEnv(_)), "got {err:?}");
    }
}
