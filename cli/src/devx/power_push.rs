//! `devx power-push` — one-shot "roll prod onto today's image".
//!
//! This is the deterministic, in-binary form of the production rollout
//! path documented in `docs/cloud-operations.md`. That public doc keeps
//! the prose rationale ("why each step, in this order"); this module is
//! the executable that runs the steps so an operator types one command
//! instead of pasting several shell blocks.
//!
//! **CI builds and publishes; power-push only rolls.** The daily
//! `deploy.yml` tag flow builds both images and publishes them to
//! public `ghcr.io` tagged `YY.MM.DD`; this module never builds or
//! pushes an image — it pins the running cluster to an
//! already-published tag.
//!
//! Two flows, matching the documented rollout path:
//!
//! - **Roll** (default): resolve the `YY.MM.DD` ghcr tag to deploy (the
//!   latest published, or `--tag`) → confirm the prod Secret satisfies
//!   the new binary's boot invariants → roll out BOTH deployments at
//!   that tag → re-register the worker with Restate. Both deployments
//!   are pinned to the **same** tag — never a version skew.
//! - **No-rebuild restart** (`--restart-only`): after a Secret value
//!   was rotated, `kubectl rollout restart` BOTH deployments that
//!   `envFrom` the Secret so the pods re-read it (pods cache `envFrom`
//!   at start and never reload).
//!
//! Everything that varies per deployment flows through `.env` — there
//! is no literal project ID, region, domain, or ghcr owner in this file
//! (same contract as `docs/env-driven-devx.md`). See [`PowerPushConfig::from_env`].
//!
//! ## What this does NOT do
//!
//! - It never builds or pushes images. CI owns that; power-push rolls a
//!   tag CI already published. The public GitHub `YY.MM.DD` tag is the
//!   source restore point, so there is no git-bundle archive step.
//! - It never auto-patches a prod Secret. The invariant check *aborts*
//!   with the exact `kubectl patch` to run when a required key is
//!   missing — generating and writing a prod secret silently is a
//!   judgment call left to the operator.
//!
//! ## Testing
//!
//! The shell-out orchestration needs a real cluster, so it isn't
//! unit-tested. The pure pieces — env-driven config, the ghcr image-URL
//! formulas, the latest-tag selector, the required-key parser, and the
//! missing-key diff — are covered by the `tests` module below.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::{env_string, require_auth, require_tools, run, workspace_root};

/// In-cluster Deployment + container names. These are workspace
/// conventions (the GKE overlay names them); they are not per-deploy
/// configuration, so they stay as constants rather than env vars.
const WEB_DEPLOYMENT: &str = "navigator-web";
const WEB_CONTAINER: &str = "web";
const WORKFLOWS_DEPLOYMENT: &str = "workflows-service";
const WORKFLOWS_CONTAINER: &str = "worker";

/// The canonical ghcr owner. The default when `NAVIGATOR_GHCR_OWNER` is
/// unset — a fork overrides it via that env var rather than editing this
/// constant, keeping the white-label seam intact.
const DEFAULT_GHCR_OWNER: &str = "neon-law-foundation";

/// Every per-deployment value `power-push` reads, resolved once from
/// the environment. Required values bail when unset (fail fast — never
/// substitute a project-internal default). Optional values fall back
/// to a documented workspace default or to `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPushConfig {
    /// Target GCP project ID (`NAVIGATOR_GCP_PROJECT_ID`). Used to
    /// derive the `kubectl` context.
    pub project_id: String,
    /// Region the cluster lives in (`NAVIGATOR_GCP_LOCATION`). Used to
    /// derive the `kubectl` context.
    pub location: String,
    /// Cluster name (`NAVIGATOR_GKE_CLUSTER_NAME`). Used to derive the
    /// `kubectl` context.
    pub cluster: String,
    /// ghcr owner (org/user) the images live under, lowercased
    /// (`NAVIGATOR_GHCR_OWNER`) — the `<owner>` in
    /// `ghcr.io/<owner>/navigator-web`. Defaults to `neon-law-foundation`
    /// (the canonical org); a fork overrides it to ship to its own org's
    /// ghcr, so the value stays env-driven rather than hard-coded.
    pub ghcr_owner: String,
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
        // ghcr image names are lowercase; lowercase the owner so a
        // mixed-case org (e.g. `Neon-Law-Foundation`) still resolves.
        // Defaults to the canonical org; a fork overrides via env.
        let ghcr_owner = optional_env("NAVIGATOR_GHCR_OWNER")
            .unwrap_or_else(|| DEFAULT_GHCR_OWNER.to_string())
            .to_ascii_lowercase();
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
            ghcr_owner,
            namespace,
            primary_domain,
            overlay_dir,
            secret_name,
            workflows_url,
            context,
        })
    }

    /// ghcr registry prefix — `ghcr.io/<owner>`. CI publishes every
    /// image under this owner; power-push rolls the cluster onto them.
    #[must_use]
    pub fn registry(&self) -> String {
        format!("ghcr.io/{}", self.ghcr_owner)
    }

    /// Published `navigator-web` image URL at the `YY.MM.DD` `tag`.
    #[must_use]
    pub fn web_image(&self, tag: &str) -> String {
        format!("{}/navigator-web:{tag}", self.registry())
    }

    /// Published `navigator-workflows-service` image URL at the
    /// `YY.MM.DD` `tag`.
    #[must_use]
    pub fn workflows_image(&self, tag: &str) -> String {
        format!("{}/navigator-workflows-service:{tag}", self.registry())
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
#[derive(Debug, Clone, Default)]
pub struct PowerPushOpts {
    /// Print every command instead of running it.
    pub dry_run: bool,
    /// No-rebuild path: just `kubectl rollout restart` both
    /// deployments (Secret-value rotation), then exit.
    pub restart_only: bool,
    /// The `YY.MM.DD[.HH]` ghcr tag to roll onto. `None` resolves the
    /// latest published tag from ghcr.
    pub tag: Option<String>,
}

/// Entry point for `Cmd::PowerPush`.
pub fn run_power_push(opts: &PowerPushOpts) -> Result<()> {
    let cfg = PowerPushConfig::from_env()?;
    if opts.restart_only {
        return restart_only(&cfg, opts.dry_run);
    }
    roll(&cfg, opts)
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

/// Roll the cluster onto an already-published `YY.MM.DD` ghcr tag.
/// CI built and published the images; this only updates the cluster:
/// resolve the tag → confirm the Secret satisfies the new binary's boot
/// invariants → pin BOTH deployments AND every trigger `CronJob` to that
/// tag → wait → re-register the worker with Restate → smoke-check. No
/// build, no push, no skew — every navigator image in sync at one tag.
fn roll(cfg: &PowerPushConfig, opts: &PowerPushOpts) -> Result<()> {
    require_tools(&["kubectl"])?;
    // Authenticated, not just installed: gcloud carries the GKE
    // credentials the pinned kubectl context resolves against. Restate
    // re-register downgrades to a warning when its CLI / admin API is
    // absent, so it is not required here.
    require_auth(&["gcloud"])?;
    let dry_run = opts.dry_run;

    // 1. Pre-flight — confirm the prod context resolves before any call.
    verify_context(cfg, dry_run)?;

    // 2. Resolve the YY.MM.DD tag to roll: explicit `--tag`, else the
    //    latest published tag on ghcr. Both deployments get the SAME tag.
    let tag = match &opts.tag {
        Some(t) => {
            validate_release_tag(t)?;
            t.clone()
        }
        None => resolve_latest_tag(cfg, dry_run)?,
    };
    let web_remote = cfg.web_image(&tag);
    let workflows_remote = cfg.workflows_image(&tag);
    eprintln!(
        "==> rolling {} ({}) onto {tag}\n      {web_remote}\n      {workflows_remote}",
        cfg.project_id, cfg.context
    );

    // 2b. Fail fast if the two service images aren't actually published at
    //     this tag — pinning a deployment to a missing tag wedges it in
    //     ImagePullBackOff. Verify on every live run, regardless of how the
    //     tag was chosen: the auto-resolve path picks the latest tag from
    //     navigator-web alone, so without this the workflows-service image is
    //     never checked, and a partial CI publish (web tag lands, the
    //     workflows-service publish leg fails — they run as a fail-fast:false
    //     matrix) would still roll workflows-service onto a missing tag.
    //     Skipped only in dry-run, where `resolve_latest_tag` returns a
    //     placeholder with nothing to verify against.
    if !dry_run {
        ensure_tag_published(&cfg.ghcr_owner, "navigator-web", &tag)?;
        ensure_tag_published(&cfg.ghcr_owner, "navigator-workflows-service", &tag)?;
    }

    // 3. Sync the manifest (only when an overlay is configured).
    sync_overlay(cfg, dry_run)?;

    // 4. Confirm the prod Secret satisfies the new binary's invariants.
    ensure_secret_invariants(cfg, dry_run)?;

    // 5. Pin BOTH deployment images to the same tag, then wait on both
    //    rollouts.
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

    // 5b. Pin every trigger CronJob to the same tag so a roll is atomic
    //     across ALL navigator images, not just the two services. CronJobs
    //     don't "roll" — the new image takes effect on the next scheduled
    //     run — so there is nothing to wait on.
    pin_cronjob_images(cfg, &tag, dry_run)?;

    // 6. Re-register the worker with Restate (best-effort).
    reregister(cfg, dry_run);

    // 7. Smoke-check the public surface (best-effort).
    smoke_check(cfg, dry_run);

    eprintln!("==> power-push complete: {tag} live in {}", cfg.project_id);
    Ok(())
}

/// Re-pin every navigator trigger `CronJob` to `tag`. Discovers the
/// `CronJobs` from the live cluster and re-points any container whose image
/// is one of ours (`ghcr.io/<owner>/navigator-*`) — no hard-coded list,
/// so a newly added trigger is covered automatically. Each image is
/// re-pinned only after confirming `tag` is actually published for it;
/// a trigger whose image hasn't published `tag` yet is skipped with a
/// warning rather than wedged in `ImagePullBackOff`.
fn pin_cronjob_images(cfg: &PowerPushConfig, tag: &str, dry_run: bool) -> Result<()> {
    let prefix = format!("{}/", cfg.registry());
    if dry_run {
        eprintln!("DRY-RUN: would re-pin every {prefix}navigator-* CronJob image to {tag}");
        return Ok(());
    }
    let list = kubectl_list_json(cfg, "cronjobs")?;
    let Some(items) = list.get("items").and_then(serde_json::Value::as_array) else {
        eprintln!("==> no CronJobs found; nothing to pin");
        return Ok(());
    };
    let mut pinned = 0u32;
    for item in items {
        let Some(name) = item
            .pointer("/metadata/name")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let containers = item
            .pointer("/spec/jobTemplate/spec/template/spec/containers")
            .and_then(serde_json::Value::as_array);
        let Some(containers) = containers else {
            continue;
        };
        for c in containers {
            let (Some(cname), Some(image)) = (
                c.get("name").and_then(serde_json::Value::as_str),
                c.get("image").and_then(serde_json::Value::as_str),
            ) else {
                continue;
            };
            // Ours only: `ghcr.io/<owner>/navigator-<something>:<tag>`.
            if !image.starts_with(&prefix) {
                continue;
            }
            let base = image.rsplit_once(':').map_or(image, |(b, _)| b);
            let short = base.strip_prefix(&prefix).unwrap_or(base);
            if ghcr_tag_exists(&cfg.ghcr_owner, short, tag) {
                let target = format!("{base}:{tag}");
                exec(
                    false,
                    kubectl(cfg)
                        .arg("set")
                        .arg("image")
                        .arg(format!("cronjob/{name}"))
                        .arg(format!("{cname}={target}")),
                )?;
                pinned += 1;
            } else {
                eprintln!(
                    "WARN: {short} has no {tag} tag on ghcr — leaving CronJob/{name} on {image}"
                );
            }
        }
    }
    eprintln!("==> pinned {pinned} trigger CronJob image(s) to {tag}");
    Ok(())
}

/// True when `tag` is the `YY.MM.DD` release shape — three dot-separated
/// two-digit groups (e.g. `26.06.23`) — with an optional `.HH` fourth
/// group for an ad-hoc same-day release (e.g. `26.06.25.14`).
#[must_use]
pub fn is_release_tag(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('.').collect();
    (parts.len() == 3 || parts.len() == 4)
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Reject a `--tag` that is not a `YY.MM.DD[.HH]` release tag — rolling a
/// `latest` or a `ci-<sha>` tag onto a workload is exactly the
/// un-auditable deploy we forbid.
fn validate_release_tag(tag: &str) -> Result<()> {
    if is_release_tag(tag) {
        Ok(())
    } else {
        bail!(
            "--tag must be a YY.MM.DD release tag, optionally with an .HH suffix for an ad-hoc same-day release (e.g. 26.06.23 or 26.06.25.14), got `{tag}`"
        );
    }
}

/// The newest `YY.MM.DD[.HH]` tag in `tags`. Zero-padded `YY.MM.DD` sorts
/// lexicographically the same as chronologically, and an `.HH` ad-hoc
/// suffix (e.g. `26.06.25.14`) sorts after the bare same-day tag it
/// extends, so `max` is the latest. Non-release tags (`latest`,
/// `ci-<sha>`) are ignored.
#[must_use]
pub fn pick_latest_release_tag(tags: &[String]) -> Option<String> {
    tags.iter().filter(|t| is_release_tag(t)).max().cloned()
}

/// Resolve the latest published `YY.MM.DD` tag from ghcr. In `--dry-run`
/// we don't touch the network — print a placeholder so the planned
/// `set image` commands still render.
fn resolve_latest_tag(cfg: &PowerPushConfig, dry_run: bool) -> Result<String> {
    if dry_run {
        eprintln!(
            "DRY-RUN: would resolve the latest YY.MM.DD tag from ghcr.io/{}/navigator-web",
            cfg.ghcr_owner
        );
        return Ok("<latest-ghcr-tag>".to_string());
    }
    let tags = fetch_ghcr_tags(&cfg.ghcr_owner, "navigator-web")?;
    pick_latest_release_tag(&tags).ok_or_else(|| {
        anyhow::anyhow!(
            "no YY.MM.DD release tag on ghcr.io/{}/navigator-web — has the daily deploy published one yet?",
            cfg.ghcr_owner
        )
    })
}

/// List a public ghcr package's tags anonymously: mint a pull-scoped
/// token, then GET `/v2/<owner>/<image>/tags/list`. Public packages need
/// no credential — the same path GKE's anonymous pulls take. Builds a
/// private current-thread runtime so the rest of `power-push` stays sync.
fn fetch_ghcr_tags(owner: &str, image: &str) -> Result<Vec<String>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for ghcr tag resolution")?;
    let repo = format!("{owner}/{image}");
    runtime.block_on(async move {
        let client = reqwest::Client::new();
        let token_url = format!("https://ghcr.io/token?scope=repository:{repo}:pull");
        let token_body: serde_json::Value = client
            .get(&token_url)
            .send()
            .await
            .context("request ghcr pull token")?
            .json()
            .await
            .context("parse ghcr token response")?;
        let token = token_body
            .get("token")
            .and_then(serde_json::Value::as_str)
            .context("ghcr token missing from response")?;
        let list_url = format!("https://ghcr.io/v2/{repo}/tags/list");
        let resp = client
            .get(&list_url)
            .bearer_auth(token)
            .send()
            .await
            .context("request ghcr tags/list")?;
        if !resp.status().is_success() {
            bail!(
                "ghcr tags/list for {repo} returned {} — is the package public?",
                resp.status()
            );
        }
        let body: serde_json::Value = resp.json().await.context("parse ghcr tags/list")?;
        let tags = body
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(tags)
    })
}

/// Whether `tag` is published for `ghcr.io/<owner>/<image>`. Conservative
/// on error: a failed lookup returns `false` (treat as "can't confirm →
/// don't pin"), so it never green-lights a tag it couldn't verify.
fn ghcr_tag_exists(owner: &str, image: &str, tag: &str) -> bool {
    fetch_ghcr_tags(owner, image).is_ok_and(|tags| tags.iter().any(|t| t == tag))
}

/// Bail unless `tag` is published for `ghcr.io/<owner>/<image>`. Used to
/// fail a roll fast — before any `kubectl set image` — when a service
/// image is missing the requested tag (which would otherwise wedge the
/// deployment in `ImagePullBackOff`). Distinguishes a lookup error (network)
/// from an honestly-absent tag.
fn ensure_tag_published(owner: &str, image: &str, tag: &str) -> Result<()> {
    let tags = fetch_ghcr_tags(owner, image)
        .with_context(|| format!("check ghcr.io/{owner}/{image}:{tag} is published"))?;
    if tags.iter().any(|t| t == tag) {
        Ok(())
    } else {
        bail!(
            "ghcr.io/{owner}/{image}:{tag} is not published — power-push only rolls published \
             tags. Publish it via the daily deploy (or pick a tag that exists) first."
        );
    }
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

/// `kubectl get <kind> -o json` for a whole collection (no name), parsed.
/// The result is a `List` whose `items` array the caller walks.
fn kubectl_list_json(cfg: &PowerPushConfig, kind: &str) -> Result<serde_json::Value> {
    let out = kubectl(cfg)
        .arg("get")
        .arg(kind)
        .arg("-o")
        .arg("json")
        .output()
        .with_context(|| format!("run kubectl get {kind}"))?;
    if !out.status.success() {
        bail!(
            "kubectl get {kind} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    serde_json::from_slice(&out.stdout)
        .with_context(|| format!("parse `kubectl get {kind} -o json`"))
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
            ghcr_owner: "neon-law-foundation".into(),
            namespace: "navigator".into(),
            primary_domain: "example.com".into(),
            overlay_dir: None,
            secret_name: "navigator-web-secrets".into(),
            workflows_url: None,
            context: "gke_my-org-prod_us-west4_navigator-prod".into(),
        }
    }

    #[test]
    fn derived_names_target_ghcr_at_the_owner() {
        let cfg = sample_config();
        assert_eq!(cfg.registry(), "ghcr.io/neon-law-foundation");
        assert_eq!(
            cfg.web_image("26.06.23"),
            "ghcr.io/neon-law-foundation/navigator-web:26.06.23"
        );
        assert_eq!(
            cfg.workflows_image("26.06.23"),
            "ghcr.io/neon-law-foundation/navigator-workflows-service:26.06.23"
        );
    }

    #[test]
    fn is_release_tag_matches_only_yy_mm_dd() {
        assert!(is_release_tag("26.06.23"));
        assert!(is_release_tag("00.01.09"));
        assert!(is_release_tag("26.06.25.14")); // ad-hoc same-day .HH suffix
        assert!(is_release_tag("26.06.25.00"));
        assert!(!is_release_tag("latest"));
        assert!(!is_release_tag("ci-6a5f96a"));
        assert!(!is_release_tag("2026.06.23")); // four-digit year
        assert!(!is_release_tag("26.6.23")); // unpadded month
        assert!(!is_release_tag("26.06")); // too few groups
        assert!(!is_release_tag("26.06.25.4")); // unpadded hour
        assert!(!is_release_tag("26.06.25.14.30")); // too many groups
    }

    #[test]
    fn pick_latest_release_tag_takes_the_newest_and_ignores_non_releases() {
        let tags = vec![
            "latest".to_string(),
            "26.06.10".to_string(),
            "ci-deadbeef".to_string(),
            "26.06.23".to_string(),
            "26.05.31".to_string(),
        ];
        assert_eq!(pick_latest_release_tag(&tags), Some("26.06.23".to_string()));
        // An ad-hoc `.HH` release sorts after the bare same-day tag.
        assert_eq!(
            pick_latest_release_tag(&[
                "26.06.25".to_string(),
                "26.06.25.14".to_string(),
                "26.06.10".to_string(),
            ]),
            Some("26.06.25.14".to_string())
        );
        assert_eq!(
            pick_latest_release_tag(&["latest".to_string(), "ci-x".to_string()]),
            None
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
