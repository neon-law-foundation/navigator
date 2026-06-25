//! Integration test pinning the Neon Law Navigator OPA authorization policy.
//!
//! The policy is a pure function `input -> bool`, so its decisions are
//! best asserted by `opa test` against the real Rego — not by deploying
//! OPA into KIND and curling a port-forward, which proved chronically
//! flaky in CI (a `kubectl port-forward` that accepts the local
//! connection then stalls hung the deploy job until its 60-minute
//! timeout, twice). The KIND e2e (`cli::devx::e2e`) no longer probes
//! OPA; this test is where the allow/deny coverage lives.
//!
//! There is exactly one copy of the policy: the `navigator.rego` body
//! embedded in the `opa-policies` `ConfigMap` (`k8s/base/opa/opa.yaml`),
//! which is what the cluster actually mounts. This test extracts that
//! body verbatim, writes it next to the checked-in
//! `k8s/base/opa/navigator_test.rego`, and runs `opa test` over both —
//! so the tests run against the same Rego the cluster serves and there
//! is nothing to drift.
//!
//! `opa` must be on `PATH`. CI installs it (`.github/workflows/ci.yml`);
//! locally the test skips with a notice if the binary is absent, unless
//! `CI` is set (then a missing `opa` is a hard failure, so the gate can
//! never silently pass by skipping).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<repo>/cli`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a parent (the workspace root)")
        .to_path_buf()
}

/// Pull the `navigator.rego` policy body out of the `opa-policies`
/// `ConfigMap`. `opa.yaml` is a multi-document file (`ConfigMap` +
/// Deployment + Service); we want the `ConfigMap`'s `data` entry.
fn extract_policy_rego(opa_yaml: &str) -> String {
    for doc in serde_yaml::Deserializer::from_str(opa_yaml) {
        let value = serde_yaml::Value::deserialize(doc).expect("opa.yaml is valid YAML");
        if value.get("kind").and_then(serde_yaml::Value::as_str) != Some("ConfigMap") {
            continue;
        }
        if let Some(rego) = value
            .get("data")
            .and_then(|d| d.get("navigator.rego"))
            .and_then(serde_yaml::Value::as_str)
        {
            return rego.to_string();
        }
    }
    panic!("k8s/base/opa/opa.yaml has no ConfigMap with data.\"navigator.rego\"");
}

fn opa_available() -> bool {
    Command::new("opa")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn opa_policy_passes_its_rego_unit_tests() {
    let root = repo_root();
    let opa_yaml = std::fs::read_to_string(root.join("k8s/base/opa/opa.yaml"))
        .expect("read k8s/base/opa/opa.yaml");
    let policy = extract_policy_rego(&opa_yaml);

    if !opa_available() {
        assert!(
            std::env::var_os("CI").is_none(),
            "`opa` is not on PATH but CI is set — the OPA policy gate would silently skip. \
             Install OPA before `cargo test` (see .github/workflows/ci.yml)."
        );
        eprintln!(
            "SKIP opa_policy_passes_its_rego_unit_tests: `opa` not installed. \
             Install it (https://www.openpolicyagent.org/docs/latest/#running-opa) to run policy tests locally."
        );
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir for opa test");
    std::fs::write(dir.path().join("navigator.rego"), &policy).expect("write navigator.rego");
    // The test cases are checked in beside the policy; embed them at
    // compile time so this test fails to build if the file goes missing.
    std::fs::write(
        dir.path().join("navigator_test.rego"),
        include_str!("../../k8s/base/opa/navigator_test.rego"),
    )
    .expect("write navigator_test.rego");

    let out = Command::new("opa")
        .arg("test")
        .arg("--verbose")
        .arg(dir.path())
        .output()
        .expect("run `opa test`");

    assert!(
        out.status.success(),
        "OPA policy unit tests failed.\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
