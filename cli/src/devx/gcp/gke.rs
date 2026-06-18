//! Provision the GKE Autopilot cluster that runs `web`.
//!
//! Unlike the other `ensure_*` modules in this crate, GKE provisioning
//! shells out to `gcloud` rather than hitting the Container API REST
//! surface directly. The cluster spec is ~200 lines of JSON if you
//! reach for the API; the `gcloud container clusters create-auto`
//! one-liner does the same thing with sensible defaults baked in.
//! Matching the existing `kind` / `kubectl` / `helm` shell-outs in
//! `devx/src/main.rs` also keeps the surface area familiar.
//!
//! Each shell-out flows through [`GcpClient::shell_out`]:
//!
//! - In `Mode::Execute`, the command runs and we inspect exit + stderr.
//! - In `Mode::DryRun`, the command is recorded as `SHELL <line>` and
//!   reports a synthetic zero exit. The recorder lets
//!   `devx gcp setup --dry-run` print the full plan without touching
//!   the cluster or the operator's gcloud session.
//!
//! ## Idempotency
//!
//! `gcloud` returns non-zero with `"already exists"` (or
//! `"ALREADY_EXISTS"`) on re-runs. [`ShellResult::is_already_exists`]
//! flags that case so each step matches the
//! `Created`/`AlreadyExists` shape used elsewhere. We deliberately
//! do NOT pre-check existence with a separate `describe` call — that
//! would force dry-run to either silently skip the create (wrong) or
//! gain a special "synthesize a describe response" mode (too much
//! plumbing).

use super::client::{GcpClient, ShellResult};
use super::error::{SetupError, SetupResult};
use super::SetupConfig;

/// Default Autopilot cluster name when the operator doesn't override
/// it. `SetupConfig::from_env` reads `NAVIGATOR_GKE_CLUSTER_NAME` to
/// replace this value.
pub const DEFAULT_CLUSTER_NAME: &str = "navigator-prod";

/// Default reserved global static-IP name attached to the Gateway.
/// Pinning the IP across cluster re-creates means DNS A records stay
/// valid. Overridable via `NAVIGATOR_GATEWAY_IP_NAME`.
pub const DEFAULT_GATEWAY_IP_NAME: &str = "navigator-gateway-ip";

/// Outcome of a single ensure step. Same shape as
/// `buckets::EnsureOutcome` for callers that want to log a verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureOutcome {
    Created,
    AlreadyExists,
}

/// Idempotently bring up the production cluster. Sequence:
///
/// 1. Reserve the global static IP the Gateway binds to.
/// 2. Create the Autopilot cluster.
/// 3. Enable Fleet config-management and register the cluster as a
///    membership.
/// 4. Apply the Config Sync `RootSync` so the cluster pulls
///    `k8s/overlays/gke` from this repo.
pub async fn ensure_autopilot_cluster(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<EnsureOutcome> {
    ensure_static_ip(client, project_id, config).await?;
    let cluster_outcome = ensure_cluster(client, project_id, config).await?;
    ensure_fleet_membership(client, project_id, config).await?;
    // Config Sync ties the cluster to a specific GitHub repo + dir.
    // Skip the step when the operator hasn't pointed `devx` at their
    // own fork — running it with a default placeholder URL would
    // create a `RootSync` that fails to clone and just spams the
    // cluster's `configsync` controller.
    if let Some(repo) = config.config_sync_repo.as_deref() {
        ensure_root_sync(client, repo, &config.config_sync_dir).await?;
    }
    Ok(cluster_outcome)
}

async fn ensure_static_ip(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<EnsureOutcome> {
    let result = client
        .shell_out(
            "gcloud",
            &[
                "compute",
                "addresses",
                "create",
                &config.gateway_ip_name,
                "--global",
                "--project",
                project_id,
            ],
        )
        .await?;
    classify(result, "reserve gateway IP")
}

async fn ensure_cluster(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<EnsureOutcome> {
    // The `--enable-secret-manager` flag was renamed in gcloud
    // (was `--enable-secret-manager-csi-driver`); the new flag
    // enables the Secret Manager add-on which registers a CSI
    // driver under the GKE-specific name `secrets-store-gke.csi.k8s.io`.
    //
    // The `--addons=ConfigConnector,BackupRestore` flag was tried
    // but didn't reliably reconcile on the GKE version we ended up
    // on. The cluster currently doesn't have either addon active;
    // IAM bindings are managed via direct `gcloud` calls instead.
    // See `prompts/cloud-logging-sink.md` and `prompts/devx-gcp-setup-fixes.md`.
    let result = client
        .shell_out(
            "gcloud",
            &[
                "container",
                "clusters",
                "create-auto",
                &config.cluster_name,
                "--project",
                project_id,
                "--region",
                &config.region,
                "--release-channel",
                "rapid",
                "--enable-secret-manager",
            ],
        )
        .await?;
    classify(result, "create-auto cluster")
}

async fn ensure_fleet_membership(
    client: &GcpClient,
    project_id: &str,
    config: &SetupConfig,
) -> SetupResult<EnsureOutcome> {
    // Enabling Config Management on the Fleet is a no-op when it's
    // already enabled; we run it unconditionally and don't treat its
    // result as a Created/AlreadyExists signal.
    let _ = client
        .shell_out(
            "gcloud",
            &[
                "beta",
                "container",
                "fleet",
                "config-management",
                "enable",
                "--project",
                project_id,
            ],
        )
        .await?;
    let gke_cluster = format!("{}/{}", config.region, config.cluster_name);
    let result = client
        .shell_out(
            "gcloud",
            &[
                "container",
                "fleet",
                "memberships",
                "register",
                &config.cluster_name,
                "--gke-cluster",
                &gke_cluster,
                "--project",
                project_id,
            ],
        )
        .await?;
    classify(result, "register fleet membership")
}

/// Apply the `RootSync` CR via `kubectl`. The cluster admin must have
/// already authenticated (`gcloud container clusters get-credentials`)
/// — this matches the assumption in `devx/src/main.rs::kind_up_steps`
/// that the operator's `kubectl` context already points at the right
/// cluster.
async fn ensure_root_sync(client: &GcpClient, repo: &str, dir: &str) -> SetupResult<EnsureOutcome> {
    let manifest = root_sync_manifest(repo, dir);
    let result = client
        .shell_out_with_stdin(
            "kubectl",
            &["apply", "--filename", "-", "--server-side"],
            Some(&manifest),
        )
        .await?;
    classify(result, "apply RootSync")
}

fn root_sync_manifest(repo: &str, dir: &str) -> String {
    format!(
        r"apiVersion: configsync.gke.io/v1beta1
kind: RootSync
metadata:
  name: navigator
  namespace: config-management-system
spec:
  sourceFormat: unstructured
  git:
    repo: {repo}
    branch: main
    dir: {dir}
    auth: none
"
    )
}

fn classify(result: ShellResult, op: &'static str) -> SetupResult<EnsureOutcome> {
    if result.succeeded() {
        return Ok(EnsureOutcome::Created);
    }
    if result.is_already_exists() {
        return Ok(EnsureOutcome::AlreadyExists);
    }
    Err(SetupError::ShellFailed {
        operation: op,
        command: result.command_line,
        exit: result.exit,
        stderr: result.stderr,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::client::{GcpClient, StaticToken};
    use super::super::SetupConfig;
    use super::ensure_autopilot_cluster;

    fn config_with_rootsync() -> SetupConfig {
        SetupConfig {
            config_sync_repo: Some("https://example.com/your-org/your-repo".into()),
            config_sync_dir: "examples/deploy/k8s/gke".into(),
            ..SetupConfig::default()
        }
    }

    #[tokio::test]
    async fn dry_run_records_each_gcloud_invocation_in_order() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into()))).with_dry_run();
        let config = config_with_rootsync();
        ensure_autopilot_cluster(&client, "my-project", &config)
            .await
            .unwrap();
        let calls = client.recorded_calls();
        assert_eq!(
            calls.len(),
            5,
            "expected 5 shell-outs (ip + cluster + fleet-enable + fleet-register + rootsync), got {calls:?}"
        );
        for call in &calls {
            assert_eq!(
                call.method, "SHELL",
                "gke calls should be recorded as SHELL, got {}",
                call.method
            );
        }
        assert!(
            calls[0].url.contains("compute addresses create"),
            "step 1 static IP: {}",
            calls[0].url
        );
        assert!(
            calls[0].url.contains(&config.gateway_ip_name),
            "step 1 mentions IP name: {}",
            calls[0].url
        );
        assert!(
            calls[1].url.contains("container clusters create-auto"),
            "step 2 cluster create: {}",
            calls[1].url
        );
        assert!(
            calls[1].url.contains(&config.cluster_name),
            "step 2 mentions cluster: {}",
            calls[1].url
        );
        assert!(
            calls[1].url.contains("--enable-secret-manager"),
            "step 2 enables the Secret Manager addon: {}",
            calls[1].url
        );
        assert!(
            calls[2].url.contains("fleet config-management enable"),
            "step 3 fleet enable: {}",
            calls[2].url
        );
        assert!(
            calls[3].url.contains("fleet memberships register"),
            "step 4 fleet register: {}",
            calls[3].url
        );
        assert!(
            calls[4].url.starts_with("kubectl apply"),
            "step 5 kubectl apply: {}",
            calls[4].url
        );
        let stdin = calls[4]
            .body
            .as_deref()
            .expect("RootSync manifest should be piped as stdin");
        assert!(
            stdin.contains(&config.config_sync_dir),
            "step 5 stdin references the overlay dir: {stdin}"
        );
        assert!(
            stdin.contains("kind: RootSync"),
            "step 5 stdin is a RootSync manifest: {stdin}"
        );
    }

    #[tokio::test]
    async fn dry_run_skips_rootsync_when_no_repo_configured() {
        // Default config has `config_sync_repo: None` so the
        // RootSync step must be skipped — running it with a
        // placeholder repo URL would create a non-syncable resource.
        let client = GcpClient::new(Arc::new(StaticToken("t".into()))).with_dry_run();
        ensure_autopilot_cluster(&client, "my-project", &SetupConfig::default())
            .await
            .unwrap();
        let calls = client.recorded_calls();
        assert_eq!(
            calls.len(),
            4,
            "expected 4 shell-outs (no RootSync), got {calls:?}"
        );
        assert!(
            calls.iter().all(|c| !c.url.starts_with("kubectl")),
            "no kubectl call should be recorded: {calls:?}"
        );
    }

    #[tokio::test]
    async fn dry_run_carries_the_project_id_through_every_step() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into()))).with_dry_run();
        ensure_autopilot_cluster(&client, "navigator-test", &config_with_rootsync())
            .await
            .unwrap();
        for call in client.recorded_calls() {
            if call.url.starts_with("kubectl") {
                continue; // RootSync manifest doesn't carry the project ID.
            }
            assert!(
                call.url.contains("navigator-test"),
                "expected project ID in {}",
                call.url
            );
        }
    }
}
