//! `devx gcp setup --project-id <PROJECT_ID>` — provision the GCP
//! resources `web` depends on (VPC, Cloud SQL Postgres, two GCS
//! buckets, GKE Autopilot cluster + Config Sync) from a single Rust
//! binary.
//!
//! Every step is **idempotent**: each `ensure_*` function issues a
//! create call and treats HTTP 409 Conflict (REST steps) or
//! `"already exists"` stderr (gcloud shell-out steps) as success. The
//! same `setup` invocation can therefore be re-run after a partial
//! failure without producing duplicates.
//!
//! ## Pipeline order
//!
//! 1. [`services::enable_services`] — `serviceusage.batchEnable`.
//!    Must run first; nothing else works without the APIs enabled.
//! 2. [`network::ensure_network`] — custom-mode VPC.
//! 3. [`sql::ensure_sql_instance`] — Cloud SQL Postgres instance,
//!    `navigator` database, `web` SQL user (with a generated
//!    password printed once to stderr).
//! 4. [`buckets::ensure_bucket`] for assets and logs.
//! 5. [`gke::ensure_autopilot_cluster`] — GKE Autopilot cluster,
//!    Gateway static IP, Fleet membership, Config Sync `RootSync`.
//!
//! Steps 1–4 talk to GCP REST APIs via [`client::GcpClient`]; step 5
//! shells out to `gcloud` and `kubectl` (the Container API alone is
//! ~200 lines of cluster JSON). Tests stand up wiremock and override
//! base URLs per service for the REST steps, and use the dry-run
//! recorder for the shell-out step — no traffic ever leaves the host,
//! no GCP credentials needed. See the `cloud-rest-endpoints` skill
//! for the layered CI strategy.

pub mod artifact_registry;
pub mod auth;
pub mod buckets;
pub mod client;
pub mod error;
pub mod gke;
pub mod iap;
pub mod lro;
pub mod network;
pub mod push_image;
pub mod services;
pub mod sql;

pub use error::SetupResult;

use self::client::GcpClient;

/// Default region. Overridable via `NAVIGATOR_GCP_LOCATION`.
pub const DEFAULT_REGION: &str = "us-west4";

/// Bucket name suffixes appended to the project ID.
pub const ASSETS_BUCKET_SUFFIX: &str = "-assets";
pub const DOCUMENTS_BUCKET_SUFFIX: &str = "-documents";
pub const LOGS_BUCKET_SUFFIX: &str = "-logs";

/// Per-deployment overrides for `devx gcp setup`. Every field defaults
/// to the workspace's preferred name; OSS forks change them via CLI
/// flags or env vars without forking the code. The struct is built by
/// `devx/src/main.rs::gcp_setup` from clap-parsed `--region`,
/// `--cluster-name`, etc. flags that fall back to env vars.
#[derive(Debug, Clone)]
pub struct SetupConfig {
    /// GCP region used for Cloud SQL, GKE Autopilot, and bucket
    /// location. Default: `us-west4`.
    pub region: String,
    /// GKE Autopilot cluster name. Default: `navigator-prod`.
    pub cluster_name: String,
    /// Cloud SQL Postgres instance name. Default: `navigator-pg`.
    pub sql_instance: String,
    /// VPC network name. Default: `navigator-vpc`.
    pub vpc_name: String,
    /// Reserved global static-IP name attached to the Gateway.
    /// Default: `navigator-gateway-ip`.
    pub gateway_ip_name: String,
    /// HTTPS URL of the GitHub (or other Git host) repo that `Config
    /// Sync` should reconcile from. `None` skips the `RootSync` step —
    /// the right default for OSS forks that don't run `GitOps` yet.
    pub config_sync_repo: Option<String>,
    /// Path inside `config_sync_repo` that the `RootSync` watches.
    /// Default: `examples/deploy/k8s/gke` to match the parameterized
    /// overlay shipped with the workspace.
    pub config_sync_dir: String,
}

impl Default for SetupConfig {
    fn default() -> Self {
        Self {
            region: DEFAULT_REGION.to_string(),
            cluster_name: gke::DEFAULT_CLUSTER_NAME.to_string(),
            sql_instance: sql::DEFAULT_INSTANCE_NAME.to_string(),
            vpc_name: network::DEFAULT_NETWORK_NAME.to_string(),
            gateway_ip_name: gke::DEFAULT_GATEWAY_IP_NAME.to_string(),
            config_sync_repo: None,
            config_sync_dir: "examples/deploy/k8s/gke".to_string(),
        }
    }
}

/// Run the full setup pipeline. See module docs for the order.
pub async fn run(client: &GcpClient, project_id: &str, config: &SetupConfig) -> SetupResult<()> {
    services::enable_services(client, project_id).await?;
    network::ensure_network(client, project_id, config).await?;
    sql::ensure_sql_instance(client, project_id, config).await?;
    let assets = format!("{project_id}{ASSETS_BUCKET_SUFFIX}");
    let documents = format!("{project_id}{DOCUMENTS_BUCKET_SUFFIX}");
    let logs = format!("{project_id}{LOGS_BUCKET_SUFFIX}");
    buckets::ensure_bucket(client, project_id, &assets, &config.region).await?;
    buckets::ensure_bucket(client, project_id, &documents, &config.region).await?;
    buckets::ensure_bucket(client, project_id, &logs, &config.region).await?;
    gke::ensure_autopilot_cluster(client, project_id, config).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::client::{GcpClient, GcpService, StaticToken};

    #[tokio::test]
    async fn dry_run_records_full_pipeline_with_no_network_traffic() {
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            // Point every service at an unreachable address so a
            // real HTTP call would fail loudly.
            .with_base_url(GcpService::ServiceUsage, "http://127.0.0.1:1")
            .with_base_url(GcpService::Compute, "http://127.0.0.1:1")
            .with_base_url(GcpService::SqlAdmin, "http://127.0.0.1:1")
            .with_base_url(GcpService::Storage, "http://127.0.0.1:1")
            .with_dry_run();

        let config = super::SetupConfig {
            config_sync_repo: Some("https://example.com/your-org/your-repo".into()),
            ..super::SetupConfig::default()
        };
        super::run(&client, "my-project", &config).await.unwrap();

        let calls = client.recorded_calls();
        // REST: 1 services.batchEnable + 1 networks.insert + 3 sqladmin
        // posts (instance, db, user) + 3 storage inserts (assets,
        // documents, logs) = 8.
        // SHELL (gke): 5 (gateway IP + create-auto + fleet-enable +
        // fleet-register + RootSync apply) = 5.
        assert_eq!(calls.len(), 13, "expected 13 calls, got {calls:?}");
        let urls: Vec<&str> = calls.iter().map(|c| c.url.as_str()).collect();
        let methods: Vec<&str> = calls.iter().map(|c| c.method).collect();

        assert!(
            urls[0].contains("services:batchEnable"),
            "step 1 services: {}",
            urls[0]
        );
        assert!(
            urls[1].contains("/global/networks"),
            "step 2 network: {}",
            urls[1]
        );
        assert!(
            urls[2].ends_with("/instances"),
            "step 3a sql instance: {}",
            urls[2]
        );
        assert!(
            urls[3].contains("/databases"),
            "step 3b sql db: {}",
            urls[3]
        );
        assert!(urls[4].contains("/users"), "step 3c sql user: {}", urls[4]);
        assert!(
            calls[5]
                .body
                .as_deref()
                .unwrap()
                .contains("my-project-assets"),
            "step 4a assets bucket: {:?}",
            calls[5].body
        );
        assert!(
            calls[6]
                .body
                .as_deref()
                .unwrap()
                .contains("my-project-documents"),
            "step 4b documents bucket: {:?}",
            calls[6].body
        );
        assert!(
            calls[7]
                .body
                .as_deref()
                .unwrap()
                .contains("my-project-logs"),
            "step 4c logs bucket: {:?}",
            calls[7].body
        );
        // Steps 8..=12 are the GKE shell-outs.
        for (i, m) in methods.iter().enumerate().skip(8) {
            assert_eq!(*m, "SHELL", "step {i} should be SHELL, got {m}");
        }
        assert!(
            urls[8].contains("compute addresses create"),
            "step 5a static IP: {}",
            urls[8]
        );
        assert!(
            urls[9].contains("container clusters create-auto"),
            "step 5b cluster: {}",
            urls[9]
        );
        assert!(
            urls[10].contains("fleet config-management enable"),
            "step 5c fleet enable: {}",
            urls[10]
        );
        assert!(
            urls[11].contains("fleet memberships register"),
            "step 5d fleet register: {}",
            urls[11]
        );
        assert!(
            urls[12].starts_with("kubectl apply"),
            "step 5e kubectl apply: {}",
            urls[12]
        );
    }

    /// Cross-reference the "Deploy the Navigator" workshop prose
    /// (`web/content/workshops/navigator/DEPLOY.md`) against the pipeline
    /// it teaches. If the prose names a service, bucket, or command the
    /// dry-run does not actually use — or omits one it does — this test
    /// fails and the workshop is stale. The *renderable* half of the
    /// contract (route, brand, stepped-content shape) lives in
    /// `features/tests/features/deploy_the_navigator_walkthrough.feature`;
    /// this is the half that can only run where `super::run` is reachable.
    #[tokio::test]
    async fn deploy_workshop_prose_matches_the_dry_run_pipeline() {
        use super::services::REQUIRED_SERVICES;

        let deploy_md = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../web/content/workshops/navigator/DEPLOY.md"
        );
        let prose = std::fs::read_to_string(deploy_md)
            .expect("read DEPLOY.md — the deploy workshop must exist for this grounding test");

        // Use the same placeholder project id the prose prints, so the
        // recorded bucket names line up with the names the workshop shows.
        let project_id = "your-project-id";
        let client = GcpClient::new(Arc::new(StaticToken("t".into())))
            .with_base_url(GcpService::ServiceUsage, "http://127.0.0.1:1")
            .with_base_url(GcpService::Compute, "http://127.0.0.1:1")
            .with_base_url(GcpService::SqlAdmin, "http://127.0.0.1:1")
            .with_base_url(GcpService::Storage, "http://127.0.0.1:1")
            .with_dry_run();
        super::run(&client, project_id, &super::SetupConfig::default())
            .await
            .unwrap();
        let calls = client.recorded_calls();

        // 1. The command the workshop prints is the real invocation —
        //    `cargo run -p cli -- gcp setup`, with cargo's `--`
        //    separator (the orchestration commands collapsed into the
        //    `navigator` CLI; there is no longer a separate `devx` binary).
        assert!(
            prose.contains("cargo run -p cli -- gcp setup --project-id"),
            "DEPLOY.md must print the real `cargo run -p cli -- gcp setup --project-id` command",
        );
        assert!(
            prose.contains("--dry-run"),
            "DEPLOY.md must teach the --dry-run preview",
        );

        // 2. The prose's "twelve" APIs match REQUIRED_SERVICES exactly,
        //    and each short name is named in the prose.
        assert_eq!(
            REQUIRED_SERVICES.len(),
            12,
            "the workshop says twelve APIs; keep prose and code in lockstep",
        );
        assert!(
            prose.contains("twelve"),
            "DEPLOY.md must state the API count in words (twelve)",
        );
        for svc in REQUIRED_SERVICES {
            let short = svc.strip_suffix(".googleapis.com").unwrap_or(svc);
            assert!(
                prose.contains(short),
                "DEPLOY.md must name the {svc} API (looked for `{short}`)",
            );
        }
        assert!(
            prose.contains("batchEnable"),
            "DEPLOY.md must name the serviceusage.batchEnable call",
        );

        // 3. Exactly three buckets are created, and the prose names each
        //    (this is what kills the stale "two buckets" drift).
        let mut bucket_names: Vec<String> = calls
            .iter()
            .filter_map(|c| c.body.as_deref())
            .flat_map(|body| {
                [
                    super::ASSETS_BUCKET_SUFFIX,
                    super::DOCUMENTS_BUCKET_SUFFIX,
                    super::LOGS_BUCKET_SUFFIX,
                ]
                .into_iter()
                .map(|suffix| format!("{project_id}{suffix}"))
                .filter(|name| body.contains(name.as_str()))
                .collect::<Vec<_>>()
            })
            .collect();
        bucket_names.sort();
        bucket_names.dedup();
        assert_eq!(
            bucket_names.len(),
            3,
            "pipeline must create exactly three buckets, got {bucket_names:?}",
        );
        for name in &bucket_names {
            assert!(
                prose.contains(name.as_str()),
                "DEPLOY.md must name the {name} bucket the pipeline creates",
            );
        }

        // 4. Idempotency: the prose teaches the 409-is-success rule.
        assert!(
            prose.contains("409"),
            "DEPLOY.md must explain that HTTP 409 means already-exists/success",
        );

        // 5. Scorpio's trust claim: the workshop documents the
        //    print-once-to-stderr password and never bakes a literal
        //    secret in. The generated password is 32 alphanumeric chars
        //    (sql.rs) and is never produced in dry-run; the longest
        //    alphanumeric run in honest prose stays well under that.
        assert!(
            prose.contains("stderr") && prose.contains("once"),
            "DEPLOY.md must document that the SQL password is printed once to stderr",
        );
        let longest_alnum_run = prose
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::len)
            .max()
            .unwrap_or(0);
        assert!(
            longest_alnum_run < 32,
            "DEPLOY.md must not bake in a literal SQL password (found a {longest_alnum_run}-char token)",
        );
    }
}
