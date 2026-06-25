//! `devx observability` — stand up the `OTel` Collector seam in a prod
//! cluster and wire the long-running binaries to it.
//!
//! This is the deterministic, in-binary form of the operator steps in
//! [`examples/deploy/k8s/observability/README.md`]. Production is
//! managed by direct `kubectl`/`gcloud` (no Config Sync, and the
//! `navigator-otel-env` `ConfigMap` the deployment manifests `envFrom` is
//! *not* part of any overlay `power-push` applies), so without this
//! command the collector never gets deployed and every binary's
//! `OTEL_EXPORTER_OTLP_ENDPOINT` stays unset — telemetry silently never
//! leaves the pod. `devx observability apply` closes that gap in one
//! idempotent command:
//!
//! 1. **GSA + IAM** — ensure the `navigator-otel` Google service account
//!    exists, carries `roles/cloudtrace.agent` +
//!    `roles/monitoring.metricWriter` + `roles/logging.logWriter`, and is
//!    bound to the in-cluster `otel-collector` KSA via Workload Identity.
//! 2. **Collector** — render the bundled manifests with the project id
//!    substituted and `kubectl apply` them: the Collector Deployment +
//!    Service, the `otel-collector-config`, the shared
//!    `navigator-otel-env` `ConfigMap`, and the GMP self-monitoring
//!    (`PodMonitoring` + alert `Rules`).
//! 3. **Wire the binaries** — patch `navigator-web` and
//!    `workflows-service` to `envFrom` the `navigator-otel-env` `ConfigMap`
//!    so `OTEL_EXPORTER_OTLP_ENDPOINT` reaches `telemetry::init`. This is
//!    a `kubectl patch` rather than an overlay edit because prod is
//!    direct-`kubectl`-managed and `power-push` here is an image-only
//!    push (no `NAVIGATOR_GKE_OVERLAY_DIR`).
//!
//! Everything per-deployment flows through the environment via the same
//! [`PowerPushConfig`] `power-push` uses — there is no literal project
//! id, region, cluster, namespace, or context in this file.
//!
//! ## Testing
//!
//! The orchestration shells out to `gcloud`/`kubectl`, so it isn't
//! unit-tested. The pure pieces — the project-id substitution and the
//! `envFrom` patch builder — are covered by the `tests` module below.

use std::fs;
use std::process::Command;

use anyhow::{Context, Result};

use super::power_push::PowerPushConfig;
use super::{require_auth, require_tools, run};

/// In-cluster Deployment + container names (workspace conventions, same
/// as `power_push`). Each long-running binary gets the collector
/// endpoint via `envFrom` the shared `ConfigMap`.
const WEB_DEPLOYMENT: &str = "navigator-web";
const WEB_CONTAINER: &str = "web";
const WORKFLOWS_DEPLOYMENT: &str = "workflows-service";
const WORKFLOWS_CONTAINER: &str = "worker";

/// The Collector's Google service account short name + the KSA it backs.
const OTEL_GSA: &str = "navigator-otel";
const OTEL_KSA: &str = "otel-collector";
/// The shared `ConfigMap` that carries `OTEL_EXPORTER_OTLP_ENDPOINT` — one
/// source of truth for the collector URL, `envFrom`'d by every binary.
const OTEL_ENV_CONFIGMAP: &str = "navigator-otel-env";
/// The K8s Secret each binary already `envFrom`s; preserved alongside the
/// `ConfigMap` when we rewrite `envFrom`.
const WEB_SECRET: &str = "navigator-web-secrets";

/// The telemetry-write roles the Collector's GSA needs to fan OTLP out to
/// Google Cloud (traces, metrics, logs respectively).
const OTEL_ROLES: &[&str] = &[
    "roles/cloudtrace.agent",
    "roles/monitoring.metricWriter",
    "roles/logging.logWriter",
];

/// The Collector + `ConfigMap`s + Service + self-monitoring manifests,
/// bundled into the binary so the command is self-contained. Both carry
/// the `YOUR_PROJECT_ID` placeholder convention (`otel-collector.yaml`
/// twice: the WI GSA annotation + the `googlecloud` exporter project;
/// `collector-monitoring.yaml` has none) — `render_manifest` substitutes
/// the real project id.
const OTEL_COLLECTOR_YAML: &str =
    include_str!("../../../examples/deploy/k8s/observability/otel-collector.yaml");
const COLLECTOR_MONITORING_YAML: &str =
    include_str!("../../../examples/deploy/k8s/observability/collector-monitoring.yaml");

/// The placeholder every deploy-side manifest carries for the GCP project.
const PROJECT_PLACEHOLDER: &str = "YOUR_PROJECT_ID";

/// Options parsed from the `devx observability apply` flags.
#[derive(Debug, Clone, Copy)]
pub struct ObservabilityOpts {
    /// Print every command instead of running it.
    pub dry_run: bool,
}

/// Entry point for `Cmd::Observability`.
pub fn run_observability(opts: ObservabilityOpts) -> Result<()> {
    let cfg = PowerPushConfig::from_env()?;
    require_tools(&["kubectl", "gcloud"])?;
    if !opts.dry_run {
        require_auth(&["gcloud"])?;
    }
    eprintln!(
        "==> observability: standing up the `OTel` collector in {} ({})",
        cfg.project_id, cfg.context
    );
    ensure_gsa_iam(&cfg, opts.dry_run)?;
    apply_manifests(&cfg, opts.dry_run)?;
    wire_binaries(&cfg, opts.dry_run)?;
    eprintln!(
        "==> observability ready. Roll the binaries so they pick up the endpoint \
         (a `devx power-push` set-image, or `kubectl rollout restart`), then look for \
         traces in Cloud Trace + the `navigator.workflow.trigger.fired` metric."
    );
    Ok(())
}

/// Step 1 — ensure the Collector's GSA exists, carries the three
/// telemetry-write roles, and is bound to the in-cluster KSA via Workload
/// Identity. Every call is idempotent: the GSA is created only when absent
/// (`describe` probe), and the IAM bindings are no-ops when already present.
fn ensure_gsa_iam(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    let gsa = gsa_email(&cfg.project_id);
    if gsa_exists(cfg, &gsa)? {
        eprintln!("==> GSA {gsa} already exists");
    } else {
        eprintln!("==> creating GSA {gsa}");
        exec(
            dry_run,
            Command::new("gcloud")
                .args(["iam", "service-accounts", "create", OTEL_GSA])
                .arg(format!("--project={}", cfg.project_id))
                .args(["--display-name", "Neon Law Navigator `OTel` Collector"]),
        )?;
    }
    // Bind the telemetry-write roles one at a time. `add-iam-policy-binding`
    // is read-modify-write on the project policy, so a tight loop can lose
    // an etag race; running them sequentially (each its own gcloud call)
    // avoids that, and a repeat binding is a documented no-op.
    for role in OTEL_ROLES {
        eprintln!("==> binding {role} → {gsa}");
        exec(
            dry_run,
            Command::new("gcloud")
                .args(["projects", "add-iam-policy-binding", &cfg.project_id])
                .arg(format!("--member=serviceAccount:{gsa}"))
                .arg(format!("--role={role}"))
                .args(["--condition", "None"]),
        )?;
    }
    eprintln!("==> binding Workload Identity {OTEL_KSA} KSA → {gsa}");
    exec(
        dry_run,
        Command::new("gcloud")
            .args(["iam", "service-accounts", "add-iam-policy-binding", &gsa])
            .arg(format!("--project={}", cfg.project_id))
            .args(["--role", "roles/iam.workloadIdentityUser"])
            .arg(format!(
                "--member=serviceAccount:{}.svc.id.goog[{}/{OTEL_KSA}]",
                cfg.project_id, cfg.namespace
            )),
    )
}

/// Step 2 — render the bundled manifests with the project id substituted
/// and `kubectl apply` them (context-pinned, the manifests carry their own
/// namespace). The collector config is in a `ConfigMap`, so a server-side
/// apply can't catch a bad collector pipeline — the operator confirms the
/// rollout settles afterward (this command waits on it).
fn apply_manifests(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    for (name, template) in [
        ("otel-collector.yaml", OTEL_COLLECTOR_YAML),
        ("collector-monitoring.yaml", COLLECTOR_MONITORING_YAML),
    ] {
        let rendered = render_manifest(template, &cfg.project_id);
        let path = std::env::temp_dir().join(format!("navigator-otel-{name}"));
        if dry_run {
            eprintln!(
                "DRY-RUN: would render {name} → {} and apply it",
                path.display()
            );
            continue;
        }
        fs::write(&path, rendered)
            .with_context(|| format!("write rendered {name} to {}", path.display()))?;
        eprintln!("==> applying {name}");
        run(Command::new("kubectl")
            .args(["--context", &cfg.context, "apply", "-f"])
            .arg(&path))?;
        let _ = fs::remove_file(&path);
    }
    if dry_run {
        eprintln!("DRY-RUN: would wait for the otel-collector rollout");
        return Ok(());
    }
    eprintln!("==> waiting for the otel-collector rollout");
    run(Command::new("kubectl").args([
        "--context",
        &cfg.context,
        "-n",
        &cfg.namespace,
        "rollout",
        "status",
        "deployment/otel-collector",
        "--timeout=120s",
    ]))
}

/// Step 3 — patch `navigator-web` and `workflows-service` so the binary
/// container `envFrom`s the `navigator-otel-env` `ConfigMap` (alongside the
/// existing Secret). That supplies `OTEL_EXPORTER_OTLP_ENDPOINT`, which is
/// what flips `telemetry::init` from stdout-only to JSON + OTLP export.
fn wire_binaries(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    for (deployment, container) in [
        (WEB_DEPLOYMENT, WEB_CONTAINER),
        (WORKFLOWS_DEPLOYMENT, WORKFLOWS_CONTAINER),
    ] {
        let patch = envfrom_patch(container);
        eprintln!("==> wiring {deployment} ({container}) → {OTEL_ENV_CONFIGMAP}");
        exec(
            dry_run,
            Command::new("kubectl")
                .args(["--context", &cfg.context, "-n", &cfg.namespace])
                .args(["patch", "deployment", deployment, "--type=strategic", "-p"])
                .arg(&patch),
        )?;
    }
    Ok(())
}

/// The Collector GSA's full email for a project.
fn gsa_email(project_id: &str) -> String {
    format!("{OTEL_GSA}@{project_id}.iam.gserviceaccount.com")
}

/// True when the GSA already exists — `gcloud … describe` exits non-zero
/// when it does not, which we map to "create it". Under `--dry-run` we
/// assume absent so the create command is the one printed.
fn gsa_exists(cfg: &PowerPushConfig, gsa: &str) -> Result<bool> {
    let status = Command::new("gcloud")
        .args(["iam", "service-accounts", "describe", gsa])
        .arg(format!("--project={}", cfg.project_id))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("probe whether the navigator-otel GSA exists")?;
    Ok(status.success())
}

/// Render a deploy-side manifest by substituting the project-id
/// placeholder. Pure so the substitution is unit-testable; a template
/// with no placeholder (the self-monitoring manifest) passes through
/// unchanged.
#[must_use]
pub fn render_manifest(template: &str, project_id: &str) -> String {
    template.replace(PROJECT_PLACEHOLDER, project_id)
}

/// Build the strategic-merge patch that rewrites a single container's
/// `envFrom` to carry the `OTel` `ConfigMap` first, then the existing Secret.
/// `envFrom` has no strategic-merge key, so the whole list is replaced —
/// hence both entries are restated. Pure + deterministic so it is
/// unit-testable.
#[must_use]
pub fn envfrom_patch(container: &str) -> String {
    format!(
        concat!(
            r#"{{"spec":{{"template":{{"spec":{{"containers":[{{"#,
            r#""name":"{container}","envFrom":["#,
            r#"{{"configMapRef":{{"name":"{cm}"}}}},"#,
            r#"{{"secretRef":{{"name":"{secret}"}}}}"#,
            r#"]}}]}}}}}}}}"#,
        ),
        container = container,
        cm = OTEL_ENV_CONFIGMAP,
        secret = WEB_SECRET,
    )
}

/// Run a command, or — under `--dry-run` — print it instead.
fn exec(dry_run: bool, cmd: &mut Command) -> Result<()> {
    if dry_run {
        let mut line = cmd.get_program().to_string_lossy().into_owned();
        for arg in cmd.get_args() {
            line.push(' ');
            line.push_str(&arg.to_string_lossy());
        }
        eprintln!("DRY-RUN $ {line}");
        Ok(())
    } else {
        run(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_every_project_placeholder() {
        let rendered = render_manifest(OTEL_COLLECTOR_YAML, "my-org-prod");
        // The bundled collector manifest carries the placeholder exactly
        // twice (WI GSA annotation + googlecloud exporter project); both
        // must be substituted and none left behind.
        assert!(!rendered.contains(PROJECT_PLACEHOLDER));
        assert!(rendered.contains("navigator-otel@my-org-prod.iam.gserviceaccount.com"));
        assert!(rendered.contains("project: my-org-prod"));
    }

    #[test]
    fn render_leaves_placeholder_free_manifest_unchanged() {
        // The self-monitoring manifest has no project placeholder, so
        // rendering is the identity — guards against accidental rewrites.
        assert_eq!(
            render_manifest(COLLECTOR_MONITORING_YAML, "my-org-prod"),
            COLLECTOR_MONITORING_YAML
        );
    }

    #[test]
    fn gsa_email_follows_the_iam_convention() {
        assert_eq!(
            gsa_email("my-org-prod"),
            "navigator-otel@my-org-prod.iam.gserviceaccount.com"
        );
    }

    #[test]
    fn envfrom_patch_lists_configmap_then_secret_for_the_named_container() {
        let patch = envfrom_patch("web");
        let parsed: serde_json::Value =
            serde_json::from_str(&patch).expect("envFrom patch must be valid JSON");
        let containers = &parsed["spec"]["template"]["spec"]["containers"];
        assert_eq!(containers[0]["name"], "web");
        let env_from = &containers[0]["envFrom"];
        // `ConfigMap` first (collector endpoint), Secret second (preserved).
        assert_eq!(env_from[0]["configMapRef"]["name"], OTEL_ENV_CONFIGMAP);
        assert_eq!(env_from[1]["secretRef"]["name"], WEB_SECRET);
    }

    #[test]
    fn envfrom_patch_targets_the_worker_container_for_workflows_service() {
        let patch = envfrom_patch("worker");
        let parsed: serde_json::Value = serde_json::from_str(&patch).unwrap();
        assert_eq!(
            parsed["spec"]["template"]["spec"]["containers"][0]["name"],
            "worker"
        );
    }
}
