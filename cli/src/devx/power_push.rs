//! `devx power-push` — one-shot "ship to prod" orchestration.
//!
//! This is the deterministic, in-binary form of the `power-push` skill
//! (`.claude/skills/power-push/SKILL.md`). The skill remains the prose
//! rationale ("why each step, in this order"); this module is the
//! executable that runs the steps so an operator types one command
//! instead of pasting eight shell blocks.
//!
//! Two flows, matching the skill:
//!
//! - **Full build** (default): verify → build BOTH images → push BOTH
//!   to Artifact Registry → archive a git bundle to the GCS source
//!   bucket → confirm the prod Secret satisfies the new binary's boot
//!   invariants → roll out BOTH deployments at HEAD's short SHA →
//!   re-register the worker with Restate → reclaim the local images.
//! - **No-rebuild restart** (`--restart-only`): after a Secret value
//!   was rotated, `kubectl rollout restart` BOTH deployments that
//!   `envFrom` the Secret so the pods re-read it (pods cache `envFrom`
//!   at start and never reload).
//!
//! Everything that varies per deployment flows through `.env` — there
//! is no literal project ID, region, domain, registry path, or bucket
//! name in this file (same contract as the skill). See
//! [`PowerPushConfig::from_env`].
//!
//! ## What this does NOT do
//!
//! - It never commits. It reads `git rev-parse --short HEAD`; the
//!   operator commits first so the image tag is a real commit SHA.
//! - It never auto-patches a prod Secret. The invariant check (7b)
//!   *aborts* with the exact `kubectl patch` to run when a required
//!   key is missing — generating and writing a prod secret silently is
//!   a judgment call left to the operator.
//!
//! ## Testing
//!
//! The shell-out orchestration needs a real Docker daemon + cluster,
//! so it isn't unit-tested. The pure pieces — env-driven config, the
//! derived-name formulas, the required-key parser, and the missing-key
//! diff — are covered by the `tests` module below.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::{
    build_image_at_platform, env_string, git_commit_time, git_full_head, git_short_head,
    require_auth, require_tools, run, workspace_root,
};

/// Prod runs on GKE Autopilot, which is amd64. Pin every pushed image to
/// `linux/amd64` so a ship from an arm64 (Apple-Silicon) laptop produces
/// an amd64 image — the Dockerfiles otherwise build for the host arch.
const PROD_PLATFORM: &str = "linux/amd64";

/// Local image tags `devx image` / `devx image-workflows-service`
/// produce. Reused from `main.rs` so there is one source of truth for
/// the `:dev` names this module retags and reclaims.
use super::{WEB_IMAGE, WORKFLOWS_SERVICE_IMAGE};

/// In-cluster Deployment + container names. These are workspace
/// conventions (the GKE overlay names them); they are not per-deploy
/// configuration, so they stay as constants rather than env vars.
const WEB_DEPLOYMENT: &str = "navigator-web";
const WEB_CONTAINER: &str = "web";
const WORKFLOWS_DEPLOYMENT: &str = "workflows-service";
const WORKFLOWS_CONTAINER: &str = "worker";

/// Every per-deployment value `power-push` reads, resolved once from
/// the environment. Required values bail when unset (fail fast — never
/// substitute a project-internal default). Optional values fall back
/// to a documented workspace default or to `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPushConfig {
    /// Target GCP project ID (`NAVIGATOR_GCP_PROJECT_ID`).
    pub project_id: String,
    /// Region for Artifact Registry + bucket + cluster
    /// (`NAVIGATOR_GCP_LOCATION`).
    pub location: String,
    /// Cluster name — also the Artifact Registry repo name
    /// (`NAVIGATOR_GKE_CLUSTER_NAME`).
    pub cluster: String,
    /// K8s namespace for the Deployments (`NAVIGATOR_K8S_NAMESPACE`,
    /// default `navigator`).
    pub namespace: String,
    /// Public hostname for the post-rollout smoke check
    /// (`NAVIGATOR_PRIMARY_DOMAIN`).
    pub primary_domain: String,
    /// Private kustomize overlay path (`NAVIGATOR_GKE_OVERLAY_DIR`).
    /// `None` → image-only push (skip the manifest apply in 7a); set
    /// it on laptop-`kubectl apply` forks, leave unset on `GitOps`.
    pub overlay_dir: Option<String>,
    /// Name of the K8s Secret both deployments `envFrom`
    /// (`NAVIGATOR_WEB_SECRET_NAME`, default `navigator-web-secrets`).
    pub secret_name: String,
    /// Public worker URL Restate Cloud dials (`NAVIGATOR_WORKFLOWS_URL`).
    /// `None` → fall through to the `devx restate register` default.
    pub workflows_url: Option<String>,
    /// `kubectl` context to pin every prod call to. Override with
    /// `NAVIGATOR_GKE_CONTEXT`; otherwise the GKE convention
    /// `gke_<project>_<location>_<cluster>-prod`.
    pub context: String,
}

impl PowerPushConfig {
    /// Resolve every value from the environment (`.env` is already
    /// loaded by `main()` via `dotenvy`). Bails on the first missing
    /// required var with a message naming it.
    pub fn from_env() -> Result<Self> {
        let project_id = required_env("NAVIGATOR_GCP_PROJECT_ID")?;
        let location = required_env("NAVIGATOR_GCP_LOCATION")?;
        let cluster = required_env("NAVIGATOR_GKE_CLUSTER_NAME")?;
        let primary_domain = required_env("NAVIGATOR_PRIMARY_DOMAIN")?;
        let namespace = env_string("NAVIGATOR_K8S_NAMESPACE", "navigator");
        let secret_name = env_string("NAVIGATOR_WEB_SECRET_NAME", "navigator-web-secrets");
        let overlay_dir = optional_env("NAVIGATOR_GKE_OVERLAY_DIR");
        let workflows_url = optional_env("NAVIGATOR_WORKFLOWS_URL");
        let context = optional_env("NAVIGATOR_GKE_CONTEXT")
            .unwrap_or_else(|| derived_context(&project_id, &location, &cluster));
        Ok(Self {
            project_id,
            location,
            cluster,
            namespace,
            primary_domain,
            overlay_dir,
            secret_name,
            workflows_url,
            context,
        })
    }

    /// Artifact Registry repo path — `<region>-docker.pkg.dev/<project>/<cluster>`.
    /// Matches the workspace convention in `docs/oss-install.md`.
    #[must_use]
    pub fn registry(&self) -> String {
        format!(
            "{}-docker.pkg.dev/{}/{}",
            self.location, self.project_id, self.cluster
        )
    }

    /// Pushed `navigator-web` image URL at `sha`.
    #[must_use]
    pub fn web_image(&self, sha: &str) -> String {
        format!("{}/navigator-web:{sha}", self.registry())
    }

    /// Pushed `navigator-workflows-service` image URL at `sha`.
    #[must_use]
    pub fn workflows_image(&self, sha: &str) -> String {
        format!("{}/navigator-workflows-service:{sha}", self.registry())
    }

    /// The GCS source bucket — `gs://<project>-source`.
    #[must_use]
    pub fn source_bucket(&self) -> String {
        format!("gs://{}-source", self.project_id)
    }

    /// The bundle object URL for `sha` inside the source bucket.
    #[must_use]
    pub fn bundle_object(&self, sha: &str) -> String {
        format!("{}/navigator-{sha}.bundle", self.source_bucket())
    }

    /// The public worker URL the 7d re-register targets, resolved the
    /// same way `devx restate register` resolves it: explicit
    /// `NAVIGATOR_WORKFLOWS_URL` first, otherwise derived from
    /// `NAVIGATOR_PRIMARY_DOMAIN` (`https://workflows.<domain>/`), never
    /// the bare placeholder when a domain is known. This is what the
    /// 2026-06-10 ship needed — it had a domain but no explicit URL and
    /// fell through to `workflows.example.com`, silently no-op'ing the
    /// register.
    #[must_use]
    pub fn workflows_url_resolved(&self) -> String {
        super::resolve_workflows_url(
            None,
            self.workflows_url.as_deref(),
            Some(&self.primary_domain),
        )
    }
}

/// The GKE context naming convention `gcloud container clusters
/// get-credentials` writes. Factored out so the formula is testable.
#[must_use]
fn derived_context(project_id: &str, location: &str, cluster: &str) -> String {
    format!("gke_{project_id}_{location}_{cluster}-prod")
}

/// Read a required env var, treating unset *or* empty as an error.
fn required_env(key: &str) -> Result<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => bail!("{key} must be set in .env for `devx power-push`"),
    }
}

/// Read an optional env var, mapping unset *or* empty to `None`.
fn optional_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Parse the keys `web::config::enforce_prod_invariants` requires, by
/// scraping the `"<KEY> must be set` string literals straight from the
/// invariant source. Reading the source (rather than maintaining a
/// duplicate list) means this never drifts from the binary's actual
/// boot requirements.
///
/// Pairs each key with the optional **trigger** that gates it. Some
/// invariants are conditional:
/// the binary only requires the key when another env var is itself set —
/// e.g. `"OIDC_AUDIENCE must be set when OIDC_JWKS_URL is …"`. The
/// invariant message names its own trigger ("… when `TRIGGER` is …"), so
/// the trigger is read from the same literal, staying faithful to the
/// "scrape the source, never maintain a parallel list" philosophy.
///
/// An unconditional invariant (`"<KEY> must be set (otherwise …"`) yields
/// `(KEY, None)`; a conditional one yields `(KEY, Some(TRIGGER))`. A key
/// that appears both ways resolves to `None` (unconditional wins — it is
/// always required). Sorted + de-duplicated by key.
#[must_use]
pub fn required_secret_keys_with_triggers(config_src: &str) -> Vec<(String, Option<String>)> {
    const MARKER: &str = " must be set";
    let mut keys: BTreeMap<String, Option<String>> = BTreeMap::new();
    for line in config_src.lines() {
        let Some(end) = line.find(MARKER) else {
            continue;
        };
        let prefix = &line[..end];
        let bytes = prefix.as_bytes();
        let mut start = bytes.len();
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' {
                start -= 1;
            } else {
                break;
            }
        }
        // Only count it when the identifier opens a string literal —
        // i.e. the char before the run is a double-quote. That filters
        // out prose that happens to contain "… must be set".
        if start < bytes.len() && start > 0 && bytes[start - 1] == b'"' {
            let key = prefix[start..].to_string();
            let trigger = trigger_after(&line[end + MARKER.len()..]);
            let unconditional = trigger.is_none();
            keys.entry(key)
                .and_modify(|existing| {
                    if unconditional {
                        *existing = None;
                    }
                })
                .or_insert(trigger);
        }
    }
    keys.into_iter().collect()
}

/// Read the trigger key out of the tail of an invariant message — the
/// `TRIGGER` in `" when TRIGGER is …"`. Returns `None` for an
/// unconditional invariant (whose tail starts with `" (otherwise …"`).
fn trigger_after(tail: &str) -> Option<String> {
    let rest = tail.trim_start().strip_prefix("when ")?.trim_start();
    let ident: String = rest
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    (!ident.is_empty()).then_some(ident)
}

/// Of the parsed (key, trigger) invariants, the keys actually required
/// for *this* environment: every unconditional key, plus each
/// conditional key whose trigger is itself satisfied (present in the
/// Secret or a Deployment env). A conditional key whose trigger is absent
/// is not required — the binary's runtime invariant skips it too.
#[must_use]
pub fn effective_required_keys(
    required: &[(String, Option<String>)],
    satisfied: &BTreeSet<String>,
) -> Vec<String> {
    required
        .iter()
        .filter(|(_key, trigger)| trigger.as_ref().is_none_or(|t| satisfied.contains(t)))
        .map(|(key, _trigger)| key.clone())
        .collect()
}

/// Required keys not satisfied by anything the running pod can see.
/// `satisfied` is the union of the Secret's data keys and the
/// Deployments' declared env-var names — a key present in either is
/// fine (a plain env var, a `secretKeyRef`, or `envFrom` all count).
/// What remains will crash-loop the new pod at boot.
#[must_use]
pub fn missing_keys(required: &[String], satisfied: &BTreeSet<String>) -> Vec<String> {
    required
        .iter()
        .filter(|k| !satisfied.contains(*k))
        .cloned()
        .collect()
}

// ---------- orchestration (shell-out; not unit-tested) ----------

/// Options parsed from the `devx power-push` flags.
#[derive(Debug, Clone, Copy)]
pub struct PowerPushOpts {
    /// Print every command instead of running it.
    pub dry_run: bool,
    /// No-rebuild path: just `kubectl rollout restart` both
    /// deployments (Secret-value rotation), then exit.
    pub restart_only: bool,
    /// Skip fmt/clippy/test/markdown-lint (only when shipping a SHA
    /// already verified in this session).
    pub skip_verify: bool,
}

/// Entry point for `Cmd::PowerPush`.
pub fn run_power_push(opts: PowerPushOpts) -> Result<()> {
    let cfg = PowerPushConfig::from_env()?;
    if opts.restart_only {
        return restart_only(&cfg, opts.dry_run);
    }
    full_build(&cfg, opts)
}

/// The no-rebuild push: restart both deployments so the pods re-read a
/// rotated Secret value. Pods cache `envFrom` at start and never
/// reload, so a rotation is invisible until the pod is recreated.
fn restart_only(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    require_tools(&["kubectl"])?;
    require_auth(&["gcloud"])?;
    verify_context(cfg, dry_run)?;
    eprintln!("==> no-rebuild push: rollout restart both deployments (Secret rotation)");
    exec(
        dry_run,
        kubectl(cfg)
            .arg("rollout")
            .arg("restart")
            .arg(format!("deployment/{WEB_DEPLOYMENT}"))
            .arg(format!("deployment/{WORKFLOWS_DEPLOYMENT}")),
    )?;
    wait_rollouts(cfg, dry_run, "120s")?;
    eprintln!(
        "==> restart complete. VERIFY on the third-party side (the pod will 2xx against a \
         valid-but-wrong key) — compare upstream stats before/after."
    );
    Ok(())
}

/// The full build → push → bundle → deploy → re-register ship.
fn full_build(cfg: &PowerPushConfig, opts: PowerPushOpts) -> Result<()> {
    require_tools(&["git", "docker", "kubectl", "gcloud"])?;
    // Authenticated, not just installed: docker (build), gcloud (registry
    // push), doppler (the config this runs under), restate (the re-register
    // step). A logged-out CLI here would otherwise fail mid-ship, after
    // images are already pushed.
    require_auth(&["docker", "gcloud", "doppler", "restate"])?;
    let dry_run = opts.dry_run;

    // 1. Pre-flight — confirm context resolves and warn on a dirty tree
    //    (uncommitted work won't ship: the image tag is HEAD's SHA).
    verify_context(cfg, dry_run)?;
    warn_if_dirty()?;
    let sha = git_short_head()?;
    eprintln!(
        "==> shipping HEAD {sha} to {} ({})",
        cfg.project_id, cfg.context
    );

    // 3. Verify before building the image (fmt, clippy, test, md-lint).
    if opts.skip_verify {
        eprintln!("==> skipping verify (--skip-verify): only safe at an already-verified SHA");
    } else {
        verify(dry_run)?;
    }

    // 4. Build BOTH images. The web image is stamped with the full SHA +
    //    commit time as build-args so the running binary can report the
    //    deployed commit at `GET /version` (it can't drift from the image
    //    bytes this way). See `images/Dockerfile.web` + `web::version`.
    let root = workspace_root()?;
    let full_sha = git_full_head()?;
    let commit_time = git_commit_time()?;
    build(
        dry_run,
        WEB_IMAGE,
        "images/Dockerfile.web",
        &root,
        &[
            ("GIT_SHA", full_sha.as_str()),
            ("BUILD_TIME", commit_time.as_str()),
        ],
    )?;
    build(
        dry_run,
        WORKFLOWS_SERVICE_IMAGE,
        "images/Dockerfile.workflows-service",
        &root,
        &[],
    )?;

    // 5. Push BOTH to Artifact Registry.
    let web_remote = cfg.web_image(&sha);
    let workflows_remote = cfg.workflows_image(&sha);
    tag_and_push(dry_run, WEB_IMAGE, &web_remote)?;
    tag_and_push(dry_run, WORKFLOWS_SERVICE_IMAGE, &workflows_remote)?;

    // 6. Archive a git bundle to the GCS source bucket.
    bundle_to_gcs(cfg, &sha, dry_run)?;

    // 7a. Sync the manifest (only when an overlay is configured).
    sync_overlay(cfg, dry_run)?;

    // 7b. Confirm the prod Secret satisfies the new binary's invariants.
    ensure_secret_invariants(cfg, dry_run)?;

    // 7c. Bump BOTH images, then wait on both rollouts.
    eprintln!("==> rolling out both deployments at {sha}");
    exec(
        dry_run,
        kubectl(cfg)
            .arg("set")
            .arg("image")
            .arg(format!("deployment/{WEB_DEPLOYMENT}"))
            .arg(format!("{WEB_CONTAINER}={web_remote}")),
    )?;
    exec(
        dry_run,
        kubectl(cfg)
            .arg("set")
            .arg("image")
            .arg(format!("deployment/{WORKFLOWS_DEPLOYMENT}"))
            .arg(format!("{WORKFLOWS_CONTAINER}={workflows_remote}")),
    )?;
    wait_rollouts(cfg, dry_run, "300s")?;

    // 7d. Re-register the worker with Restate (best-effort).
    reregister(cfg, dry_run);

    // 8. Smoke-check the public surface (best-effort) and reclaim disk.
    smoke_check(cfg, dry_run);
    reclaim(dry_run);

    eprintln!("==> power-push complete: {sha} live in {}", cfg.project_id);
    Ok(())
}

/// Run the four workspace-wide gates the skill mandates before a ship.
fn verify(dry_run: bool) -> Result<()> {
    eprintln!("==> verify: fmt, clippy, test, markdown-lint (workspace-wide)");
    exec(
        dry_run,
        Command::new("cargo").args(["fmt", "--all", "--", "--check"]),
    )?;
    exec(
        dry_run,
        Command::new("cargo").args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    )?;
    exec(dry_run, Command::new("cargo").args(["test", "--workspace"]))?;
    exec(
        dry_run,
        Command::new("cargo").args([
            "run",
            "-p",
            "cli",
            "--quiet",
            "--",
            "validate",
            "--markdown-only",
            "--no-default-excludes",
            ".",
        ]),
    )
}

/// Build one image, honoring `--dry-run`. Delegates to the same
/// `build_image_at_with_args` `devx image` uses so behavior is identical.
fn build(
    dry_run: bool,
    tag: &str,
    dockerfile: &str,
    root: &Path,
    build_args: &[(&str, &str)],
) -> Result<()> {
    if dry_run {
        use std::fmt::Write as _;
        let mut args = String::new();
        for (k, v) in build_args {
            let _ = write!(args, " --build-arg {k}={v}");
        }
        eprintln!(
            "DRY-RUN $ docker build --platform {PROD_PLATFORM} -t {tag} -f {}{args} {}",
            root.join(dockerfile).display(),
            root.display()
        );
        Ok(())
    } else {
        build_image_at_platform(tag, dockerfile, root, build_args, Some(PROD_PLATFORM))
    }
}

/// `docker tag` the local `:dev` image to the GAR URL and `docker push`.
fn tag_and_push(dry_run: bool, local: &str, remote: &str) -> Result<()> {
    exec(
        dry_run,
        Command::new("docker").arg("tag").arg(local).arg(remote),
    )?;
    exec(dry_run, Command::new("docker").arg("push").arg(remote))
}

/// `git bundle create … --all` then `gcloud storage cp` to the source
/// bucket, then remove the local bundle. `--all` makes the object a
/// full restore point (`git clone <bundle>` works anywhere).
fn bundle_to_gcs(cfg: &PowerPushConfig, sha: &str, dry_run: bool) -> Result<()> {
    let bundle = std::env::temp_dir().join(format!("navigator-{sha}.bundle"));
    let bundle = bundle.to_string_lossy().into_owned();
    let object = cfg.bundle_object(sha);
    eprintln!("==> archiving git bundle → {object}");
    exec(
        dry_run,
        Command::new("git")
            .arg("bundle")
            .arg("create")
            .arg(&bundle)
            .arg("--all"),
    )?;
    exec(
        dry_run,
        Command::new("gcloud")
            .arg("storage")
            .arg("cp")
            .arg(&bundle)
            .arg(&object)
            .arg(format!("--project={}", cfg.project_id)),
    )?;
    // Best-effort cleanup of the local bundle; never fail the ship on it.
    if !dry_run {
        let _ = fs::remove_file(&bundle);
    }
    Ok(())
}

/// 7a — `kubectl diff` (surface drift) then `kubectl apply` the
/// overlay, only when `NAVIGATOR_GKE_OVERLAY_DIR` is set. On `GitOps`
/// (no overlay dir) the controller reconciles continuously; skip.
fn sync_overlay(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    let Some(overlay) = cfg.overlay_dir.as_deref() else {
        eprintln!(
            "==> no NAVIGATOR_GKE_OVERLAY_DIR set; image-only push (skipping manifest apply)"
        );
        return Ok(());
    };
    eprintln!("==> syncing manifest from overlay {overlay}");
    // `kubectl diff` exits 1 when a diff exists — that's the signal,
    // not a failure; show it but don't abort on it.
    let _ = exec(dry_run, kubectl_ctx(cfg).arg("diff").arg("-k").arg(overlay));
    exec(
        dry_run,
        kubectl_ctx(cfg).arg("apply").arg("-k").arg(overlay),
    )
}

/// 7b — abort the ship if any boot-required key is absent from both
/// the Secret and the Deployments' env. A missing key crash-loops the
/// new pod at boot (`enforce_prod_invariants`), so catching it here
/// beats a silently-stalled rollout. We never auto-patch: print the
/// exact `kubectl patch` and stop.
fn ensure_secret_invariants(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    if dry_run {
        eprintln!(
            "DRY-RUN: would diff required keys (web/src/config.rs) vs Secret + Deployment env"
        );
        return Ok(());
    }
    let root = workspace_root()?;
    let config_src = fs::read_to_string(root.join("web/src/config.rs"))
        .context("read web/src/config.rs for the required-key invariants")?;
    let parsed = required_secret_keys_with_triggers(&config_src);

    let mut satisfied = secret_data_keys(cfg)?;
    for deployment in [WEB_DEPLOYMENT, WORKFLOWS_DEPLOYMENT] {
        satisfied.extend(deployment_env_names(cfg, deployment)?);
    }

    // Drop conditional invariants whose trigger isn't configured here —
    // the binary's own runtime check skips them too, so requiring them
    // would be a false positive (e.g. OIDC_AUDIENCE/OIDC_ISSUER when the
    // optional OIDC_JWKS_URL bearer path isn't enabled in prod).
    let required = effective_required_keys(&parsed, &satisfied);

    let missing = missing_keys(&required, &satisfied);
    if missing.is_empty() {
        eprintln!(
            "==> Secret invariants OK ({} required keys present)",
            required.len()
        );
        return Ok(());
    }
    bail!(
        "the new binary requires keys absent from both the `{secret}` Secret and the \
         Deployment env: {missing:?}\n\
         A missing key crash-loops the new pod at boot. Add each (value never transits \
         logs) before re-running power-push, e.g.:\n  \
         kubectl --context {ctx} -n {ns} patch secret {secret} --type=merge \\\n    \
         -p '{{\"stringData\":{{\"{first}\":\"<value>\"}}}}'\n\
         (Keys provided as deployment env — e.g. NAVIGATOR_OPA_URL — belong in the overlay \
         (7a/NAVIGATOR_GKE_OVERLAY_DIR), not the Secret.)",
        secret = cfg.secret_name,
        ctx = cfg.context,
        ns = cfg.namespace,
        first = missing.first().map_or("KEY", String::as_str),
    )
}

/// The data keys carried by the prod Secret.
fn secret_data_keys(cfg: &PowerPushConfig) -> Result<BTreeSet<String>> {
    let json = kubectl_json(cfg, "secret", &cfg.secret_name)?;
    Ok(json
        .get("data")
        .and_then(serde_json::Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default())
}

/// The env-var names declared by every container in a Deployment.
fn deployment_env_names(cfg: &PowerPushConfig, deployment: &str) -> Result<BTreeSet<String>> {
    let json = kubectl_json(cfg, "deployment", deployment)?;
    let mut names = BTreeSet::new();
    if let Some(containers) = json
        .pointer("/spec/template/spec/containers")
        .and_then(serde_json::Value::as_array)
    {
        for container in containers {
            if let Some(env) = container.get("env").and_then(serde_json::Value::as_array) {
                for var in env {
                    if let Some(name) = var.get("name").and_then(serde_json::Value::as_str) {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }
    Ok(names)
}

/// 7d — re-register the worker with Restate so any handler added since
/// the last registration is reachable. Best-effort: a missing CLI or a
/// registration error warns rather than failing the ship (forks not on
/// Restate Cloud, expired SSO token, etc.).
fn reregister(cfg: &PowerPushConfig, dry_run: bool) {
    let url = cfg.workflows_url_resolved();
    if dry_run {
        eprintln!(
            "DRY-RUN: would re-register the worker with Restate (devx restate register {url})"
        );
        return;
    }
    // The admin REST API path (RESTATE_ADMIN_URL + RESTATE_ADMIN_TOKEN) needs
    // no `restate` CLI, so only require the CLI when those env vars are absent.
    let has_admin_api = !std::env::var("RESTATE_ADMIN_URL")
        .unwrap_or_default()
        .trim()
        .is_empty()
        && !std::env::var("RESTATE_ADMIN_TOKEN")
            .unwrap_or_default()
            .trim()
            .is_empty();
    if !has_admin_api && !tool_present("restate") {
        eprintln!("WARN: no RESTATE_ADMIN_URL/TOKEN and `restate` CLI not on PATH; skipping re-register (register manually if on Restate Cloud)");
        return;
    }
    // Pass the resolved URL explicitly so the ship targets the same host
    // the dry-run printed, independent of how `restate_register` would
    // re-resolve it from the env.
    if let Err(err) = super::restate_register(Some(&url)) {
        eprintln!("WARN: Restate re-register failed (continuing): {err:#}");
    }
}

/// 8 — curl the public landing and grep a fixed phrase. Best-effort:
/// reports, never fails the ship.
fn smoke_check(cfg: &PowerPushConfig, dry_run: bool) {
    let url = format!("https://www.{}/", cfg.primary_domain);
    if dry_run {
        eprintln!("DRY-RUN: would smoke-check {url}");
        return;
    }
    if !tool_present("curl") {
        eprintln!("WARN: `curl` not on PATH; skipping smoke check of {url}");
        return;
    }
    // Confirm the public landing is non-empty by grepping a stable phrase.
    let phrase = "home";
    match Command::new("curl").args(["-fsS", &url]).output() {
        Ok(out) if out.status.success() => {
            let body = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            if body.contains(phrase) {
                eprintln!("==> smoke check OK ({url})");
            } else {
                eprintln!("WARN: {url} returned 200 but the expected phrase was absent — inspect the page");
            }
        }
        Ok(out) => eprintln!("WARN: smoke check non-2xx for {url}: {}", out.status),
        Err(err) => eprintln!("WARN: smoke check could not reach {url}: {err}"),
    }
    eprintln!("==> workflows-service has no public /; confirm it is ready:");
    eprintln!(
        "    kubectl --context {} -n {} get pods -l app={WORKFLOWS_DEPLOYMENT}",
        cfg.context, cfg.namespace
    );
}

/// Reclaim the local `:dev` images — they live in GAR + the cluster
/// now. `docker rmi` ignores errors (the images may already be gone).
fn reclaim(dry_run: bool) {
    if dry_run {
        eprintln!("DRY-RUN: would docker rmi {WEB_IMAGE} {WORKFLOWS_SERVICE_IMAGE}");
        return;
    }
    let _ = Command::new("docker")
        .arg("rmi")
        .arg(WEB_IMAGE)
        .arg(WORKFLOWS_SERVICE_IMAGE)
        .status();
}

// ---------- small shared helpers ----------

/// A `kubectl` invocation pinned to the prod context and namespace.
fn kubectl(cfg: &PowerPushConfig) -> Command {
    let mut cmd = kubectl_ctx(cfg);
    cmd.arg("-n").arg(&cfg.namespace);
    cmd
}

/// A `kubectl` invocation pinned to the prod context only (for the
/// kustomize `diff`/`apply`, which carry their own namespaces).
fn kubectl_ctx(cfg: &PowerPushConfig) -> Command {
    let mut cmd = Command::new("kubectl");
    cmd.arg("--context").arg(&cfg.context);
    cmd
}

/// `kubectl get <kind> <name> -o json`, parsed.
fn kubectl_json(cfg: &PowerPushConfig, kind: &str, name: &str) -> Result<serde_json::Value> {
    let out = kubectl(cfg)
        .arg("get")
        .arg(kind)
        .arg(name)
        .arg("-o")
        .arg("json")
        .output()
        .with_context(|| format!("run kubectl get {kind} {name}"))?;
    if !out.status.success() {
        bail!(
            "kubectl get {kind} {name} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    serde_json::from_slice(&out.stdout)
        .with_context(|| format!("parse `kubectl get {kind} {name} -o json`"))
}

/// Wait on both Deployments' rollouts at the given timeout.
fn wait_rollouts(cfg: &PowerPushConfig, dry_run: bool, timeout: &str) -> Result<()> {
    for deployment in [WEB_DEPLOYMENT, WORKFLOWS_DEPLOYMENT] {
        exec(
            dry_run,
            kubectl(cfg)
                .arg("rollout")
                .arg("status")
                .arg(format!("deployment/{deployment}"))
                .arg(format!("--timeout={timeout}")),
        )?;
    }
    Ok(())
}

/// Confirm the resolved `kubectl` context exists before any prod call.
/// A deterministic ship must not silently land on whatever context
/// happens to be current.
fn verify_context(cfg: &PowerPushConfig, dry_run: bool) -> Result<()> {
    if dry_run {
        eprintln!("DRY-RUN: would pin kubectl context → {}", cfg.context);
        return Ok(());
    }
    let out = Command::new("kubectl")
        .args(["config", "get-contexts", "-o", "name"])
        .output()
        .context("kubectl config get-contexts")?;
    if !out.status.success() {
        bail!(
            "kubectl config get-contexts failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let contexts = String::from_utf8_lossy(&out.stdout);
    if !contexts.lines().any(|c| c == cfg.context) {
        bail!(
            "kubectl context '{}' not found. Get prod credentials \
             (`gcloud container clusters get-credentials …`) or set NAVIGATOR_GKE_CONTEXT \
             to the right context name.",
            cfg.context
        );
    }
    eprintln!("==> pinning kubectl context → {}", cfg.context);
    Ok(())
}

/// Warn (don't fail) when the working tree is dirty: uncommitted work
/// won't ship because the image tag is HEAD's SHA.
fn warn_if_dirty() -> Result<()> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .context("git status --porcelain")?;
    if out.status.success() && !out.stdout.is_empty() {
        eprintln!(
            "WARN: working tree is dirty — uncommitted changes will NOT ship \
             (image tag = HEAD's SHA). Commit first if they should go out."
        );
    }
    Ok(())
}

/// True when `tool` is on PATH (same probe as `require_tools`, but
/// boolean — for best-effort steps that downgrade to a warning).
fn tool_present(tool: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run a command, or — under `--dry-run` — print it instead.
fn exec(dry_run: bool, cmd: &mut Command) -> Result<()> {
    if dry_run {
        eprintln!("DRY-RUN $ {}", render_cmd(cmd));
        Ok(())
    } else {
        run(cmd)
    }
}

/// Render a `Command` as a copy-pasteable shell line for `--dry-run`.
fn render_cmd(cmd: &Command) -> String {
    let mut out = cmd.get_program().to_string_lossy().into_owned();
    for arg in cmd.get_args() {
        out.push(' ');
        out.push_str(&arg.to_string_lossy());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> PowerPushConfig {
        PowerPushConfig {
            project_id: "my-org-prod".into(),
            location: "us-west4".into(),
            cluster: "navigator".into(),
            namespace: "navigator".into(),
            primary_domain: "example.com".into(),
            overlay_dir: None,
            secret_name: "navigator-web-secrets".into(),
            workflows_url: None,
            context: "gke_my-org-prod_us-west4_navigator-prod".into(),
        }
    }

    #[test]
    fn derived_names_follow_the_workspace_convention() {
        let cfg = sample_config();
        assert_eq!(
            cfg.registry(),
            "us-west4-docker.pkg.dev/my-org-prod/navigator"
        );
        assert_eq!(
            cfg.web_image("abc1234"),
            "us-west4-docker.pkg.dev/my-org-prod/navigator/navigator-web:abc1234"
        );
        assert_eq!(
            cfg.workflows_image("abc1234"),
            "us-west4-docker.pkg.dev/my-org-prod/navigator/navigator-workflows-service:abc1234"
        );
        assert_eq!(cfg.source_bucket(), "gs://my-org-prod-source");
        assert_eq!(
            cfg.bundle_object("abc1234"),
            "gs://my-org-prod-source/navigator-abc1234.bundle"
        );
    }

    #[test]
    fn workflows_url_derives_from_primary_domain_when_unset() {
        // The 2026-06-10 ship symptom: a domain is configured but the
        // explicit URL is not. The resolved URL must be the real
        // ingress derived from the domain, never the
        // `workflows.example.com` placeholder.
        let cfg = PowerPushConfig {
            primary_domain: "neonlaw.com".into(),
            workflows_url: None,
            ..sample_config()
        };
        assert_eq!(
            cfg.workflows_url_resolved(),
            "https://workflows.neonlaw.com/"
        );
    }

    #[test]
    fn workflows_url_prefers_explicit_override() {
        let cfg = PowerPushConfig {
            workflows_url: Some("https://workflows.neonlaw.com/".into()),
            ..sample_config()
        };
        assert_eq!(
            cfg.workflows_url_resolved(),
            "https://workflows.neonlaw.com/"
        );
    }

    #[test]
    fn derived_context_matches_gke_get_credentials_naming() {
        assert_eq!(
            derived_context("my-org-prod", "us-west4", "navigator"),
            "gke_my-org-prod_us-west4_navigator-prod"
        );
    }

    #[test]
    fn required_secret_keys_scrapes_the_invariant_literals() {
        // Mirrors the real shape of web/src/config.rs invariant lines,
        // including a multi-line string and a prose false-positive.
        let src = r#"
            bail!(
                "RESTATE_BROKER_URL must be set (otherwise the in-memory \
                 broker would silently swallow jobs)"
            );
            ensure!(cfg.has("DOCUSIGN_HMAC_KEY"), "DOCUSIGN_HMAC_KEY must be set (otherwise forgeable)");
            // a comment explaining that something must be set should NOT match
            "SENDGRID_API_KEY must be set";
        "#;
        let keys: Vec<String> = required_secret_keys_with_triggers(src)
            .into_iter()
            .map(|(key, _trigger)| key)
            .collect();
        assert_eq!(
            keys,
            vec![
                "DOCUSIGN_HMAC_KEY".to_string(),
                "RESTATE_BROKER_URL".to_string(),
                "SENDGRID_API_KEY".to_string(),
            ]
        );
    }

    #[test]
    fn conditional_invariants_carry_their_trigger_and_gate_on_it() {
        // Mirrors the real shape: two conditional OIDC invariants nested
        // under an `if get("OIDC_JWKS_URL")` block, plus one unconditional.
        let src = r#"
            if get("OIDC_JWKS_URL").is_some_and(|s| !s.is_empty()) {
                violations.push(
                    "OIDC_AUDIENCE must be set when OIDC_JWKS_URL is (otherwise \
                     bearer tokens are accepted without audience pinning)"
                        .into(),
                );
                violations.push(
                    "OIDC_ISSUER must be set when OIDC_JWKS_URL is (otherwise the \
                     bearer token's issuer is unverified)"
                        .into(),
                );
            }
            "SENDGRID_API_KEY must be set (otherwise outbound email is dropped)";
        "#;
        let parsed = required_secret_keys_with_triggers(src);
        assert_eq!(
            parsed,
            vec![
                (
                    "OIDC_AUDIENCE".to_string(),
                    Some("OIDC_JWKS_URL".to_string())
                ),
                ("OIDC_ISSUER".to_string(), Some("OIDC_JWKS_URL".to_string())),
                ("SENDGRID_API_KEY".to_string(), None),
            ]
        );

        // OIDC_JWKS_URL not configured → the two conditional keys are NOT
        // required; only the unconditional one is. This is the prod case
        // where the optional JWKS bearer path is off.
        let without_jwks: BTreeSet<String> =
            ["SENDGRID_API_KEY"].into_iter().map(String::from).collect();
        assert_eq!(
            effective_required_keys(&parsed, &without_jwks),
            vec!["SENDGRID_API_KEY".to_string()]
        );
        assert!(missing_keys(
            &effective_required_keys(&parsed, &without_jwks),
            &without_jwks
        )
        .is_empty());

        // OIDC_JWKS_URL configured but its companions absent → both
        // conditional keys are now required and reported missing.
        let with_jwks: BTreeSet<String> = ["SENDGRID_API_KEY", "OIDC_JWKS_URL"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            missing_keys(&effective_required_keys(&parsed, &with_jwks), &with_jwks),
            vec!["OIDC_AUDIENCE".to_string(), "OIDC_ISSUER".to_string()]
        );
    }

    #[test]
    fn required_secret_keys_dedupes() {
        let src = r#""SENDGRID_API_KEY must be set"; "SENDGRID_API_KEY must be set again";"#;
        assert_eq!(
            required_secret_keys_with_triggers(src),
            vec![("SENDGRID_API_KEY".to_string(), None)]
        );
    }

    #[test]
    fn missing_keys_reports_only_the_unsatisfied() {
        let required = vec![
            "DOCUSIGN_HMAC_KEY".to_string(),
            "NAVIGATOR_OPA_URL".to_string(),
            "SENDGRID_API_KEY".to_string(),
        ];
        // OPA url provided via deployment env, SendGrid key via the Secret.
        let satisfied: BTreeSet<String> = ["NAVIGATOR_OPA_URL", "SENDGRID_API_KEY"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            missing_keys(&required, &satisfied),
            vec!["DOCUSIGN_HMAC_KEY".to_string()]
        );
    }

    #[test]
    fn missing_keys_empty_when_all_satisfied() {
        let required = vec!["A".to_string(), "B".to_string()];
        let satisfied: BTreeSet<String> = ["A", "B", "C"].into_iter().map(String::from).collect();
        assert!(missing_keys(&required, &satisfied).is_empty());
    }
}
