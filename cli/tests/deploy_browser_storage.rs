//! Guard the host storage lanes the release gate's browser suite writes through.
//!
//! Two fixtures in `server/tests/browser_e2e.rs` publish objects from the
//! runner that the in-cluster `web` then reads back: the download-all-documents
//! test writes blobs through `cloud::from_env()`, and
//! `the_project_page_links_to_the_client_portal_and_it_streams` publishes a
//! portal bundle through `cloud::applications_from_env()`.
//!
//! Those are DIFFERENT LANES, and in Garage that distinction has teeth.
//! `dev garage-bootstrap` mints one key per bucket and grants it that bucket
//! alone (`cli/src/devx/garage.rs`), so the documents key cannot write the
//! applications bucket. `S3StorageConfig::applications_from_lookup` reads
//! `NAVIGATOR_APPLICATIONS_*` and falls back to the generic
//! `NAVIGATOR_STORAGE_*` credentials — a fallback that resolves to the
//! documents key here and would be denied on the applications bucket. So each
//! lane the suite uses has to be wired explicitly, the same way
//! `k8s/base/web/web.yaml` wires it for the pod.
//!
//! Deploy run 32094090106 is why this is asserted rather than reviewed. With
//! the port-forwards supervised the browser suite reached all 26 tests for the
//! first time, and the portal test failed on all three retries with
//! `MissingEnv("NAVIGATOR_APPLICATIONS_BUCKET or NAVIGATOR_STORAGE_BUCKET")`.
//! The lane had never been wired; nothing had ever run far enough to notice.

use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

/// The job that runs the browser suite against the KIND cluster.
const INTEGRATION: &str = "integration";

/// The step whose environment the browser fixtures inherit.
const BROWSER_STEP: &str = "browser + accessibility e2e (nextest, retry-classified)";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root is cli/'s parent")
        .to_path_buf()
}

fn yaml_at(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_yaml::from_str(&raw).unwrap_or_else(|error| panic!("{relative} parses as YAML: {error}"))
}

/// The Kubernetes manifests are multi-document; pick the one document with
/// `metadata.name`, so a guard reads the object it means rather than whichever
/// one happens to come first.
fn manifest_named(relative: &str, object: &str) -> Value {
    use serde::Deserialize;

    let path = repo_root().join(relative);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    let found = serde_yaml::Deserializer::from_str(&raw)
        .filter_map(|document| Value::deserialize(document).ok())
        .find(|document| document["metadata"]["name"].as_str() == Some(object));
    found.unwrap_or_else(|| panic!("{relative} defines an object named `{object}`"))
}

/// Everything the browser step's process can read: its declared `env` keys and
/// the `run` block, which exports the per-lane keys it pulls out of the Garage
/// secret at run time.
fn browser_step_environment() -> (Vec<String>, String) {
    let workflow = yaml_at(".github/workflows/deploy.yml");
    let steps = workflow["jobs"][INTEGRATION]["steps"]
        .as_sequence()
        .unwrap_or_else(|| panic!("job `{INTEGRATION}` has a steps list"));

    let step = steps
        .iter()
        .find(|step| step.get("name").and_then(Value::as_str) == Some(BROWSER_STEP))
        .unwrap_or_else(|| panic!("the `{INTEGRATION}` job runs a `{BROWSER_STEP}` step"));

    let env = step
        .get("env")
        .and_then(Value::as_mapping)
        .map(|mapping| {
            mapping
                .keys()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let run = step
        .get("run")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    (env, run)
}

#[test]
fn the_browser_suite_wires_every_storage_lane_its_fixtures_write_through() {
    let (env, run) = browser_step_environment();
    let declares = |key: &str| env.iter().any(|name| name == key) || run.contains(key);

    // The documents lane rides the generic `NAVIGATOR_STORAGE_*` credentials,
    // which is why it has worked all along; the applications lane has its own
    // key and needs its own three.
    for key in [
        "NAVIGATOR_APPLICATIONS_BUCKET",
        "NAVIGATOR_APPLICATIONS_ACCESS_KEY",
        "NAVIGATOR_APPLICATIONS_SECRET_KEY",
    ] {
        assert!(
            declares(key),
            "the browser step never provides `{key}`, so the portal fixture cannot publish its \
             bundle. Garage grants each key exactly one bucket, so falling back to the generic \
             `NAVIGATOR_STORAGE_*` credentials reaches the applications bucket with the \
             documents key and is denied."
        );
    }
}

#[test]
fn the_browser_suite_names_the_applications_bucket_the_cluster_serves() {
    let overlay = manifest_named("k8s/overlays/kind/garage/garage.yaml", "navigator-garage");
    let bucket = overlay["data"]["applications_bucket"]
        .as_str()
        .expect("the kind Garage ConfigMap names an applications bucket");

    let workflow = yaml_at(".github/workflows/deploy.yml");
    let steps = workflow["jobs"][INTEGRATION]["steps"]
        .as_sequence()
        .expect("the integration job has steps");
    let step = steps
        .iter()
        .find(|step| step.get("name").and_then(Value::as_str) == Some(BROWSER_STEP))
        .expect("the integration job runs the browser step");

    let declared = step["env"]["NAVIGATOR_APPLICATIONS_BUCKET"]
        .as_str()
        .expect("the browser step names the applications bucket");

    assert_eq!(
        declared, bucket,
        "the browser step publishes the portal bundle to `{declared}` while the cluster serves \
         `{bucket}` — the fixture would write a bundle `web` never reads, and the test would \
         fail on a 404 that says nothing about the mismatch"
    );
}
