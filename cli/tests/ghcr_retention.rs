//! Guard the GHCR retention sweep — the one workflow in this repository that
//! DELETES published artifacts.
//!
//! Every other workflow adds: images, archives, check runs. This one removes,
//! unattended, on a clock, and what it removes cannot be recovered — a deleted
//! container version is gone, and `ops ship` refuses a tag the registry no
//! longer holds. So the properties that keep it from deleting something
//! load-bearing are asserted here rather than left to review, because a sweep
//! that deleted too much would report success doing it.
//!
//! Three of those properties are the safety floor, and they are independent:
//!
//!   1. an AGE bound, so a version has to be genuinely old to qualify;
//!   2. a COUNT floor, so the newest versions survive however old they are —
//!      this is what stops a quiet month from deleting the version production is
//!      running;
//!   3. a `latest` exemption, so the mutable pointer every published image
//!      carries is never orphaned.
//!
//! Retention was count-only before this workflow existed, enforced by Artifact
//! Registry `cleanupPolicies` that GHCR never reads (`docs/gitops.md` → "Image
//! retention"). The count floor is carried forward deliberately: it is the half
//! that cannot expire.

use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

/// Delete nothing newer than this. Mirrors `CUTOFF_DAYS` in the workflow.
const CUTOFF_DAYS: u32 = 30;
/// Keep at least this many of every image's newest versions, whatever their age.
const RETAINED_VERSIONS: u32 = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root is cli/'s parent")
        .to_path_buf()
}

fn source() -> String {
    let path = repo_root().join(".github/workflows/ghcr-retention.yml");
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn workflow() -> Value {
    serde_yaml::from_str(&source()).expect("ghcr-retention.yml parses as YAML")
}

/// `on` is the YAML 1.1 boolean `true`, so `serde_yaml` keys it as a bool.
/// Reading it by name silently finds nothing and every assertion passes
/// vacuously.
fn triggers() -> serde_yaml::Mapping {
    let workflow = workflow();
    workflow
        .get(Value::Bool(true))
        .or_else(|| workflow.get("on"))
        .expect("ghcr-retention.yml must declare a trigger block")
        .as_mapping()
        .expect("the trigger block must be a mapping")
        .clone()
}

fn sweep_script() -> String {
    let workflow = workflow();
    workflow["jobs"]["sweep"]["steps"]
        .as_sequence()
        .expect("the sweep job must declare steps")
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 01:11 UTC, and 1:11 rather than 1:00 on purpose: GitHub delays scheduled runs
/// when the hosted-runner queue is deep, and the top of the hour is when it is
/// deepest. The old nightly release held this slot; the sweep inherits it now
/// that publishing runs from a tag.
#[test]
fn the_sweep_runs_nightly_at_0111_utc() {
    let crons: Vec<String> = triggers()
        .get(Value::String("schedule".into()))
        .expect("the sweep must run on a clock — retention nobody triggers is retention nobody has")
        .as_sequence()
        .expect("the schedule trigger must be a sequence")
        .iter()
        .filter_map(|entry| entry["cron"].as_str().map(str::to_string))
        .collect();

    assert!(
        crons.iter().any(|cron| cron == "11 1 * * *"),
        "the GHCR sweep must run at 01:11 UTC. Got: {crons:?}"
    );
}

/// A destructive unattended job must be rehearsable. The dispatch exists so the
/// sweep can be watched reporting what it WOULD delete before a night deletes
/// it — the only way to prove a change to this workflow without waiting for the
/// clock and finding out from the registry.
#[test]
fn the_sweep_can_be_rehearsed_without_deleting() {
    assert!(
        triggers().contains_key(Value::String("workflow_dispatch".into())),
        "the sweep must keep `workflow_dispatch`: a delete job you cannot rehearse is one whose \
         first proof is a registry you cannot restore"
    );

    let script = sweep_script();
    assert!(
        script.contains("DRY_RUN"),
        "the sweep must honour a dry-run mode, and the dispatch must be able to set it"
    );
}

/// THE COUNT FLOOR. The property an age-only rule cannot provide.
///
/// Publishing runs from a tag, so nothing guarantees a release this month. An
/// age-only sweep would then delete the exact versions production is running:
/// serving pods survive it, because they already pulled, but a restart, a
/// reschedule, or a node replacement cannot pull an image that is gone, and
/// `ops ship --tag <previous>` — the documented rollback — refuses a tag the
/// registry no longer holds. A count cannot expire.
#[test]
fn the_newest_versions_survive_any_age() {
    let script = sweep_script();

    assert!(
        script.contains(&format!("RETAINED_VERSIONS={RETAINED_VERSIONS}")),
        "the sweep must keep the newest {RETAINED_VERSIONS} versions of every image whatever their \
         age — the floor that stops a quiet month deleting what production runs"
    );
    assert!(
        script.contains(&format!("CUTOFF_DAYS={CUTOFF_DAYS}")),
        "the sweep must bound deletion by age as well as count"
    );
}

/// The `latest` pointer is published on every image and must never be orphaned.
/// Deleting the version it points at leaves a tag resolving to nothing, which
/// fails at pull time rather than at sweep time.
#[test]
fn the_latest_pointer_is_never_deleted() {
    assert!(
        sweep_script().contains("latest"),
        "the sweep must exempt the version tagged `latest`: it is a published pointer, and \
         deleting what it points at breaks a pull rather than the sweep"
    );
}

/// The sweep may only touch packages this repository publishes.
///
/// A GHCR package is owned by the ORG, and the org owns packages other
/// repositories push. Enumerating `/orgs/{org}/packages` and deleting by age
/// alone would sweep those too — a workflow in the Navigator repository deleting
/// another repository's images, on a clock, with no signal that it had. So the
/// candidate list is filtered by the linked repository, and an unlinked package
/// (`repository: null`) is skipped rather than assumed to be ours.
#[test]
fn the_sweep_only_touches_this_repositorys_packages() {
    let script = sweep_script();

    assert!(
        script.contains(".repository.name"),
        "the sweep must filter candidate packages by their linked repository — the org owns \
         packages this repository did not publish"
    );
    assert!(
        script.contains("container"),
        "the sweep must scope itself to container packages"
    );
}

/// Deleting a package version is the whole grant, and it is the narrowest one
/// that does it. In particular this workflow must not be able to move a ref: a
/// sweep with `contents: write` could rewrite the repository it is pruning
/// images for.
#[test]
fn the_sweep_holds_only_the_packages_grant() {
    let workflow = workflow();
    let permissions = &workflow["jobs"]["sweep"]["permissions"];

    assert_eq!(
        permissions["packages"].as_str(),
        Some("write"),
        "the sweep needs `packages: write` to delete a version, and GITHUB_TOKEN is the whole \
         credential — no PAT to rotate"
    );
    assert_ne!(
        permissions["contents"].as_str(),
        Some("write"),
        "the sweep must not be able to write repository contents: it prunes a registry, and \
         nothing in this repository's automation may move a ref"
    );
    assert!(
        !source().contains("google-github-actions/auth"),
        "the sweep reaches no cloud provider — GHCR is the only registry, and a surviving \
         credential exchange is reach it does not need"
    );
}

/// An unattended destructive job that says nothing is indistinguishable from one
/// that never ran. A silent nightly failure went unnoticed for four consecutive
/// nights once (`docs/gitops.md` → "What detects a broken pipeline"), and this
/// job's failure mode is quieter still: nothing goes red, images just stop being
/// pruned, or worse, are pruned wrongly.
#[test]
fn the_sweep_reports_to_navigator() {
    let source = source();

    assert!(
        source.contains("SLACK_WEBHOOK_URL"),
        "the sweep must post its result to #navigator through the prod ops webhook"
    );
    assert!(
        source.contains("if: failure()"),
        "the sweep must page #navigator when it fails — on a clock, nobody is reading the run"
    );
}

/// Identifiers and counts, never content. The rule binds this surface as hard as
/// it binds the release reports: a sweep summary names images and totals.
#[test]
fn the_sweep_summary_carries_no_client_bearing_field() {
    let source = source();

    for forbidden in ["persons", "matters", "projects/", "@neonlaw.com"] {
        assert!(
            !source.contains(forbidden),
            "the sweep summary must carry identifiers and counts only; found `{forbidden}`"
        );
    }
}
