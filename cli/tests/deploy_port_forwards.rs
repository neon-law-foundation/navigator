//! Guard the release gate's host→cluster port-forwards, and the diagnostics
//! that explain one when it dies.
//!
//! The browser and accessibility suites run on the runner but read the store
//! and the object store *inside* KIND, so four browser fixtures and
//! `dev grant-lawyer` reach `SurrealDB` through `kubectl port-forward`, and the
//! download-all-documents fixture reaches Garage the same way. Those forwards
//! have to stay up for the whole suite — around half an hour.
//!
//! `kubectl port-forward` does not retry. When its stream to the pod breaks it
//! prints the error and exits, and nothing notices: the port simply stops
//! answering. Deploy run 32082116362 died that way — eleven browser tests
//! passed, then `admin_edits_matter_participation_from_the_project_workbench`
//! got `Connection refused` on `ws://127.0.0.1:18000` on all three nextest
//! retries, sixteen minutes into a suite whose port-forward step had reported
//! itself healthy. A one-shot forward turns any momentary stream drop into a
//! permanently dead port and a confusing panic in whichever fixture happens to
//! touch the store next.
//!
//! So both properties are asserted here rather than left to review:
//!
//! - a forward that is expected to outlive its own step respawns when it exits;
//! - every `/tmp` log the diagnostics artifact collects is a file some step in
//!   that job actually writes.
//!
//! The second is what kept run 32082116362 unexplained. The upload listed
//! `/tmp/neon-browser-port-forward.log`, which no step has ever written, while
//! the two real logs went uncollected — and `if-no-files-found: ignore` meant
//! the artifact uploaded quietly without them.

use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

/// The job that stands the cluster up and runs the browser suite against it.
const INTEGRATION: &str = "integration";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root is cli/'s parent")
        .to_path_buf()
}

fn deploy_workflow() -> Value {
    let path = repo_root().join(".github/workflows/deploy.yml");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_yaml::from_str(&raw).unwrap_or_else(|error| panic!("deploy.yml parses as YAML: {error}"))
}

fn steps(workflow: &Value) -> &Vec<Value> {
    workflow["jobs"][INTEGRATION]["steps"]
        .as_sequence()
        .unwrap_or_else(|| panic!("job `{INTEGRATION}` has a steps list"))
}

fn name(step: &Value) -> &str {
    step.get("name").and_then(Value::as_str).unwrap_or_default()
}

fn run(step: &Value) -> &str {
    step.get("run").and_then(Value::as_str).unwrap_or_default()
}

/// A step that opens a forward meant to outlive it: it backgrounds a
/// `kubectl port-forward` and later steps depend on the port answering.
fn opens_a_background_port_forward(step: &Value) -> bool {
    let script = run(step);
    script.contains("port-forward") && script.contains("kubectl") && script.contains('&')
}

#[test]
fn every_background_port_forward_respawns_when_its_stream_drops() {
    let workflow = deploy_workflow();
    let mut guarded = 0;

    for step in steps(&workflow) {
        if !opens_a_background_port_forward(step) {
            continue;
        }
        guarded += 1;
        let label = name(step);
        let script = run(step);
        assert!(
            script.contains("while true"),
            "`{label}` backgrounds a one-shot `kubectl port-forward`. kubectl exits when its \
             stream to the pod breaks, so the port stops answering for the rest of the job and \
             the next fixture to touch it fails with a bare `Connection refused`. Supervise it: \
             re-run kubectl in a `while true` loop so a dropped stream costs a reconnect, not \
             the run."
        );
    }

    assert!(
        guarded >= 2,
        "expected the SurrealDB and Garage forwards to be guarded here, found {guarded} — \
         if a forward was renamed or removed, follow it rather than letting the guard go quiet"
    );
}

#[test]
fn every_uploaded_diagnostic_log_is_a_file_some_step_writes() {
    let workflow = deploy_workflow();
    let steps = steps(&workflow);

    let upload = steps
        .iter()
        .find(|step| {
            step.get("with")
                .and_then(|with| with.get("name"))
                .and_then(Value::as_str)
                == Some("e2e-diagnostics")
        })
        .expect("the integration job uploads an `e2e-diagnostics` artifact");

    let paths = upload["with"]["path"]
        .as_str()
        .expect("the artifact lists its paths");

    // Everything the runner writes for diagnosis lands in /tmp; the rest of
    // the list is repository-relative build output.
    let collected: Vec<&str> = paths
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("/tmp/"))
        .collect();

    assert!(
        !collected.is_empty(),
        "the diagnostics artifact collects no runner-side logs at all"
    );

    let scripts: String = steps.iter().map(run).collect::<Vec<_>>().join("\n");

    for path in collected {
        // A trailing slash names a directory of screenshots, written by the
        // test binary rather than by a step's shell.
        if path.ends_with('/') {
            continue;
        }
        assert!(
            scripts.contains(path),
            "the diagnostics artifact collects `{path}`, but no step in the `{INTEGRATION}` job \
             writes it. An artifact naming a file nothing produces uploads quietly — \
             `if-no-files-found: ignore` — and the failure it was meant to explain stays \
             unexplained."
        );
    }
}
