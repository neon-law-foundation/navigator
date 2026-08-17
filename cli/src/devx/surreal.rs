//! The `SurrealDB` member of the local dependency tier (#1093).
//!
//! Surreal runs in KIND like every other dependency: Docker is a
//! prerequisite of this loop either way, so the Kubernetes setup is
//! worth exercising rather than routing around. Nothing here starts a
//! host process, and no new binary joins the preflight — the manifest
//! at `k8s/overlays/kind/surreal/surreal.yaml` is the whole of the
//! engine's lifecycle, and `worktree-env down` reclaims it by deleting
//! the cluster.
//!
//! Two consumers share one engine, which is why it is a server and not
//! an embedded database:
//!
//! - host `web`, through the port-forward this module's caller opens;
//! - the in-cluster `workflows-service` worker, at
//!   [`IN_CLUSTER_ENDPOINT`].
//!
//! Per-worktree isolation comes from the per-worktree cluster, exactly
//! two checkouts have two engines, so their
//! rows cannot meet.

use anyhow::{Context, Result};
use store::surreal::{AuthScope, SurrealAuth, SurrealConfig};

use super::{KindConfig, SURREAL_LOCAL_PASSWORD, SURREAL_LOCAL_USER, SURREAL_NAMESPACE};

/// Where a pod inside the cluster reaches the engine. The host uses a
/// port-forward; in-cluster callers use the Service, which needs no
/// forward and survives a pod restart.
pub(super) const IN_CLUSTER_ENDPOINT: &str = "ws://surreal.navigator.svc.cluster.local:8000";

/// The Service's port, in-cluster. The host-side port is a
/// `KindConfig` field because it varies per worktree slot; this one
/// never does.
pub(super) const SERVICE_PORT: u16 = 8000;

/// The connection the host uses: the port-forward, with the disposable
/// local root credentials.
pub(super) fn host_config(cfg: &KindConfig, database: &str) -> SurrealConfig {
    SurrealConfig {
        endpoint: format!("ws://localhost:{}", cfg.surreal_port),
        namespace: SURREAL_NAMESPACE.to_string(),
        database: database.to_string(),
        auth: SurrealAuth::Password {
            scope: AuthScope::Root,
            username: SURREAL_LOCAL_USER.to_string(),
            password: SURREAL_LOCAL_PASSWORD.to_string(),
        },
    }
}

/// Apply the DEFINE schema to this environment's database.
///
/// Idempotent, so every `up` runs it: a reused cluster converges on the
/// current definitions, and a fresh one gets them for the first time.
/// `worktree-env up` applies it unconditionally.
pub(super) fn apply_schema(cfg: &KindConfig, database: &str) -> Result<()> {
    let config = host_config(cfg, database);
    runtime()?.block_on(async {
        let db = store::surreal::connect(&config)
            .await
            .with_context(|| format!("connect to SurrealDB at {}", config.endpoint))?;
        store::schema::apply(&db)
            .await
            .context("apply the SurrealDB schema")
    })
}

/// A private current-thread runtime, mirroring how the rest of `devx`
/// bridges its sync command handlers to async work.
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for SurrealDB schema operations")
}

#[cfg(test)]
mod tests {
    use super::{host_config, IN_CLUSTER_ENDPOINT, SERVICE_PORT};
    use std::path::Path;
    use store::surreal::{AuthScope, SurrealAuth};

    fn manifest() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root is cli/'s parent")
            .join("k8s/overlays/kind/surreal/surreal.yaml");
        std::fs::read_to_string(path).expect("read the KIND Surreal manifest")
    }

    /// The in-cluster endpoint is a claim about the manifest: the
    /// Service name, its namespace, and its port. If a manifest edit
    /// renames or remaps any of them, the worker's endpoint silently
    /// stops resolving — so the coupling is asserted here rather than
    /// discovered at runtime.
    #[test]
    fn the_in_cluster_endpoint_matches_the_manifest_it_names() {
        let manifest = manifest();

        assert!(manifest.contains("name: surreal"), "{manifest}");
        assert!(manifest.contains("namespace: navigator"), "{manifest}");
        assert!(
            manifest.contains(&format!("port: {SERVICE_PORT}")),
            "the Service must publish {SERVICE_PORT}"
        );
        assert!(
            manifest.contains(&format!("containerPort: {SERVICE_PORT}")),
            "the container must listen on {SERVICE_PORT}"
        );
        assert_eq!(
            IN_CLUSTER_ENDPOINT,
            format!("ws://surreal.navigator.svc.cluster.local:{SERVICE_PORT}")
        );
    }

    /// The pod must bind every interface: a Service reaches it from
    /// another pod's address, so a loopback bind would leave the worker
    /// unable to connect while the pod itself looked healthy.
    #[test]
    fn the_engine_binds_every_interface_so_the_service_can_reach_it() {
        assert!(manifest().contains("0.0.0.0:8000"), "{}", manifest());
    }

    /// Local data resetting with the pod is a decision, not an
    /// accident: `memory` is the storage argument, and phase 1 must not
    /// silently acquire a persistent volume.
    #[test]
    fn the_local_engine_is_memory_backed_with_no_volume() {
        let manifest = manifest();
        assert!(manifest.contains("- memory"), "{manifest}");
        assert!(!manifest.contains("PersistentVolumeClaim"), "{manifest}");
    }

    #[test]
    fn the_host_config_points_at_the_forwarded_port_with_local_credentials() {
        let mut cfg = super::super::tests::default_cfg();
        cfg.surreal_port = 20_224;

        let config = host_config(&cfg, "navigator");

        assert_eq!(config.endpoint, "ws://localhost:20224");
        assert_eq!(config.namespace, "navigator");
        assert_eq!(config.database, "navigator");
        assert_eq!(
            config.auth,
            SurrealAuth::Password {
                scope: AuthScope::Root,
                username: "root".into(),
                password: "root".into(),
            }
        );
    }
}
