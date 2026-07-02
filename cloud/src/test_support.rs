//! Test-only harness: a real GCS wire path against `fake-gcs-server`.
//!
//! Behind the `test-support` feature (mirroring `store`'s), so
//! production consumers of `cloud` don't pull `testcontainers` into
//! their build graph. Tests that need to exercise [`GcsStorage`]
//! (`crate::GcsStorage`) — the same backend KIND and prod use — spawn
//! one emulator container per call:
//!
//! ```ignore
//! let gcs = cloud::test_support::fake_gcs("navigator").await;
//! gcs.storage.put("forms/x.pdf", b"%PDF", "application/pdf").await?;
//! ```
//!
//! The container is reaped when the returned handle drops. Docker is
//! required everywhere in this workspace, so the helper never falls
//! back to a filesystem stub — a test that asks for the GCS wire path
//! gets the GCS wire path.

use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use crate::gcs::{GcsStorage, GcsStorageConfig};

/// The same emulator image the KIND overlay runs
/// (`k8s/overlays/kind/deps/fake-gcs-server.yaml`), pinned to a tag so
/// the test wire path doesn't drift under a `latest` re-pull.
const FAKE_GCS_IMAGE: &str = "fsouza/fake-gcs-server";
const FAKE_GCS_TAG: &str = "1.52.2";
const FAKE_GCS_PORT: u16 = 4443;

/// A running `fake-gcs-server` with one pre-created bucket and a
/// [`GcsStorage`] pointed at it. Dropping the handle stops the
/// container.
pub struct FakeGcs {
    /// Held so the container lives as long as the storage handle.
    _container: ContainerAsync<GenericImage>,
    pub storage: GcsStorage,
    /// The emulator endpoint (`http://<host>:<mapped-port>`), for a
    /// second handle onto the same emulator.
    pub endpoint: String,
    pub bucket: String,
}

impl FakeGcs {
    /// A second [`GcsStorage`] onto the same emulator, addressing
    /// `bucket` (created if missing). Lets a test model the prod
    /// multi-bucket topology (assets vs documents) on one container.
    ///
    /// # Panics
    ///
    /// Panics when the emulator rejects bucket creation or the client
    /// cannot be built — test-support code fails loudly.
    pub async fn storage_for_bucket(&self, bucket: &str) -> GcsStorage {
        create_bucket(&self.endpoint, bucket).await;
        GcsStorage::new_from_config(GcsStorageConfig {
            bucket: bucket.to_string(),
            endpoint: Some(self.endpoint.clone()),
        })
        .await
        .expect("fake-gcs client builds")
    }
}

/// Start a `fake-gcs-server` container, create `bucket`, and return a
/// [`GcsStorage`] speaking the real GCS JSON API against it.
///
/// # Panics
///
/// Panics when Docker is unavailable or the emulator misbehaves —
/// test-support code fails loudly rather than skipping.
pub async fn fake_gcs(bucket: &str) -> FakeGcs {
    let container = GenericImage::new(FAKE_GCS_IMAGE, FAKE_GCS_TAG)
        .with_exposed_port(FAKE_GCS_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stderr("server started at"))
        // `-scheme http`: the emulator defaults to self-signed TLS,
        // which the anonymous client refuses. `-public-host localhost`
        // keeps download/media URLs on the mapped host instead of the
        // production `storage.googleapis.com`.
        .with_cmd([
            "-scheme",
            "http",
            "-port",
            &FAKE_GCS_PORT.to_string(),
            "-public-host",
            "localhost",
        ])
        .start()
        .await
        .expect("fake-gcs-server container starts (is Docker running?)");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(FAKE_GCS_PORT.tcp())
        .await
        .expect("mapped fake-gcs port");
    let endpoint = format!("http://{host}:{port}");
    create_bucket(&endpoint, bucket).await;
    let storage = GcsStorage::new_from_config(GcsStorageConfig {
        bucket: bucket.to_string(),
        endpoint: Some(endpoint.clone()),
    })
    .await
    .expect("fake-gcs client builds");
    FakeGcs {
        _container: container,
        storage,
        endpoint,
        bucket: bucket.to_string(),
    }
}

/// Create a bucket on the emulator via the GCS JSON API. Idempotent
/// enough for tests: a 409 (already exists) is success.
async fn create_bucket(endpoint: &str, bucket: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{endpoint}/storage/v1/b?project=test"))
        .json(&serde_json::json!({ "name": bucket }))
        .send()
        .await
        .expect("bucket-create request reaches the emulator");
    assert!(
        resp.status().is_success() || resp.status().as_u16() == 409,
        "fake-gcs bucket create failed: {}",
        resp.status()
    );
}
