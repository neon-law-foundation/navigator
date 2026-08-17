//! Live four-state proof for the guarded local staging reset boundary.
//!
//! This test is deliberately ignored: it destroys and recreates the isolated
//! KIND environment selected by `.devx/env`. Run it only after the repository
//! lifecycle has prepared that environment:
//!
//! ```text
//! cargo run -p cli -- dev worktree-env up --path "$PWD"
//! set -a; source .devx/env; set +a
//! cargo test -p cli --test staging_reset_proof -- --ignored
//! ```
//!
//! It proves the four kinds of disposable state that must not cross a reset:
//! the store, the configured object-store lane, a Restate journal, and an
//! in-cluster Git repository. The test reports IDs and row counts only.

use std::{path::Path, process::Command};

use uuid::Uuid;

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a workspace parent")
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set from .devx/env"))
}

fn kind_context() -> String {
    format!("kind-{}", required_env("NAVIGATOR_KIND_CLUSTER"))
}

/// The configured application namespace: the reset boundary `staging reset`
/// deletes and recreates. A worktree tier can select a non-default namespace,
/// so every disposable-state fixture and its removal check must target
/// `NAVIGATOR_K8S_NAMESPACE`. Hardcoding the shared `navigator` default would
/// place the Git pod outside the namespace the reset actually recreates, so
/// the proof would no longer prove removal on such a tier.
fn app_namespace() -> String {
    required_env("NAVIGATOR_K8S_NAMESPACE")
}

fn kubectl(namespace: Option<&str>, args: &[&str]) -> String {
    let mut command = Command::new("kubectl");
    command.arg("--context").arg(kind_context());
    if let Some(namespace) = namespace {
        command.arg("--namespace").arg(namespace);
    }
    command.args(args);
    let output = command.output().expect("run kubectl");
    assert!(
        output.status.success(),
        "kubectl {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).expect("kubectl output is UTF-8")
}

fn kubectl_fails(namespace: Option<&str>, args: &[&str]) {
    let mut command = Command::new("kubectl");
    command.arg("--context").arg(kind_context());
    if let Some(namespace) = namespace {
        command.arg("--namespace").arg(namespace);
    }
    command.args(args);
    let output = command.output().expect("run kubectl");
    assert!(
        !output.status.success(),
        "kubectl {:?} unexpectedly succeeded: {}",
        args,
        String::from_utf8_lossy(&output.stdout).trim()
    );
}

fn navigator(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_navigator"))
        .current_dir(workspace())
        .args(args)
        .output()
        .expect("run navigator");
    assert!(
        output.status.success(),
        "navigator {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).expect("navigator output is UTF-8")
}

fn environment_id() -> String {
    let output = navigator(&["dev", "staging", "status"]);
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("staging: environment-id="))
        .map(str::to_owned)
        .filter(|id| !id.is_empty())
        .expect("staging status must report an environment ID")
}

fn namespace_uid(namespace: &str) -> String {
    kubectl(
        None,
        &[
            "get",
            "namespace",
            namespace,
            "-o",
            "jsonpath={.metadata.uid}",
        ],
    )
    .trim()
    .to_owned()
}

async fn baseline_counts(surreal: &store::surreal::SurrealDb) -> Vec<usize> {
    vec![
        store::persons::list_directory(surreal, "", "", &[])
            .await
            .expect("count persons")
            .len(),
        store::entities::all(surreal)
            .await
            .expect("count entities")
            .len(),
        store::projects::all(surreal)
            .await
            .expect("count projects")
            .len(),
        store::questions::list_all(surreal)
            .await
            .expect("count questions")
            .len(),
        store::templates::list_current(surreal)
            .await
            .expect("count templates")
            .len(),
    ]
}

fn create_disposable_git_commit(proof_id: Uuid) -> (String, String) {
    let namespace = app_namespace();
    let git_pod = format!("reset-proof-git-{proof_id}");
    kubectl(
        Some(&namespace),
        &[
            "run",
            &git_pod,
            // Any git-bearing image works: this pod only has to create a
            // throwaway in-cluster repo the reset must not preserve. Stock
            // Alpine keeps the fixture independent of the workspace's own
            // published images (ENG-142 retired the git-bearing one).
            "--image=alpine:3",
            "--restart=Never",
            "--command",
            "--",
            "sh",
            "-ceu",
            "apk add --no-cache git >/dev/null; \
             git init /tmp/repo; cd /tmp/repo; git config user.email reset-proof@example.com; \
             git config user.name 'Reset proof'; echo reset-proof > state; git add state; \
             git commit -m reset-proof; sleep 3600",
        ],
    );
    kubectl(
        Some(&namespace),
        &[
            "wait",
            "--for=condition=Ready",
            &format!("pod/{git_pod}"),
            "--timeout=120s",
        ],
    );
    let git_commit = kubectl(
        Some(&namespace),
        &[
            "exec",
            &git_pod,
            "--",
            "git",
            "-C",
            "/tmp/repo",
            "rev-parse",
            "HEAD",
        ],
    );
    assert_eq!(
        git_commit.trim().len(),
        40,
        "Git commit must exist before reset"
    );
    (git_pod, git_commit)
}

struct ResetProof {
    before_id: String,
    before_restate_uid: String,
    baseline: Vec<usize>,
    person_id: String,
    object_key: String,
    git_pod: String,
    git_commit: String,
}

async fn establish_disposable_state() -> ResetProof {
    let surreal = store::surreal::connect_from_env()
        .await
        .expect("connect to the host web store");
    store::schema::apply(&surreal)
        .await
        .expect("apply the development schema");
    let storage = cloud::from_env().await.expect("configured storage path");
    // This is the same environment-aware seed operation web runs at boot.
    // Calling it directly keeps the proof on this worktree's schema rather
    // than coupling it to whatever release image happened to be published.
    store::seed::seed_environment(
        &surreal,
        &storage,
        store::DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .expect("seed canonical plus development baseline");
    navigator(&["dev", "staging", "up"]);
    let before_id = environment_id();
    let before_restate_uid = namespace_uid("restate");
    let baseline = baseline_counts(&surreal).await;
    assert!(
        baseline.iter().all(|count| *count > 0),
        "the deployed app must restore canonical plus development baseline rows: {baseline:?}"
    );

    let proof_id = Uuid::now_v7();
    let person_id = store::persons::create(
        &surreal,
        &store::persons::NewPerson::new(
            "Reset proof",
            format!("reset-proof-{proof_id}@example.com"),
        ),
    )
    .await
    .expect("insert a non-baseline store row")
    .id
    .to_string();

    let object_key = format!("closure-audit/{proof_id}");
    storage
        .put(&object_key, b"reset-proof", "text/plain")
        .await
        .expect("write object through configured storage");
    assert!(
        storage.get(&object_key).await.is_ok(),
        "object write must round-trip"
    );

    let invocation = workflows::start_workflow(
        &required_env("RESTATE_BROKER_URL"),
        std::env::var("RESTATE_AUTH_TOKEN").ok().as_deref(),
        "Heartbeat",
        &format!("closure-audit-{proof_id}"),
        "run",
        &serde_json::json!({}),
        true,
    )
    .await
    .expect("create a real Restate invocation");
    assert!(
        invocation.contains("invocation"),
        "the ingress must acknowledge a journaled invocation"
    );

    let (git_pod, git_commit) = create_disposable_git_commit(proof_id);

    ResetProof {
        before_id,
        before_restate_uid,
        baseline,
        person_id,
        object_key,
        git_pod,
        git_commit,
    }
}

async fn assert_reset_removed_disposable_state(proof: ResetProof) {
    // The destructive path is the production-shaped CLI path, never an ad
    // hoc namespace deletion. It verifies the labels/context before delete.
    navigator(&["dev", "staging", "reset"]);
    // `staging reset` restores the same task-owned dependency tier and
    // rewrites `.devx/env`.
    dotenvy::from_path_override(workspace().join(".devx/env")).expect("refresh reset env");

    assert_ne!(
        proof.before_id,
        environment_id(),
        "reset must create a new environment ID"
    );
    assert_ne!(
        proof.before_restate_uid,
        namespace_uid("restate"),
        "the Restate journal namespace must be recreated"
    );

    let after_surreal = store::surreal::connect_from_env()
        .await
        .expect("reconnect to the store after reset");
    store::schema::apply(&after_surreal)
        .await
        .expect("apply the recreated development schema");
    let storage = cloud::from_env().await.expect("refreshed storage path");
    store::seed::seed_environment(
        &after_surreal,
        &storage,
        store::DeploymentEnvironment::Dev,
        store::seed::BrandSeed::Neon,
    )
    .await
    .expect("restore canonical plus development baseline");
    assert_eq!(
        baseline_counts(&after_surreal).await,
        proof.baseline,
        "reset must restore exactly the canonical plus development baseline"
    );
    let survivor = store::persons::find_by_id(
        &after_surreal,
        proof.person_id.parse().expect("the proof id is a UUID"),
    )
    .await
    .expect("query the reset proof row");
    assert!(
        survivor.is_none(),
        "reset must remove the inserted store row"
    );

    assert!(
        storage.get(&proof.object_key).await.is_err(),
        "reset must remove the inserted storage object"
    );
    let namespace = app_namespace();
    kubectl_fails(Some(&namespace), &["get", "pod", &proof.git_pod]);

    let worker_manifest = kubectl(
        Some(&namespace),
        &[
            "get",
            "restatedeployment",
            "workflows-service",
            "-o",
            "json",
        ],
    );
    assert!(
        worker_manifest.contains("NAVIGATOR_SURREAL_ENDPOINT"),
        "the worker must take its store coordinates from the environment-owned config"
    );
    assert!(
        required_env("NAVIGATOR_SURREAL_ENDPOINT").starts_with("ws"),
        "the host web must target the same in-cluster store"
    );
    assert_eq!(
        proof.git_commit.trim().len(),
        40,
        "the reset proof recorded a Git commit"
    );
}

/// Requires a freshly prepared task-owned KIND environment and performs a
/// destructive reset of that environment.
#[tokio::test]
#[ignore = "requires the explicitly prepared local KIND harness"]
async fn guarded_reset_removes_every_disposable_state_and_restores_the_dev_baseline() {
    assert_eq!(
        required_env("NAVIGATOR_ENVIRONMENT"),
        "dev",
        "the staging lifecycle must use the single dev application profile"
    );
    assert_eq!(
        std::env::var("NAV_REQUIRE_HARNESS").as_deref(),
        Ok("1"),
        "set NAV_REQUIRE_HARNESS=1 before this destructive proof"
    );
    assert_reset_removed_disposable_state(establish_disposable_state().await).await;
}
