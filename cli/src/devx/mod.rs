// devx — developer-experience tool that brings up Neon Law Navigator
// dependency stack (Postgres, fake-gcs-server, Keycloak, Restate,
// OPA) inside a KIND cluster while leaving the `web` binary on the
// host so it can be restarted in-process during a Rust edit-compile
// loop.
//
// The KIND cluster + manifests are also what `devx deploy` uses.
// Both flows drive Kustomize overlays under `k8s/overlays/`:
//   - `kind-deps` — base + deps + workflows-service (no `web`),
//                   used by `devx up` for host-side iteration
//   - `kind`      — full local stack including `web`,
//                   used by `devx deploy`
//   - `gke`       — production overlay; Config Sync reconciles
//                   this in GKE Autopilot

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;

pub mod brand;
mod dns;
mod doctor;
mod e2e;
mod gcp;
mod ghcr;
mod observability;
mod ship;
mod worktree_env;

pub use worktree_env::WorktreeEnvCmd;

// KIND/local defaults. Each pairs with a `KindConfig` field and a
// `NAVIGATOR_*` env var (see `KindConfig::from_env`). The constants are
// the fallback an empty `.env` resolves to, so default behavior is
// byte-for-byte what the old inline `const`s gave.
const DEFAULT_CLUSTER_NAME: &str = "navigator";
const DEFAULT_NAMESPACE: &str = "navigator";
const INGRESS_MANIFEST: &str = "https://raw.githubusercontent.com/kubernetes/ingress-nginx/controller-v1.11.2/deploy/static/provider/kind/deploy.yaml";

// Restate Operator — same chart drives KIND and GKE. Release notes:
// https://github.com/restatedev/restate-operator/releases. Kept aligned
// with the server image in `k8s/overlays/kind/deps/restate.yaml` and
// `RESTATE_CLI_VERSION` below (2.6.1 / 1.7.0). NOTE: the local "restate
// won't provision" wedge was NOT a version skew — it was the operator's
// own `deny-all` NetworkPolicy being enforced by recent kindnet, which
// blocked the operator→node `:5122` provisioning dial. The fix lives in
// `restate.yaml` (`security.disableNetworkPolicies: true`), not here.
const RESTATE_OPERATOR_VERSION: &str = "2.6.1";

// Restate CLI — pinned so operator-laptop and CI-runner versions
// don't drift. The CLI talks to Restate Cloud's admin API; a
// mismatch silently desyncs `deployments register`. Bumps require
// editing this constant and the matching `RESTATE_CLI_VERSION`
// env in `.github/workflows/deploy.yml`. 1.6.x renamed
// `deployment` to `deployments` and `--env` to `--environment`.
// Tracks the server line (see `RESTATE_OPERATOR_VERSION` above and
// the server image in `restate.yaml`) — keep all three on 1.7.x.
const RESTATE_CLI_VERSION: &str = "1.7.0";

// Last-resort public HTTPS endpoint Restate Cloud uses to reach the
// `workflows-service` worker. Backed by the ingress + managed cert
// under `examples/deploy/k8s/gke/ingress/workflows-*.yaml`. This is a
// placeholder of *last* resort: the URL is resolved by
// [`resolve_workflows_url`], which prefers an explicit `--url` /
// `NAVIGATOR_WORKFLOWS_URL`, then derives `https://workflows.<domain>/`
// from `NAVIGATOR_PRIMARY_DOMAIN`, and only falls back to this constant
// when none of those are set. Hitting this constant in prod means the
// re-register silently no-ops (the 2026-06-10 ship symptom).
pub(crate) const WORKFLOWS_PUBLIC_URL: &str = "https://workflows.example.com/";

// Local `:dev` image tags KIND loads. CI publishes the real images to
// ghcr (`YY.M.D`); `pull_retag_load` pulls one and retags it to the
// `:dev` name the manifests reference, so the overlays stay unchanged.
// The trigger images (archives/statutes/billing/heartbeat) are pulled
// straight from ghcr by their CronJobs in prod and are never loaded into
// the local cluster, so they need no local tag constant here.
const WEB_IMAGE: &str = "navigator-web:dev";
const WORKFLOWS_SERVICE_IMAGE: &str = "navigator-workflows-service:dev";

// Kustomize overlay roots. `Up` applies the deps-only overlay (no
// in-cluster `web`). `Deploy` applies the full overlay including
// `web`. The `gke` overlay is reconciled by Config Sync in
// production — `kustomize-gke` renders it locally for inspection.
const DEFAULT_KUSTOMIZE_KIND_DEPS: &str = "k8s/overlays/kind-deps";
const DEFAULT_KUSTOMIZE_KIND: &str = "k8s/overlays/kind";
// GKE overlay lives under `examples/deploy/k8s/gke/` (moved out of
// the canonical `k8s/` tree for the open-source release — the prod
// overlay is now an example users adapt, not a hard-coded part of
// the workspace surface). Config Sync in NeonLaw's prod cluster
// reconciles the same path, just under the new directory.
const DEFAULT_KUSTOMIZE_GKE: &str = "examples/deploy/k8s/gke";

// Host-side ports the locally-run `web` binary connects to.
// Restate's ingress port (8080 in-cluster) is remapped to host 9080
// because KIND already binds host 8080 to its nginx ingress. Postgres
// is remapped to 15432 so it doesn't collide with a host-side
// Postgres install on the standard 5432.
const DEFAULT_POSTGRES_HOST_PORT: u16 = 15432;
const DEFAULT_RESTATE_INGRESS_HOST_PORT: u16 = 9080;
const DEFAULT_RESTATE_ADMIN_HOST_PORT: u16 = 9070;
const DEFAULT_OPA_HOST_PORT: u16 = 8181;
// Keycloak (30080) and fake-gcs-server (30443) are already exposed
// to the host via NodePort + kind-config extraPortMappings. These are
// the only two host ports that touch `k8s/kind-config.yaml`: their
// `hostPort:` entries bind at `kind create` time (see
// `render_kind_config`). The NodePort/`containerPort` stays fixed.
const DEFAULT_KEYCLOAK_HOST_PORT: u16 = 30080;
const DEFAULT_FAKE_GCS_HOST_PORT: u16 = 30443;

// Local `web` defaults — matches `cargo run -p web` defaults.
const DEFAULT_LOCAL_WEB_PORT: u16 = 3001;

// Grafana LGTM telemetry sink (the `lgtm` Service in `navigator`).
// Grafana's UI (3000) and the OTLP gRPC ingest port (4317) are each
// port-forwarded to the host so the operator can browse traces/logs/
// metrics and the host-side `web` binary can export to them. 3000 is
// Grafana's own port; the host side keeps it for muscle memory.
const DEFAULT_LGTM_GRAFANA_HOST_PORT: u16 = 3000;
const DEFAULT_LGTM_OTLP_HOST_PORT: u16 = 4317;

/// Every KIND/local knob `devx` reads, resolved once in `main()` and
/// threaded into the subcommands. Each field falls back to a
/// `DEFAULT_*` constant, so an empty `.env` reproduces prior behavior
/// exactly. New local knobs are added here and in `from_env`, never as
/// a scattered `env::var` at a call site. See `docs/env-driven-devx.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KindConfig {
    cluster: String,
    namespace: String,
    deps_overlay: String,
    full_overlay: String,
    gke_overlay: String,
    postgres_port: u16,
    restate_ingress_port: u16,
    restate_admin_port: u16,
    opa_port: u16,
    keycloak_port: u16,
    fake_gcs_port: u16,
    web_port: u16,
    grafana_port: u16,
    otlp_port: u16,
}

impl KindConfig {
    /// Resolve the KIND/local config from the environment, falling back
    /// to the `DEFAULT_*` constants for any var that is unset or empty.
    fn from_env() -> Self {
        Self {
            cluster: env_string("NAVIGATOR_KIND_CLUSTER", DEFAULT_CLUSTER_NAME),
            namespace: env_string("NAVIGATOR_K8S_NAMESPACE", DEFAULT_NAMESPACE),
            deps_overlay: env_string("NAVIGATOR_KIND_DEPS_OVERLAY", DEFAULT_KUSTOMIZE_KIND_DEPS),
            full_overlay: env_string("NAVIGATOR_KIND_OVERLAY", DEFAULT_KUSTOMIZE_KIND),
            gke_overlay: env_string("NAVIGATOR_GKE_OVERLAY", DEFAULT_KUSTOMIZE_GKE),
            postgres_port: env_port("NAVIGATOR_KIND_POSTGRES_PORT", DEFAULT_POSTGRES_HOST_PORT),
            restate_ingress_port: env_port(
                "NAVIGATOR_KIND_RESTATE_INGRESS_PORT",
                DEFAULT_RESTATE_INGRESS_HOST_PORT,
            ),
            restate_admin_port: env_port(
                "NAVIGATOR_KIND_RESTATE_ADMIN_PORT",
                DEFAULT_RESTATE_ADMIN_HOST_PORT,
            ),
            opa_port: env_port("NAVIGATOR_KIND_OPA_PORT", DEFAULT_OPA_HOST_PORT),
            keycloak_port: env_port("NAVIGATOR_KIND_KEYCLOAK_PORT", DEFAULT_KEYCLOAK_HOST_PORT),
            fake_gcs_port: env_port("NAVIGATOR_KIND_FAKE_GCS_PORT", DEFAULT_FAKE_GCS_HOST_PORT),
            web_port: env_port("NAVIGATOR_KIND_WEB_PORT", DEFAULT_LOCAL_WEB_PORT),
            grafana_port: env_port(
                "NAVIGATOR_KIND_GRAFANA_PORT",
                DEFAULT_LGTM_GRAFANA_HOST_PORT,
            ),
            otlp_port: env_port("NAVIGATOR_KIND_OTLP_PORT", DEFAULT_LGTM_OTLP_HOST_PORT),
        }
    }
}

/// Read a string env var, treating unset *and* empty as "use default".
/// Empty-as-default keeps a `FOO=` line in `.env` from blanking a path.
fn env_string(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Read a `u16` port env var, falling back to `default` when unset,
/// empty, or unparseable. An invalid value falls back rather than
/// crashing the dev loop — the default is always a working port.
fn env_port(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(default)
}

#[derive(Subcommand)]
pub enum DnsCmd {
    /// Ensure `SendGrid`-compatible mail records on the given domain.
    /// Add `--dkim-target1` and `--dkim-target2` once you've run
    /// `SendGrid`'s Domain Authentication wizard (it hands you the
    /// two `CNAME` targets to paste).
    Setup {
        /// Apex domain to provision (e.g. `example.com`).
        #[arg(long)]
        domain: String,
        /// First `DKIM` `CNAME` target from `SendGrid` Domain Authentication.
        #[arg(long)]
        dkim_target1: Option<String>,
        /// Second `DKIM` `CNAME` target from `SendGrid` Domain Authentication.
        #[arg(long)]
        dkim_target2: Option<String>,
        /// Preview the `DNSimple` calls without sending any traffic.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum GcpCmd {
    /// Provision network, Postgres, three GCS buckets, and a GKE
    /// Autopilot cluster in the given Google Cloud project. Safe to
    /// re-run: every step ignores "already exists" responses.
    Setup {
        /// Google Cloud project ID (e.g. `your-project-id`).
        #[arg(long)]
        project_id: String,
        /// Region for Cloud SQL, GKE Autopilot, and bucket location.
        /// Falls back to `NAVIGATOR_GCP_LOCATION` then to the
        /// workspace default.
        #[arg(long, env = "NAVIGATOR_GCP_LOCATION")]
        region: Option<String>,
        /// GKE Autopilot cluster name. Falls back to
        /// `NAVIGATOR_GKE_CLUSTER_NAME`.
        #[arg(long, env = "NAVIGATOR_GKE_CLUSTER_NAME")]
        cluster_name: Option<String>,
        /// Cloud SQL Postgres instance name. Falls back to
        /// `NAVIGATOR_SQL_INSTANCE`.
        #[arg(long, env = "NAVIGATOR_SQL_INSTANCE")]
        sql_instance: Option<String>,
        /// VPC network name. Falls back to `NAVIGATOR_VPC_NAME`.
        #[arg(long, env = "NAVIGATOR_VPC_NAME")]
        vpc_name: Option<String>,
        /// Reserved global static-IP name. Falls back to
        /// `NAVIGATOR_GATEWAY_IP_NAME`.
        #[arg(long, env = "NAVIGATOR_GATEWAY_IP_NAME")]
        gateway_ip_name: Option<String>,
        /// HTTPS URL of the Git repo `Config Sync` should reconcile
        /// from. Falls back to `NAVIGATOR_CONFIG_SYNC_REPO`. Omit to
        /// skip the `RootSync` step entirely — sensible for forks not
        /// running `GitOps` yet.
        #[arg(long, env = "NAVIGATOR_CONFIG_SYNC_REPO")]
        config_sync_repo: Option<String>,
        /// Path inside the repo Config Sync should watch. Falls back
        /// to `NAVIGATOR_CONFIG_SYNC_DIR`.
        #[arg(long, env = "NAVIGATOR_CONFIG_SYNC_DIR")]
        config_sync_dir: Option<String>,
        /// Preview the GCP API calls that would be made, without
        /// sending any traffic. `gcloud` has no universal
        /// equivalent of this flag, so we provide one ourselves.
        #[arg(long)]
        dry_run: bool,
    },
    /// Identity-Aware Proxy operations for the `navigator-web`
    /// backend. Run after the GKE Ingress has provisioned the LB.
    /// See `docs/gemini-enterprise-mcp.md`.
    #[command(subcommand)]
    Iap(IapCmd),
}

#[derive(Subcommand)]
pub enum IapCmd {
    /// Print the IAP audience string `web::iap::IapConfig` validates
    /// against. Format:
    /// `/projects/<PROJECT_NUMBER>/global/backendServices/<SERVICE_ID>`.
    /// Paste the value into `IAP_AUDIENCE` in your GKE overlay
    /// (the example overlay lives at
    /// `examples/deploy/k8s/gke/patches/web-env.yaml`).
    Audience {
        /// Google Cloud project ID (e.g. `your-project-id`).
        #[arg(long)]
        project_id: String,
        /// Compute backend-service name. Defaults to `navigator-web`,
        /// matching the example GKE overlay.
        #[arg(long, default_value = gcp::iap::DEFAULT_SERVICE_NAME)]
        service: String,
    },
    /// Add `--member` to `roles/iap.httpsResourceAccessor` on the
    /// IAP-protected backend service. Idempotent: a no-op when the
    /// principal is already bound. Accepted member formats:
    /// `user:libra@example.com`, `group:staff@example.com`,
    /// `serviceAccount:s@p.iam.gserviceaccount.com`, or a bare
    /// OAuth client ID like `12345-abc.apps.googleusercontent.com`.
    Grant {
        /// Google Cloud project ID (e.g. `your-project-id`).
        #[arg(long)]
        project_id: String,
        /// Principal to allow past IAP. See command docstring for
        /// supported formats.
        #[arg(long)]
        member: String,
        /// Compute backend-service name. Defaults to `navigator-web`.
        #[arg(long, default_value = gcp::iap::DEFAULT_SERVICE_NAME)]
        service: String,
    },
}

#[derive(Subcommand)]
pub enum RestateCmd {
    /// Register the `workflows-service` worker with the configured
    /// Restate Cloud environment. Equivalent to
    /// `restate -y deployment register <url>` against
    /// `NAVIGATOR_WORKFLOWS_URL` (or the prod default).
    Register {
        /// Override the public worker URL. When unset, falls back to
        /// `NAVIGATOR_WORKFLOWS_URL`, then derives
        /// `https://workflows.<NAVIGATOR_PRIMARY_DOMAIN>/`, and only
        /// then the workspace placeholder
        /// (`https://workflows.example.com/`).
        #[arg(long)]
        url: Option<String>,
    },
}

/// Dispatch the orchestration subset of the `navigator` CLI — the commands
/// collapsed in from the former `devx` binary (cluster up/down, image builds,
/// deploy, e2e, GCP provisioning, ship, …). `main` loads `.env` +
/// `.devx/env` before parsing, so this only resolves the KIND config and
/// routes. Non-orchestration commands never reach here.
///
/// One big match over every subcommand — readability comes from the flat
/// dispatch, not from splitting it across helpers.
#[allow(clippy::too_many_lines)]
pub fn dispatch(command: crate::Command) -> Result<()> {
    let cfg = KindConfig::from_env();
    match command {
        crate::Command::StartDevServer => up(&cfg),
        crate::Command::Down => down(&cfg),
        crate::Command::Env => {
            print_env(&cfg);
            Ok(())
        }
        crate::Command::Status => {
            status(&cfg);
            Ok(())
        }
        crate::Command::KindUp => kind_up_only(&cfg),
        crate::Command::KindDown => kind_down_only(&cfg),
        crate::Command::Doctor { namespace } => {
            doctor::run(namespace.as_deref().unwrap_or(&cfg.namespace))
        }
        crate::Command::WorktreeEnv(cmd) => worktree_env::dispatch(cmd, &cfg),
        crate::Command::Deploy => deploy(&cfg, None),
        crate::Command::Undeploy => undeploy(&cfg),
        crate::Command::E2e => e2e::run_e2e(&cfg),
        crate::Command::GrantStaff => e2e::grant_staff(&cfg),
        crate::Command::Logs => logs(&cfg),
        crate::Command::KustomizeKind => kustomize_render(&cfg.full_overlay),
        crate::Command::KustomizeGke => kustomize_render(&cfg.gke_overlay),
        crate::Command::Gcp(GcpCmd::Setup {
            project_id,
            region,
            cluster_name,
            sql_instance,
            vpc_name,
            gateway_ip_name,
            config_sync_repo,
            config_sync_dir,
            dry_run,
        }) => {
            let mut config = gcp::SetupConfig::default();
            if let Some(v) = region {
                config.region = v;
            }
            if let Some(v) = cluster_name {
                config.cluster_name = v;
            }
            if let Some(v) = sql_instance {
                config.sql_instance = v;
            }
            if let Some(v) = vpc_name {
                config.vpc_name = v;
            }
            if let Some(v) = gateway_ip_name {
                config.gateway_ip_name = v;
            }
            if let Some(v) = config_sync_repo {
                config.config_sync_repo = Some(v);
            }
            if let Some(v) = config_sync_dir {
                config.config_sync_dir = v;
            }
            gcp_setup(project_id, dry_run, config)
        }
        crate::Command::Gcp(GcpCmd::Iap(IapCmd::Audience {
            project_id,
            service,
        })) => gcp_iap_audience(&project_id, &service),
        crate::Command::Gcp(GcpCmd::Iap(IapCmd::Grant {
            project_id,
            member,
            service,
        })) => gcp_iap_grant(&project_id, &service, &member),
        crate::Command::Restate(RestateCmd::Register { url }) => restate_register(url.as_deref()),
        crate::Command::Ship {
            dry_run,
            restart_only,
            tag,
        } => ship::run_ship(&ship::ShipOpts {
            dry_run,
            restart_only,
            tag,
        }),
        crate::Command::Dns(DnsCmd::Setup {
            domain,
            dkim_target1,
            dkim_target2,
            dry_run,
        }) => dns_setup(&domain, dkim_target1, dkim_target2, dry_run),
        crate::Command::Rebrand(cmd) => brand::run(cmd),
        crate::Command::Observability { dry_run } => {
            observability::run_observability(observability::ObservabilityOpts { dry_run })
        }
        // `main` only routes the orchestration subset here; the notation /
        // live-site commands (validate, import, login, …) are handled there.
        _ => unreachable!("devx::dispatch received a non-orchestration command"),
    }
}

/// `devx dns setup --domain <D> [--dry-run]`: ensure `SendGrid` mail
/// records on the zone via `DNSimple`. Builds a private Tokio runtime
/// because the DNS provider is async.
fn dns_setup(
    domain: &str,
    dkim_target1: Option<String>,
    dkim_target2: Option<String>,
    dry_run: bool,
) -> Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(async move {
        let mut provider = dns::DnsimpleProvider::from_env()?;
        if dry_run {
            provider = provider.with_dry_run();
        }
        let dkim_cnames: Vec<String> = [dkim_target1, dkim_target2].into_iter().flatten().collect();
        let report = dns::run_mail_setup(&provider, domain, &dkim_cnames).await?;
        for entry in &report {
            eprintln!(
                "==> {} {} \"{}\" : {:?}",
                entry.record_type.as_str(),
                if entry.name.is_empty() {
                    "(root)"
                } else {
                    entry.name.as_str()
                },
                domain,
                entry.outcome,
            );
        }
        if dry_run {
            eprintln!(
                "--- dry run: {} call(s) would be made ---",
                provider.recorded_calls().len()
            );
            for call in provider.recorded_calls() {
                eprintln!("{} {}", call.method, call.url);
                if let Some(body) = call.body {
                    eprintln!("  {body}");
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}

/// `devx gcp iap audience`: print the IAP audience string for
/// `IAP_AUDIENCE`. Requires the LB to already exist (apply the gke
/// overlay first); a 404 surfaces as a clear error.
fn gcp_iap_audience(project_id: &str, service: &str) -> Result<()> {
    use std::sync::Arc;

    use gcp::client::{GcpClient, TokenProvider};

    tracing_subscriber::fmt::try_init().ok();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(async move {
        let token: Arc<dyn TokenProvider> = gcp::auth::adc_token_provider().await?;
        let client = GcpClient::new(token);
        let project_number = gcp::iap::get_project_number(&client, project_id)
            .await
            .context("look up project number")?;
        let svc_id = gcp::iap::get_backend_service_id(&client, project_id, service)
            .await
            .with_context(|| {
                format!(
                    "look up backend service `{service}` (apply k8s/overlays/gke first \
and wait for the GKE Ingress controller to provision the LB)"
                )
            })?;
        let audience = gcp::iap::format_iap_audience(&project_number, &svc_id);
        // The audience string is the only thing the operator pastes
        // into web-env.yaml — print to stdout so it can be captured.
        println!("{audience}");
        eprintln!(
            "==> IAP_AUDIENCE for {service} in {project_id}: paste into \
k8s/overlays/gke/patches/web-env.yaml then kubectl apply -k k8s/overlays/gke"
        );
        Ok::<(), anyhow::Error>(())
    })
}

/// `devx gcp iap grant`: add a principal to `roles/iap.httpsResourceAccessor`
/// on the IAP-protected backend service. Safe to re-run — checks the
/// existing policy and skips setIamPolicy when the binding is already there.
fn gcp_iap_grant(project_id: &str, service: &str, member: &str) -> Result<()> {
    use std::sync::Arc;

    use gcp::client::{GcpClient, TokenProvider};
    use gcp::iap::BindingOutcome;

    tracing_subscriber::fmt::try_init().ok();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(async move {
        let token: Arc<dyn TokenProvider> = gcp::auth::adc_token_provider().await?;
        let client = GcpClient::new(token);
        let project_number = gcp::iap::get_project_number(&client, project_id)
            .await
            .context("look up project number")?;
        let outcome = gcp::iap::ensure_iap_iam_binding(&client, &project_number, service, member)
            .await
            .with_context(|| format!("bind {member} on {service}"))?;
        match outcome {
            BindingOutcome::Added => {
                eprintln!("==> added {member} to roles/iap.httpsResourceAccessor on {service}");
            }
            BindingOutcome::AlreadyPresent => {
                eprintln!("==> {member} already bound on {service} (no change)");
            }
        }
        Ok::<(), anyhow::Error>(())
    })
}

/// Resolve the public worker URL Restate Cloud dials, in precedence
/// order:
///   1. an explicit `--url` override,
///   2. `NAVIGATOR_WORKFLOWS_URL`,
///   3. derived from `NAVIGATOR_PRIMARY_DOMAIN` as
///      `https://workflows.<domain>/`,
///   4. the [`WORKFLOWS_PUBLIC_URL`] placeholder — only when none of the
///      above are set.
///
/// Step 3 is the hardening from the 2026-06-10 ship: an operator who has
/// a `NAVIGATOR_PRIMARY_DOMAIN` but never set the explicit workflows URL
/// now targets their real ingress instead of `workflows.example.com`,
/// which silently no-op'd the re-register. Pure (takes its inputs as
/// args) so it is unit-testable without mutating the process env.
pub(crate) fn resolve_workflows_url(
    url_override: Option<&str>,
    workflows_url_env: Option<&str>,
    primary_domain: Option<&str>,
) -> String {
    fn nonblank(v: &str) -> Option<String> {
        let t = v.trim();
        (!t.is_empty()).then(|| t.to_string())
    }
    url_override
        .and_then(nonblank)
        .or_else(|| workflows_url_env.and_then(nonblank))
        .or_else(|| {
            primary_domain
                .and_then(nonblank)
                .map(|d| format!("https://workflows.{d}/"))
        })
        .unwrap_or_else(|| WORKFLOWS_PUBLIC_URL.to_string())
}

/// `devx restate register [--url <URL>]`: register the `workflows-service`
/// worker with the caller's Restate Cloud environment. The URL is resolved
/// by [`resolve_workflows_url`] (override → `NAVIGATOR_WORKFLOWS_URL` →
/// `https://workflows.<NAVIGATOR_PRIMARY_DOMAIN>/` → placeholder).
///
/// Two transports, chosen by environment:
/// - When `RESTATE_ADMIN_URL` **and** `RESTATE_ADMIN_TOKEN` are both set
///   (the production / CI path, wired in Doppler `prd`), register via the
///   admin REST API. This is headless: it needs no `restate cloud env
///   configure` (which requires a TTY) and works with a non-expiring
///   admin-scoped API key, so an unattended `ship` from a fresh
///   machine re-registers without the SSO token or a configured CLI env.
/// - Otherwise shell out to the pinned `restate` CLI (the KIND dev loop and
///   operators who keep the `restate cloud login` SSO token fresh).
///
/// See [`docs/durable-workflows.md`] "step 7d".
fn restate_register(url_override: Option<&str>) -> Result<()> {
    let workflows_url_env = env::var("NAVIGATOR_WORKFLOWS_URL").ok();
    let primary_domain = env::var("NAVIGATOR_PRIMARY_DOMAIN").ok();
    let url = resolve_workflows_url(
        url_override,
        workflows_url_env.as_deref(),
        primary_domain.as_deref(),
    );

    let admin_url = env::var("RESTATE_ADMIN_URL").ok();
    let admin_token = env::var("RESTATE_ADMIN_TOKEN").ok();
    if let (Some(admin_url), Some(admin_token)) = (
        admin_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
        admin_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    ) {
        return register_via_admin_api(admin_url, admin_token, &url);
    }

    require_tools(&["restate"])?;
    check_restate_cli_version();
    eprintln!("==> restate -y deployments register {url}");
    run(Command::new("restate")
        .arg("-y")
        .arg("deployments")
        .arg("register")
        .arg(&url))?;
    eprintln!("==> restate -y deployments list");
    run(Command::new("restate")
        .arg("-y")
        .arg("deployments")
        .arg("list"))
}

/// Force-register the worker deployment via the Restate Cloud admin REST
/// API (`POST {admin}/deployments` with `force: true`), bearer-authenticated.
/// `force` re-runs discovery against the live worker, so every service it
/// exposes is (re)registered; the call is idempotent and safe on every ship.
fn register_via_admin_api(admin_base_url: &str, token: &str, worker_url: &str) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("create tokio runtime")?;
    runtime.block_on(register_via_admin_api_async(
        admin_base_url,
        token,
        worker_url,
    ))
}

async fn register_via_admin_api_async(
    admin_base_url: &str,
    token: &str,
    worker_url: &str,
) -> Result<()> {
    let endpoint = format!("{}/deployments", admin_base_url.trim_end_matches('/'));
    eprintln!("==> POST {endpoint} (force re-register {worker_url})");
    let body = serde_json::json!({ "uri": worker_url, "force": true });
    let resp = reqwest::Client::new()
        .post(&endpoint)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .context("POST to Restate Cloud admin /deployments")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("Restate admin register failed: HTTP {status}: {text}");
    }
    let names: Vec<String> = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("services").and_then(|s| s.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();
    eprintln!(
        "==> re-registered {worker_url} ({} services{}{})",
        names.len(),
        if names.is_empty() { "" } else { ": " },
        names.join(", ")
    );
    Ok(())
}

/// Warn (don't fail) if the on-PATH Restate CLI doesn't match the
/// pinned version. Drift between operator-laptop and CI is the
/// primary failure mode the pin guards against; surface it loudly
/// without blocking the operator who may be deliberately ahead.
fn check_restate_cli_version() {
    let Ok(out) = Command::new("restate").arg("--version").output() else {
        return;
    };
    let banner = String::from_utf8_lossy(&out.stdout);
    if !banner.contains(RESTATE_CLI_VERSION) {
        eprintln!(
            "warning: restate CLI on PATH is `{}`; devx pins {RESTATE_CLI_VERSION}",
            banner.trim()
        );
    }
}

/// `devx gcp setup --project-id <ID> [--dry-run]`: provision the GCP
/// resources `web` depends on. Builds a private Tokio runtime so the
/// rest of `devx` can stay sync — the entire `gcp` module is async
/// (it talks to GCP REST APIs).
fn gcp_setup(project_id: String, dry_run: bool, config: gcp::SetupConfig) -> Result<()> {
    use std::sync::Arc;

    use gcp::client::{GcpClient, StaticToken, TokenProvider};

    tracing_subscriber::fmt::try_init().ok();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    runtime.block_on(async move {
        // Dry-run uses a stub token — we never authenticate.
        let token: Arc<dyn TokenProvider> = if dry_run {
            Arc::new(StaticToken("dry-run".into()))
        } else {
            gcp::auth::adc_token_provider().await?
        };
        let mut client = GcpClient::new(token);
        if dry_run {
            client = client.with_dry_run();
        }
        gcp::run(&client, &project_id, &config).await?;
        if dry_run {
            eprintln!(
                "--- dry run: {} call(s) would be made ---",
                client.recorded_calls().len()
            );
            for call in client.recorded_calls() {
                eprintln!("{} {}", call.method, call.url);
                if let Some(body) = call.body {
                    eprintln!("  {body}");
                }
            }
        } else {
            tracing::info!(project = %project_id, "setup complete");
        }
        Ok::<(), anyhow::Error>(())
    })
}

// ---------- subcommands ----------

fn up(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kind", "kubectl", "docker", "helm"])?;
    let root = workspace_root()?;
    let state = StateDir::new(&root)?;

    kind_up_steps(&root, cfg)?;
    // The worker runs in-cluster; pull its published image from ghcr
    // instead of building it on the host. `web` runs on the host via
    // `cargo run -p web`, so it needs no image here.
    let owner = ghcr::owner_from_env();
    let tag = resolve_local_image_tag(&owner, None)?;
    pull_retag_load(
        &owner,
        "navigator-workflows-service",
        &tag,
        WORKFLOWS_SERVICE_IMAGE,
        cfg,
    )?;

    eprintln!("==> applying dependency manifests (skipping k8s/base/web)");
    apply_kustomize(&root, &cfg.deps_overlay)?;
    wait_for_dep_rollouts(cfg)?;

    eprintln!("==> opening port-forwards");
    state.kill_pids();
    let pids = vec![
        port_forward("svc/postgres", cfg.postgres_port, 5432, cfg, &state)?,
        // Restate lives in its own `restate` namespace (the Operator's
        // CR places the StatefulSet there). The other deps run in
        // `navigator`. Forward both Restate ports — ingress (8080) for
        // the workflow client, admin (9070) for `restate-cli` /
        // dashboard.
        port_forward_two_in(
            "restate",
            "svc/restate",
            cfg.restate_ingress_port,
            8080,
            cfg.restate_admin_port,
            9070,
            &state,
        )?,
        port_forward("svc/opa", cfg.opa_port, 8181, cfg, &state)?,
        // Grafana LGTM: forward the UI (3000) and the OTLP gRPC
        // ingest port (4317) together. The host-side `web` exports
        // telemetry to localhost:<otlp_port>; the operator browses
        // it at localhost:<grafana_port>.
        port_forward_two_in(
            &cfg.namespace,
            "svc/lgtm",
            cfg.grafana_port,
            3000,
            cfg.otlp_port,
            4317,
            &state,
        )?,
    ];
    state.write_pids(&pids)?;

    // Sanity check: probe each port the local `web` will use.
    wait_for_tcp("127.0.0.1", cfg.postgres_port)?;
    wait_for_tcp("127.0.0.1", cfg.restate_ingress_port)?;
    wait_for_tcp("127.0.0.1", cfg.opa_port)?;
    wait_for_tcp("127.0.0.1", cfg.keycloak_port)?;
    wait_for_tcp("127.0.0.1", cfg.fake_gcs_port)?;
    wait_for_tcp("127.0.0.1", cfg.otlp_port)?;

    state.write_env(&render_env(cfg))?;

    print_chrome_summary(cfg);
    Ok(())
}

/// `devx kind-up`: just create the cluster + install ingress +
/// install the Restate Operator. Don't apply application manifests.
fn kind_up_only(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kind", "kubectl", "helm"])?;
    let root = workspace_root()?;
    kind_up_steps(&root, cfg)
}

/// `devx kind-down`: delete the KIND cluster.
fn kind_down_only(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kind"])?;
    let cluster = &cfg.cluster;
    if cluster_exists(cluster)? {
        eprintln!("==> deleting KIND cluster '{cluster}'");
        run(Command::new("kind")
            .arg("delete")
            .arg("cluster")
            .arg("--name")
            .arg(cluster))?;
    } else {
        eprintln!("==> KIND cluster '{cluster}' not found; nothing to delete");
    }
    Ok(())
}

/// `devx deploy`: full in-cluster stack from published ghcr images.
/// Pulls both images at a resolved `YY.M.D` tag, retags + loads them
/// into KIND, applies every manifest under `k8s/`, waits for the
/// navigator-web rollout to settle. CI builds and publishes the images;
/// this pulls them. `tag_override` (e.g. `worktree-env --demo --tag`)
/// pins the release; else `NAVIGATOR_IMAGE_TAG`, else the latest
/// published tag is pulled.
fn deploy(cfg: &KindConfig, tag_override: Option<&str>) -> Result<()> {
    require_tools(&["kind", "kubectl", "docker", "helm"])?;
    let root = workspace_root()?;
    // `kind_up_steps` is idempotent — safe to call when the cluster
    // is already up. Establishes the Operator + nginx-ingress
    // invariants `deploy` relies on.
    kind_up_steps(&root, cfg)?;
    let owner = ghcr::owner_from_env();
    let tag = resolve_local_image_tag(&owner, tag_override)?;
    // Fail fast before any apply if either service image is missing the
    // resolved tag — a missing image would wedge the rollout in
    // ImagePullBackOff.
    ghcr::ensure_tag_published(&owner, "navigator-web", &tag)?;
    ghcr::ensure_tag_published(&owner, "navigator-workflows-service", &tag)?;
    pull_retag_load(&owner, "navigator-web", &tag, WEB_IMAGE, cfg)?;
    pull_retag_load(
        &owner,
        "navigator-workflows-service",
        &tag,
        WORKFLOWS_SERVICE_IMAGE,
        cfg,
    )?;
    apply_kustomize(&root, &cfg.full_overlay)?;
    eprintln!("==> waiting for navigator-web rollout");
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg(&cfg.namespace)
        .arg("rollout")
        .arg("status")
        .arg("deployment/navigator-web")
        .arg("--timeout=300s"))
}

/// `devx undeploy`: kubectl delete namespace navigator. Does NOT
/// touch the cluster — use `devx kind-down` for that.
fn undeploy(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kubectl"])?;
    run(Command::new("kubectl")
        .arg("delete")
        .arg("--ignore-not-found")
        .arg("namespace")
        .arg(&cfg.namespace))
}

/// `devx logs`: tail navigator-web logs.
fn logs(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kubectl"])?;
    use_kind_context(cfg)?;
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg(&cfg.namespace)
        .arg("logs")
        .arg("-f")
        .arg("deployment/navigator-web")
        .arg("-c")
        .arg("web"))
}

/// Switch kubectl to the KIND context for this cluster before any
/// cluster-mutating apply. `devx` is KIND-only; a stale GKE/EKS
/// context being current is otherwise indistinguishable from a fresh
/// KIND boot and would land manifests in the wrong place.
fn use_kind_context(cfg: &KindConfig) -> Result<()> {
    let context = format!("kind-{}", cfg.cluster);
    let out = Command::new("kubectl")
        .args(["config", "get-contexts", "-o", "name"])
        .output()
        .with_context(|| "kubectl config get-contexts failed")?;
    if !out.status.success() {
        anyhow::bail!(
            "kubectl config get-contexts failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let contexts = String::from_utf8_lossy(&out.stdout);
    if !contexts.lines().any(|c| c == context) {
        anyhow::bail!(
            "kubectl context '{context}' not found; bring the KIND cluster up first \
             (`devx kind-up`)."
        );
    }
    eprintln!("==> pinning kubectl context → {context}");
    run(Command::new("kubectl").args(["config", "use-context", &context]))
}

/// `devx kustomize-kind` / `devx kustomize-gke`: render a Kustomize
/// overlay to stdout. Inspect what `kubectl apply -k` would send
/// before sending it.
fn kustomize_render(overlay: &str) -> Result<()> {
    require_tools(&["kubectl"])?;
    let root = workspace_root()?;
    run(Command::new("kubectl")
        .arg("kustomize")
        .arg(root.join(overlay)))
}

// ---------- shared helpers ----------

/// Idempotent cluster bring-up: `kind create cluster` (if missing),
/// `kubectl apply` the nginx-ingress manifest, then `helm install`
/// the Restate Operator. Safe to re-invoke.
fn kind_up_steps(root: &Path, cfg: &KindConfig) -> Result<()> {
    let cluster = &cfg.cluster;
    if cluster_exists(cluster)? {
        eprintln!("==> KIND cluster '{cluster}' already exists, reusing");
    } else {
        eprintln!("==> creating KIND cluster '{cluster}'");
        let config_path = kind_config_path(root, cfg)?;
        run(Command::new("kind")
            .arg("create")
            .arg("cluster")
            .arg("--name")
            .arg(cluster)
            .arg("--config")
            .arg(&config_path))?;
    }
    // Pin kubectl to the KIND context before any apply. Without this,
    // a stale prod context (e.g. GKE) would receive the KIND-only
    // ingress-nginx manifest and quietly land RBAC + admission
    // webhooks where they don't belong.
    use_kind_context(cfg)?;

    eprintln!("==> installing nginx-ingress");
    run(Command::new("kubectl")
        .arg("apply")
        .arg("-f")
        .arg(INGRESS_MANIFEST))?;
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg("ingress-nginx")
        .arg("wait")
        .arg("--for=condition=ready")
        .arg("pod")
        .arg("--selector=app.kubernetes.io/component=controller")
        .arg("--timeout=180s"))?;

    eprintln!("==> installing Restate Operator (chart v{RESTATE_OPERATOR_VERSION})");
    run(Command::new("helm")
        .arg("upgrade")
        .arg("--install")
        .arg("restate-operator")
        .arg("oci://ghcr.io/restatedev/restate-operator-helm")
        .arg("--version")
        .arg(RESTATE_OPERATOR_VERSION)
        .arg("--namespace")
        .arg("restate-operator")
        .arg("--create-namespace"))?;
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg("restate-operator")
        .arg("wait")
        .arg("--for=condition=available")
        .arg("--timeout=180s")
        .arg("deployment")
        .arg("--all"))
}

/// Load a local Docker image tag into every KIND node.
///
/// `kind load docker-image` hardcodes `ctr images import --all-platforms
/// --digests`, which means it tries to import *every* manifest the local
/// image index references. Since CI began publishing multi-arch indexes
/// (`linux/amd64` + `linux/arm64` + buildx `unknown/unknown` attestation
/// manifests), that breaks on any single-arch host: `docker pull` only
/// materializes the host platform's blobs, so `--all-platforms` aborts with
/// `ctr: content digest <other-arch manifest>: not found`. `OrbStack` and
/// Docker 29 use the containerd image store, which keeps the full index, so
/// this is the *default* failure on Apple Silicon — not an opt-in misconfig.
///
/// Flatten to the daemon's own platform with `docker save --platform` first,
/// then `kind load image-archive` the single-platform tar — `--all-platforms`
/// then finds exactly one platform and succeeds.
///
/// Only a *failed `docker save`* triggers the legacy fallback: a save that
/// can't produce `<platform>` means the image is older/single-arch, where
/// `kind load docker-image` still works (one platform, nothing to mismatch).
/// A failure of the `kind load image-archive` step is deliberately *not*
/// caught — KIND being unreachable or out of disk would fail the legacy load
/// the same way, and on a multi-arch image the fallback would re-trigger the
/// very `--all-platforms` digest bug this exists to avoid. That error
/// propagates directly instead of hiding behind a "flatten failed" message.
fn kind_load_image_into_cluster(tag: &str, cfg: &KindConfig) -> Result<()> {
    let platform = format!("linux/{}", docker_daemon_arch());
    let archive = tempfile::Builder::new()
        .prefix("navigator-kind-load-")
        .suffix(".tar")
        .tempfile()
        .context("create temp image archive for kind load")?;
    eprintln!("==> docker save --platform {platform} {tag} (single-platform for kind)");
    let saved = run(Command::new("docker")
        .arg("save")
        .arg("--platform")
        .arg(&platform)
        .arg(tag)
        .arg("-o")
        .arg(archive.path()));
    if let Err(err) = saved {
        eprintln!(
            "==> `docker save --platform {platform}` failed ({err:#}); \
             falling back to `kind load docker-image` (older single-arch image?)"
        );
        return run(Command::new("kind")
            .arg("load")
            .arg("docker-image")
            .arg(tag)
            .arg("--name")
            .arg(&cfg.cluster));
    }
    eprintln!("==> kind load image-archive ({tag} → {})", cfg.cluster);
    run(Command::new("kind")
        .arg("load")
        .arg("image-archive")
        .arg(archive.path())
        .arg("--name")
        .arg(&cfg.cluster))
}

/// Architecture the Docker daemon runs as (`arm64` / `amd64`), used to build
/// the `linux/<arch>` platform passed to `docker save`. Queries the daemon
/// directly (`docker version`) so it is correct even when the CLI binary runs
/// under a different arch (Rosetta); falls back to the host arch if the daemon
/// can't be reached.
fn docker_daemon_arch() -> String {
    Command::new("docker")
        .args(["version", "--format", "{{.Server.Arch}}"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|arch| !arch.is_empty())
        .unwrap_or_else(|| normalize_docker_arch(std::env::consts::ARCH))
}

/// Map a Rust `std::env::consts::ARCH` value to the Docker/OCI arch suffix.
/// Only the two arches the workspace builds for are remapped; anything else
/// passes through unchanged.
fn normalize_docker_arch(arch: &str) -> String {
    match arch {
        "x86_64" => "amd64".to_string(),
        "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    }
}

/// Resolve the `YY.M.D` ghcr tag the local cluster should pull, in
/// precedence order: an explicit `override_tag` (e.g. `worktree-env
/// --demo --tag`), then `NAVIGATOR_IMAGE_TAG`, then the latest published
/// tag from ghcr. CI builds and publishes the images (`deploy.yml`); the
/// local loop pulls them.
fn resolve_local_image_tag(owner: &str, override_tag: Option<&str>) -> Result<String> {
    if let Some(tag) = override_tag
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| {
            env::var("NAVIGATOR_IMAGE_TAG")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
    {
        ghcr::validate_release_tag(&tag)?;
        return Ok(tag);
    }
    ghcr::resolve_latest_tag(owner, "navigator-web")
}

/// Pull a published ghcr image, retag it to the local `:dev` tag the KIND
/// manifests reference, and load it into the cluster. Retagging (rather
/// than rewriting every manifest to a ghcr ref) keeps the overlays
/// byte-identical whether an image was historically built or is now
/// pulled. `docker pull` selects the host-arch variant from the
/// multi-arch manifest, so an Apple-Silicon laptop loads a native arm64
/// image into its arm64 KIND node.
fn pull_retag_load(
    owner: &str,
    image: &str,
    tag: &str,
    dev_tag: &str,
    cfg: &KindConfig,
) -> Result<()> {
    let remote = ghcr::image_ref(owner, image, tag);
    eprintln!("==> docker pull {remote}");
    run(Command::new("docker").arg("pull").arg(&remote))?;
    run(Command::new("docker").arg("tag").arg(&remote).arg(dev_tag))?;
    kind_load_image_into_cluster(dev_tag, cfg)
}

/// Path to the `kind create cluster --config` file. At default
/// keycloak/fake-gcs ports this is the committed `k8s/kind-config.yaml`
/// verbatim (so a standalone `kind create` against it still works).
/// When either host port is overridden, render a temp copy under
/// `.devx/` with only the two `hostPort:` lines substituted.
fn kind_config_path(root: &Path, cfg: &KindConfig) -> Result<PathBuf> {
    let committed = root.join("k8s/kind-config.yaml");
    if cfg.keycloak_port == DEFAULT_KEYCLOAK_HOST_PORT
        && cfg.fake_gcs_port == DEFAULT_FAKE_GCS_HOST_PORT
    {
        return Ok(committed);
    }
    let template =
        fs::read_to_string(&committed).with_context(|| format!("read {}", committed.display()))?;
    let rendered = render_kind_config(&template, cfg);
    let dir = root.join(".devx");
    fs::create_dir_all(&dir).with_context(|| format!("create state dir {}", dir.display()))?;
    let path = dir.join("kind-config.yaml");
    fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
    eprintln!(
        "==> rendered kind-config with keycloak={} fake-gcs={} host ports → {}",
        cfg.keycloak_port,
        cfg.fake_gcs_port,
        path.display()
    );
    Ok(path)
}

/// Substitute the keycloak / fake-gcs `hostPort:` values in a
/// `kind-config.yaml` body. Only those two host ports are touched;
/// the `containerPort` (the Service's fixed `NodePort`) is preserved, so
/// the Service manifests stay in sync. At default ports the output is
/// byte-identical to the input.
fn render_kind_config(template: &str, cfg: &KindConfig) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(template.lines().count());
    // The `hostPort:` line follows its `containerPort:` line; track
    // which mapping we're inside so we rewrite the right host port.
    let mut pending: Option<u16> = None;
    for line in template.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- containerPort:") {
            pending = if trimmed.contains(&format!("containerPort: {DEFAULT_KEYCLOAK_HOST_PORT}")) {
                Some(cfg.keycloak_port)
            } else if trimmed.contains(&format!("containerPort: {DEFAULT_FAKE_GCS_HOST_PORT}")) {
                Some(cfg.fake_gcs_port)
            } else {
                None
            };
            lines.push(line.to_string());
        } else if trimmed.starts_with("hostPort:") {
            match pending.take() {
                Some(port) => {
                    let indent = &line[..line.len() - trimmed.len()];
                    lines.push(format!("{indent}hostPort: {port}"));
                }
                None => lines.push(line.to_string()),
            }
        } else {
            lines.push(line.to_string());
        }
    }
    let mut out = lines.join("\n");
    if template.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn apply_kustomize(root: &Path, overlay: &str) -> Result<()> {
    eprintln!("==> kubectl apply -k {overlay}");
    run(Command::new("kubectl")
        .arg("apply")
        .arg("-k")
        .arg(root.join(overlay)))
}

fn wait_for_dep_rollouts(cfg: &KindConfig) -> Result<()> {
    eprintln!("==> waiting for rollouts");
    for dep in ["postgres", "fake-gcs-server", "keycloak", "opa", "lgtm"] {
        wait_rollout("deployment", dep, cfg)?;
    }
    // Restate runs in its own `restate` namespace and the Operator
    // names the underlying StatefulSet from the CR spec, not literally
    // "restate". `workflows-service` is also a Restate Operator CR
    // (RestateDeployment), not a plain Deployment. Wait on each CR's
    // own `Ready` condition — that's the contract the Operator
    // exposes.
    wait_for_condition("restate", "restatecluster/restate", "Ready")?;
    wait_for_condition(
        &cfg.namespace,
        "restatedeployment/workflows-service",
        "Ready",
    )
}

fn wait_for_condition(namespace: &str, resource: &str, condition: &str) -> Result<()> {
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg(namespace)
        .arg("wait")
        .arg(format!("--for=condition={condition}"))
        .arg(resource)
        .arg("--timeout=300s"))
}

fn down(cfg: &KindConfig) -> Result<()> {
    let root = workspace_root()?;
    let state = StateDir::new(&root)?;
    eprintln!("==> killing port-forwards");
    state.kill_pids();
    let cluster = &cfg.cluster;
    if cluster_exists(cluster)? {
        eprintln!("==> deleting KIND cluster '{cluster}'");
        run(Command::new("kind")
            .arg("delete")
            .arg("cluster")
            .arg("--name")
            .arg(cluster))?;
    }
    state.clear();
    Ok(())
}

fn print_env(cfg: &KindConfig) {
    print!("{}", render_env(cfg));
}

fn status(cfg: &KindConfig) {
    let cluster = cluster_exists(&cfg.cluster).unwrap_or(false);
    println!("KIND cluster '{}': {}", cfg.cluster, yes_no(cluster));
    if let Ok(root) = workspace_root() {
        if let Ok(state) = StateDir::new(&root) {
            let pids = state.read_pids().unwrap_or_default();
            println!("Port-forward PIDs ({}): {pids:?}", pids.len());
            for &port in &[
                cfg.postgres_port,
                cfg.restate_ingress_port,
                cfg.restate_admin_port,
                cfg.opa_port,
                cfg.keycloak_port,
                cfg.fake_gcs_port,
            ] {
                let listening = std::net::TcpStream::connect_timeout(
                    &format!("127.0.0.1:{port}").parse().unwrap(),
                    Duration::from_millis(200),
                )
                .is_ok();
                println!("  127.0.0.1:{port}: {}", yes_no(listening));
            }
        }
    }
}

// ---------- helpers ----------

fn render_env(cfg: &KindConfig) -> String {
    render_env_for(cfg, "navigator", cfg.web_port)
}

/// Like [`render_env`] but parameterized by the Postgres database name and
/// the host `web` port. The default dev loop uses the `navigator` database
/// on `cfg.web_port`; `worktree-env` threads a per-worktree database
/// (`navigator_<slug>`) and a per-worktree port through the same renderer
/// so the two paths can never drift in how they wire `web` to the deps.
fn render_env_for(cfg: &KindConfig, db_name: &str, web_port: u16) -> String {
    // NAVIGATOR_SEED_FILE is intentionally omitted: the canonical
    // seed lives in `store/seeds/*.yaml` as one file per entity,
    // not the single base.yaml the in-cluster Deployment `ConfigMap`
    // mounts. `web` skips seeding when the env var is unset and
    // `cargo run -p navigator -- seed …` can be used to populate.
    //
    // `RESTATE_BROKER_URL` is what `workflows::RestateRuntime::from_env`
    // reads. When set, the host-side `web` binary signals the
    // in-cluster `workflows-service` worker through the port-forwarded
    // Restate ingress; the worker journals each transition to the
    // shared Postgres (visible at `postgres://…@localhost:15432/…`,
    // queryable from psql on the host).
    // Host-side `web` runs `enforce_prod_invariants` unconditionally
    // (no APP_ENV gate after the SQLite cutover). SENDGRID_API_KEY /
    // SENDGRID_INBOUND_SECRET stubs are required for the binary to
    // boot; the actual email backend defaults to CapturingEmail
    // because NAVIGATOR_EMAIL_BACKEND is unset.
    let lines: [(&str, String); 15] = [
        ("PORT", web_port.to_string()),
        (
            "DATABASE_URL",
            format!(
                "postgres://navigator:navigator@localhost:{}/{db_name}",
                cfg.postgres_port
            ),
        ),
        ("NAVIGATOR_STORAGE_BACKEND", "gcs".into()),
        (
            "NAVIGATOR_STORAGE_ENDPOINT",
            format!("http://localhost:{}", cfg.fake_gcs_port),
        ),
        ("NAVIGATOR_STORAGE_BUCKET", "navigator".into()),
        (
            "OAUTH_ISSUER_URL",
            format!(
                "http://localhost:{}/keycloak/realms/navigator",
                cfg.keycloak_port
            ),
        ),
        ("OAUTH_CLIENT_ID", "navigator-web".into()),
        ("OAUTH_CLIENT_SECRET", "navigator-web-secret".into()),
        (
            "OAUTH_REDIRECT_URI",
            format!("http://localhost:{web_port}/auth/callback"),
        ),
        (
            "SESSION_SECRET",
            "dev-only-session-secret-change-in-prod".into(),
        ),
        (
            "NAVIGATOR_OPA_URL",
            format!("http://localhost:{}", cfg.opa_port),
        ),
        (
            "RESTATE_BROKER_URL",
            format!("http://localhost:{}", cfg.restate_ingress_port),
        ),
        ("SENDGRID_API_KEY", "SG.kind-stub".into()),
        ("SENDGRID_INBOUND_SECRET", "kind-stub".into()),
        // Flip host-side `web` from stdout-only logs to JSON logs +
        // OTLP export of traces/metrics/logs to the in-cluster
        // Grafana LGTM sink (port-forwarded to the host). Setting
        // this is exactly what `telemetry::init` keys off. To run
        // `web` with plain stdout logs and no export, set
        // `OTEL_EXPORTER_OTLP_ENDPOINT=` (empty) in `.env`, which is
        // loaded first and wins.
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            format!("http://localhost:{}", cfg.otlp_port),
        ),
    ];
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Generated by `devx up`. Do not edit by hand — your edits are\n\
         # overwritten on the next `devx up`. Persistent / hand-edited\n\
         # values belong in `.env` at the workspace root, which is\n\
         # auto-loaded BEFORE this file by every binary's `main()`, so\n\
         # `.env` always wins on collisions.\n",
    );
    for (k, v) in lines {
        let _ = writeln!(out, "{k}={v}");
    }
    out
}

fn print_chrome_summary(cfg: &KindConfig) {
    let web = cfg.web_port;
    eprintln!();
    eprintln!("===========================================================");
    eprintln!(" devx up — full Neon Law Navigator stack running in KIND");
    eprintln!("===========================================================");
    eprintln!();
    eprintln!("Start the web server on the host:");
    eprintln!();
    eprintln!("    set -a; source .devx/env; set +a");
    eprintln!("    cargo run -p web");
    eprintln!();
    eprintln!("Walk the retainer in Chrome:");
    eprintln!("  http://localhost:{web}                    — navigator home");
    eprintln!("  http://localhost:{web}/auth/login?return_to=/portal  — OIDC flow");
    eprintln!("  http://localhost:{web}/portal/admin/retainers/new — start a stepwise walk");
    eprintln!();
    eprintln!("Inspect the workflow journal directly from the host:");
    eprintln!();
    eprintln!(
        "    psql postgres://navigator:navigator@localhost:{}/navigator \\",
        cfg.postgres_port
    );
    eprintln!("        -c 'select id, machine_kind, from_state, to_state, condition, payload");
    eprintln!("            from notation_events order by id'");
    eprintln!();
    eprintln!("Other admin UIs:");
    eprintln!(
        "  http://localhost:{}                  — Keycloak admin (admin/admin)",
        cfg.keycloak_port
    );
    eprintln!(
        "  http://localhost:{}/storage/v1/b      — fake-gcs-server buckets",
        cfg.fake_gcs_port
    );
    eprintln!(
        "  http://localhost:{}/services         — Restate admin (registered services)",
        cfg.restate_admin_port
    );
    eprintln!(
        "  http://localhost:{}                   — Grafana LGTM (Explore: Loki logs, Tempo",
        cfg.grafana_port
    );
    eprintln!(
        "                                          traces, Prometheus metrics by service.name)"
    );
    eprintln!();
    eprintln!(
        "Telemetry: host `web` exports to the LGTM OTLP sink at \
         http://localhost:{} (set in .devx/env).",
        cfg.otlp_port
    );
    eprintln!(
        "Open Grafana → Explore, pick the Tempo datasource, and search service.name=navigator-web."
    );
    eprintln!();
    eprintln!("Tear down with: cargo run --release -p devx -- down");
    eprintln!();
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn wait_rollout(kind: &str, name: &str, cfg: &KindConfig) -> Result<()> {
    run(Command::new("kubectl")
        .arg("--namespace")
        .arg(&cfg.namespace)
        .arg("rollout")
        .arg("status")
        .arg(format!("{kind}/{name}"))
        .arg("--timeout=300s"))
}

fn port_forward(
    target: &str,
    host_port: u16,
    svc_port: u16,
    cfg: &KindConfig,
    state: &StateDir,
) -> Result<u32> {
    spawn_port_forward(
        &[
            "--namespace",
            &cfg.namespace,
            "port-forward",
            target,
            &format!("{host_port}:{svc_port}"),
        ],
        state,
    )
}

fn port_forward_two_in(
    namespace: &str,
    target: &str,
    host_a: u16,
    svc_a: u16,
    host_b: u16,
    svc_b: u16,
    state: &StateDir,
) -> Result<u32> {
    spawn_port_forward(
        &[
            "--namespace",
            namespace,
            "port-forward",
            target,
            &format!("{host_a}:{svc_a}"),
            &format!("{host_b}:{svc_b}"),
        ],
        state,
    )
}

fn spawn_port_forward(args: &[&str], state: &StateDir) -> Result<u32> {
    let log_offset = state.log_size();
    let log = state.open_log()?;
    let log_err = log.try_clone()?;
    let mut cmd = Command::new("kubectl");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    detach(&mut cmd);
    let child: Child = cmd
        .spawn()
        .with_context(|| format!("spawn kubectl {args:?}"))?;
    let pid = child.id();
    // Detach: don't wait on it. `Child::drop` does not kill the
    // process; the OS adopts it via the new process group.
    std::mem::forget(child);

    // Give kubectl a moment to either bind or fail, then scan the log
    // it appended. Without this check, a `bind: address already in
    // use` error is hidden by anything else that happens to be
    // listening on that port.
    std::thread::sleep(Duration::from_millis(800));
    if let Some(err) = state.log_tail_error(log_offset)? {
        bail!("kubectl port-forward {args:?} failed: {err}");
    }
    Ok(pid)
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn detach(_cmd: &mut Command) {}

fn wait_for_tcp(host: &str, port: u16) -> Result<()> {
    let addr = format!("{host}:{port}");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(stream) = std::net::TcpStream::connect_timeout(
            &addr
                .parse()
                .with_context(|| format!("parse socket addr {addr}"))?,
            Duration::from_millis(500),
        ) {
            drop(stream);
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for {addr} to accept connections");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn cluster_exists(name: &str) -> Result<bool> {
    let out = Command::new("kind")
        .arg("get")
        .arg("clusters")
        .output()
        .context("run `kind get clusters`")?;
    if !out.status.success() {
        bail!(
            "kind get clusters failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.trim() == name))
}

fn require_tools(tools: &[&str]) -> Result<()> {
    for tool in tools {
        let ok = Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {tool}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            bail!("required tool not on PATH: {tool}");
        }
    }
    Ok(())
}

/// Verify that auth-bearing CLIs are not just present but **authenticated**.
///
/// A present-but-unauthenticated `gcloud` / `doppler` / `restate` doesn't
/// fail until deep in a flow — mid-push to Artifact Registry, mid-Restate
/// re-register — where the error is cryptic and a half-finished ship is
/// already on the cluster. This runs the cheapest read-only probe per CLI
/// up front so the whole flow aborts in one clear line before it touches
/// anything. Each probe's output is discarded; only its exit status counts.
fn require_auth(tools: &[&str]) -> Result<()> {
    for &tool in tools {
        // (probe command, what to run to fix it)
        let (probe, hint) = match tool {
            // Prints an access token iff a credential is active; non-zero
            // when logged out or no ADC / service account is available.
            "gcloud" => (
                "gcloud auth print-access-token",
                "run `gcloud auth login` (or activate a service account)",
            ),
            // `doppler me` resolves the caller from the token; fails when no
            // token is configured for this workspace.
            "doppler" => (
                "doppler me",
                "run `doppler login` (then `doppler setup` in this repo)",
            ),
            // `whoami` succeeds only when an environment is configured. This
            // catches "never logged in"; a stale Cloud token can still slip
            // through, so the register step remains the real proof.
            "restate" => (
                "restate whoami",
                "run `restate cloud login` then `restate cloud environments configure <env>`",
            ),
            // The daemon must be reachable to build/push images.
            "docker" => ("docker info", "start the Docker daemon"),
            other => bail!("require_auth: no auth probe defined for `{other}`"),
        };
        eprintln!("==> auth check: {tool}");
        let ok = Command::new("sh")
            .arg("-c")
            .arg(probe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            bail!("`{tool}` is present but not authenticated — {hint}");
        }
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let program = Path::new(cmd.get_program()).display().to_string();
    let status = cmd.status().with_context(|| format!("spawn {program}"))?;
    if !status.success() {
        bail!("command failed ({status}): {program}");
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let mut dir = env::current_dir().context("get current directory")?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("k8s").is_dir() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => bail!("could not find workspace root containing Cargo.toml and k8s/"),
        }
    }
}

// ---------- state directory (.devx/) ----------

struct StateDir {
    dir: PathBuf,
}

impl StateDir {
    fn new(root: &Path) -> Result<Self> {
        let dir = root.join(".devx");
        fs::create_dir_all(&dir).with_context(|| format!("create state dir {}", dir.display()))?;
        Ok(Self { dir })
    }

    fn pids_path(&self) -> PathBuf {
        self.dir.join("pids")
    }

    fn env_path(&self) -> PathBuf {
        self.dir.join("env")
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("port-forwards.log")
    }

    fn open_log(&self) -> Result<fs::File> {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
            .with_context(|| format!("open {}", self.log_path().display()))
    }

    fn log_size(&self) -> u64 {
        fs::metadata(self.log_path()).map_or(0, |m| m.len())
    }

    /// Return the first error-shaped line appended to the log past
    /// `since`, or None if nothing notable showed up. "Error-shaped"
    /// means kubectl's `error:` or `Unable to listen` prefixes — the
    /// happy path emits `Forwarding from …`.
    fn log_tail_error(&self, since: u64) -> Result<Option<String>> {
        let path = self.log_path();
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let tail = bytes
            .get(usize::try_from(since).unwrap_or(usize::MAX)..)
            .unwrap_or(&[]);
        let text = String::from_utf8_lossy(tail);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("error:") || trimmed.starts_with("Unable to listen") {
                return Ok(Some(trimmed.to_string()));
            }
        }
        Ok(None)
    }

    fn write_pids(&self, pids: &[u32]) -> Result<()> {
        let mut f = fs::File::create(self.pids_path())
            .with_context(|| format!("write {}", self.pids_path().display()))?;
        for pid in pids {
            writeln!(f, "{pid}")?;
        }
        Ok(())
    }

    fn read_pids(&self) -> Result<Vec<u32>> {
        let path = self.pids_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let f = fs::File::open(&path).with_context(|| format!("read {}", path.display()))?;
        let mut out = Vec::new();
        for line in BufReader::new(f).lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push(
                trimmed
                    .parse::<u32>()
                    .map_err(|e| anyhow!("malformed PID '{trimmed}': {e}"))?,
            );
        }
        Ok(out)
    }

    fn kill_pids(&self) {
        let pids = self.read_pids().unwrap_or_default();
        for pid in pids {
            // Best-effort: process may already have exited.
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = fs::remove_file(self.pids_path());
    }

    fn write_env(&self, body: &str) -> Result<()> {
        fs::write(self.env_path(), body)
            .with_context(|| format!("write {}", self.env_path().display()))
    }

    fn clear(&self) {
        let _ = fs::remove_file(self.pids_path());
        let _ = fs::remove_file(self.env_path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Every workspace member crate must be `COPY`'d into each Containerfile that
    /// runs `cargo build` against the whole workspace — otherwise cargo can't
    /// load the workspace manifest and the image build dies with
    /// `failed to read <member>/Cargo.toml`. This is the exact failure that
    /// took every `*-trigger` image (and the nightly archives email) down when
    /// the `forms` crate was added without updating the COPY lists. This test
    /// is the guard that bug had no test for. `Containerfile.redirect` builds only
    /// the `cloud` bin in a trimmed context and is intentionally excluded.
    #[test]
    fn every_workspace_member_is_copied_into_each_workspace_image() {
        use std::path::Path;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root is cli/'s parent")
            .to_path_buf();
        let cargo = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
        let start = cargo.find("members = [").expect("members array present");
        let end = start + cargo[start..].find(']').expect("members array closes");
        let members: Vec<String> = cargo[start..end]
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('"'))
            .map(|l| l.trim_matches(|c| c == '"' || c == ',').to_string())
            .filter(|m| !m.is_empty())
            .collect();
        assert!(
            members.contains(&"telemetry".to_string()) && members.contains(&"forms".to_string()),
            "sanity: parsed the members array ({} members)",
            members.len()
        );
        for df in [
            "images/Containerfile.trigger",
            "images/Containerfile.workflows-service",
            "images/Containerfile.web",
        ] {
            let content = std::fs::read_to_string(root.join(df)).expect("read Containerfile");
            for m in &members {
                assert!(
                    content.contains(&format!("COPY {m} ")),
                    "{df} is missing `COPY {m} {m}` — a fresh image build will fail to load the \
                     workspace manifest. Add the COPY line (this is the failure mode that broke \
                     the trigger images and stopped the nightly archives email)."
                );
            }
        }
    }

    // The `linux/<arch>` platform fed to `docker save` (in
    // `kind_load_image_into_cluster`) must use the OCI arch suffix, not the
    // Rust arch triple — `linux/aarch64` would never match an image's
    // `linux/arm64` manifest, defeating the multi-arch flatten.
    #[test]
    fn normalize_docker_arch_maps_rust_arches_to_oci_suffixes() {
        assert_eq!(normalize_docker_arch("x86_64"), "amd64");
        assert_eq!(normalize_docker_arch("aarch64"), "arm64");
        // Already-normalized or unknown values pass through unchanged.
        assert_eq!(normalize_docker_arch("amd64"), "amd64");
        assert_eq!(normalize_docker_arch("arm64"), "arm64");
        assert_eq!(normalize_docker_arch("riscv64"), "riscv64");
    }

    // Every env var `KindConfig::from_env` reads. Tests clear all of
    // them before asserting so a stray value from the developer's shell
    // (or a prior test) can't leak in.
    const KIND_ENV_VARS: &[&str] = &[
        "NAVIGATOR_KIND_CLUSTER",
        "NAVIGATOR_K8S_NAMESPACE",
        "NAVIGATOR_KIND_DEPS_OVERLAY",
        "NAVIGATOR_KIND_OVERLAY",
        "NAVIGATOR_GKE_OVERLAY",
        "NAVIGATOR_KIND_POSTGRES_PORT",
        "NAVIGATOR_KIND_RESTATE_INGRESS_PORT",
        "NAVIGATOR_KIND_RESTATE_ADMIN_PORT",
        "NAVIGATOR_KIND_OPA_PORT",
        "NAVIGATOR_KIND_KEYCLOAK_PORT",
        "NAVIGATOR_KIND_FAKE_GCS_PORT",
        "NAVIGATOR_KIND_WEB_PORT",
        "NAVIGATOR_KIND_GRAFANA_PORT",
        "NAVIGATOR_KIND_OTLP_PORT",
    ];

    // Process env is global; `from_env` reads all of it. Serialize the
    // env-mutating tests so they don't race each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn clear_kind_env() {
        for key in KIND_ENV_VARS {
            env::remove_var(key);
        }
    }

    /// A `KindConfig` at its defaults, built without touching the
    /// process environment — for the pure render tests.
    fn default_cfg() -> KindConfig {
        KindConfig {
            cluster: DEFAULT_CLUSTER_NAME.into(),
            namespace: DEFAULT_NAMESPACE.into(),
            deps_overlay: DEFAULT_KUSTOMIZE_KIND_DEPS.into(),
            full_overlay: DEFAULT_KUSTOMIZE_KIND.into(),
            gke_overlay: DEFAULT_KUSTOMIZE_GKE.into(),
            postgres_port: DEFAULT_POSTGRES_HOST_PORT,
            restate_ingress_port: DEFAULT_RESTATE_INGRESS_HOST_PORT,
            restate_admin_port: DEFAULT_RESTATE_ADMIN_HOST_PORT,
            opa_port: DEFAULT_OPA_HOST_PORT,
            keycloak_port: DEFAULT_KEYCLOAK_HOST_PORT,
            fake_gcs_port: DEFAULT_FAKE_GCS_HOST_PORT,
            web_port: DEFAULT_LOCAL_WEB_PORT,
            grafana_port: DEFAULT_LGTM_GRAFANA_HOST_PORT,
            otlp_port: DEFAULT_LGTM_OTLP_HOST_PORT,
        }
    }

    #[test]
    fn resolve_workflows_url_prefers_explicit_override() {
        assert_eq!(
            resolve_workflows_url(
                Some("https://flag.example/"),
                Some("https://env.example/"),
                Some("neonlaw.com"),
            ),
            "https://flag.example/"
        );
    }

    #[test]
    fn resolve_workflows_url_falls_back_to_env() {
        assert_eq!(
            resolve_workflows_url(None, Some("https://env.example/"), Some("neonlaw.com")),
            "https://env.example/"
        );
    }

    #[test]
    fn resolve_workflows_url_derives_from_primary_domain() {
        // The 2026-06-10 hardening: domain set, explicit URL unset →
        // target the real ingress, never the placeholder.
        assert_eq!(
            resolve_workflows_url(None, None, Some("neonlaw.com")),
            "https://workflows.neonlaw.com/"
        );
    }

    #[test]
    fn resolve_workflows_url_treats_blank_as_unset() {
        // Empty/whitespace override and env must not win, and a blank
        // domain falls through to the placeholder rather than producing
        // `https://workflows.//`.
        assert_eq!(
            resolve_workflows_url(Some("  "), Some(""), Some(" neonlaw.com ")),
            "https://workflows.neonlaw.com/"
        );
        assert_eq!(
            resolve_workflows_url(None, None, Some("   ")),
            WORKFLOWS_PUBLIC_URL
        );
    }

    #[test]
    fn resolve_workflows_url_placeholder_only_when_nothing_set() {
        assert_eq!(
            resolve_workflows_url(None, None, None),
            WORKFLOWS_PUBLIC_URL
        );
    }

    #[tokio::test]
    async fn admin_api_register_posts_force_to_deployments_and_reports_services() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let worker_url = "https://workflows.example.com/";
        Mock::given(method("POST"))
            .and(path("/deployments"))
            .and(header("authorization", "Bearer test-admin-key"))
            // The force re-register must send exactly {uri, force:true} — a
            // plain register (no force) would refuse to overwrite the
            // existing endpoint and a service added since last register would
            // stay invisible at the ingress.
            .and(body_json(
                serde_json::json!({ "uri": worker_url, "force": true }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "dp_test",
                "services": [{ "name": "notation" }, { "name": "Archives" }],
            })))
            .expect(1)
            .mount(&server)
            .await;

        // A trailing slash on the admin base must not double up the path.
        register_via_admin_api_async(&format!("{}/", server.uri()), "test-admin-key", worker_url)
            .await
            .expect("admin-api register should succeed against the mock");
        // `.expect(1)` on the mock asserts exactly one matching POST on drop.
    }

    #[tokio::test]
    async fn admin_api_register_errors_on_non_2xx() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/deployments"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let err = register_via_admin_api_async(
            &server.uri(),
            "bad-token",
            "https://workflows.example.com/",
        )
        .await
        .expect_err("a 401 must surface as an error, not a silent success");
        assert!(
            err.to_string().contains("401"),
            "error should name the HTTP status, got: {err}"
        );
    }

    /// Load-bearing safety net: an empty environment must reproduce the
    /// exact pre-refactor constants. If this fails, `devx up` against a
    /// clean `.env` would behave differently than it always has.
    #[test]
    fn from_env_with_no_vars_equals_defaults() {
        let _guard = lock();
        clear_kind_env();
        assert_eq!(KindConfig::from_env(), default_cfg());
    }

    #[test]
    fn from_env_reads_string_overrides() {
        let _guard = lock();
        clear_kind_env();
        env::set_var("NAVIGATOR_KIND_CLUSTER", "fork-cluster");
        env::set_var("NAVIGATOR_K8S_NAMESPACE", "fork-ns");
        env::set_var("NAVIGATOR_KIND_DEPS_OVERLAY", "my/deps");
        env::set_var("NAVIGATOR_KIND_OVERLAY", "my/full");
        env::set_var("NAVIGATOR_GKE_OVERLAY", "my/gke");
        let cfg = KindConfig::from_env();
        clear_kind_env();
        assert_eq!(cfg.cluster, "fork-cluster");
        assert_eq!(cfg.namespace, "fork-ns");
        assert_eq!(cfg.deps_overlay, "my/deps");
        assert_eq!(cfg.full_overlay, "my/full");
        assert_eq!(cfg.gke_overlay, "my/gke");
    }

    #[test]
    fn from_env_reads_port_overrides() {
        let _guard = lock();
        clear_kind_env();
        env::set_var("NAVIGATOR_KIND_POSTGRES_PORT", "25432");
        env::set_var("NAVIGATOR_KIND_RESTATE_INGRESS_PORT", "19080");
        env::set_var("NAVIGATOR_KIND_RESTATE_ADMIN_PORT", "19070");
        env::set_var("NAVIGATOR_KIND_OPA_PORT", "18181");
        env::set_var("NAVIGATOR_KIND_KEYCLOAK_PORT", "31080");
        env::set_var("NAVIGATOR_KIND_FAKE_GCS_PORT", "31443");
        env::set_var("NAVIGATOR_KIND_WEB_PORT", "4001");
        let cfg = KindConfig::from_env();
        clear_kind_env();
        assert_eq!(cfg.postgres_port, 25432);
        assert_eq!(cfg.restate_ingress_port, 19080);
        assert_eq!(cfg.restate_admin_port, 19070);
        assert_eq!(cfg.opa_port, 18181);
        assert_eq!(cfg.keycloak_port, 31080);
        assert_eq!(cfg.fake_gcs_port, 31443);
        assert_eq!(cfg.web_port, 4001);
    }

    #[test]
    fn empty_and_garbage_values_fall_back_to_defaults() {
        let _guard = lock();
        clear_kind_env();
        // Empty string → default (a `FOO=` line shouldn't blank a path).
        env::set_var("NAVIGATOR_KIND_CLUSTER", "");
        // Unparseable port → default rather than a crash.
        env::set_var("NAVIGATOR_KIND_POSTGRES_PORT", "not-a-port");
        let cfg = KindConfig::from_env();
        clear_kind_env();
        assert_eq!(cfg.cluster, DEFAULT_CLUSTER_NAME);
        assert_eq!(cfg.postgres_port, DEFAULT_POSTGRES_HOST_PORT);
    }

    #[test]
    fn render_env_threads_the_ports() {
        let mut cfg = default_cfg();
        cfg.web_port = 4001;
        cfg.postgres_port = 25432;
        cfg.opa_port = 18181;
        cfg.keycloak_port = 31080;
        cfg.fake_gcs_port = 31443;
        cfg.restate_ingress_port = 19080;
        cfg.otlp_port = 14317;
        let env = render_env(&cfg);
        assert!(env.contains("PORT=4001"));
        assert!(env.contains("OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:14317"));
        assert!(env.contains("localhost:25432/navigator"));
        assert!(env.contains("NAVIGATOR_OPA_URL=http://localhost:18181"));
        assert!(env.contains("http://localhost:31080/keycloak/realms/navigator"));
        assert!(env.contains("NAVIGATOR_STORAGE_ENDPOINT=http://localhost:31443"));
        assert!(env.contains("RESTATE_BROKER_URL=http://localhost:19080"));
        assert!(env.contains("OAUTH_REDIRECT_URI=http://localhost:4001/auth/callback"));
    }

    // The committed cluster config; the render helper must reproduce it
    // byte-for-byte at default ports.
    const COMMITTED_KIND_CONFIG: &str = include_str!("../../../k8s/kind-config.yaml");

    #[test]
    fn render_kind_config_is_byte_identical_at_defaults() {
        let rendered = render_kind_config(COMMITTED_KIND_CONFIG, &default_cfg());
        assert_eq!(rendered, COMMITTED_KIND_CONFIG);
    }

    #[test]
    fn render_kind_config_substitutes_only_the_two_hostports() {
        let mut cfg = default_cfg();
        cfg.keycloak_port = 31080;
        cfg.fake_gcs_port = 31443;
        let rendered = render_kind_config(COMMITTED_KIND_CONFIG, &cfg);
        // The two overridden host ports appear...
        assert!(rendered.contains("hostPort: 31080"));
        assert!(rendered.contains("hostPort: 31443"));
        // ...the old host ports are gone...
        assert!(!rendered.contains("hostPort: 30080"));
        assert!(!rendered.contains("hostPort: 30443"));
        // ...but the fixed NodePort/containerPort values are untouched,
        // and the ingress host ports (8080/8443) are left alone.
        assert!(rendered.contains("containerPort: 30080"));
        assert!(rendered.contains("containerPort: 30443"));
        assert!(rendered.contains("hostPort: 8080"));
        assert!(rendered.contains("hostPort: 8443"));
        // Only the two hostPort lines changed — line count is stable.
        assert_eq!(
            rendered.lines().count(),
            COMMITTED_KIND_CONFIG.lines().count()
        );
    }
}
