//! `devx e2e` + `devx grant-staff` — native Rust ports of the former
//! `scripts/e2e.sh` and `scripts/ci-grant-staff.sh`.
//!
//! Both are smoke-test plumbing for the deployed KIND stack, run from
//! CI (`.github/workflows/ci.yml`) and locally via
//! `devx`. They were the last shell scripts in the workspace; porting
//! them lets `scripts/` go away entirely (the workspace is Rust-only
//! per `CLAUDE.md`).
//!
//! Behavior is faithful to the scripts they replace: `run_e2e` waits
//! for every rollout, hits `/health` through the ingress, and confirms
//! the seed data landed; `grant_staff` pre-seeds the Staff demo user so
//! the browser e2e can reach `/admin`. The one deliberate change:
//! `grant_staff` writes the singular `persons.role` column (the schema
//! collapsed `roles` JSON → `role` in migration `m20260619_…`), fixing
//! drift the old `ci-grant-staff.sh` carried.
//!
//! The OPA policy itself is *not* probed here. Asserting a live,
//! port-forwarded OPA from this step proved chronically flaky in CI: a
//! `kubectl port-forward` that accepts the local connection then stalls
//! on its first dial to a freshly-Ready pod hung the job until the
//! 60-minute timeout — twice, surviving two rounds of "bound the probe."
//! The policy is a pure function, so its decisions are pinned by
//! `opa test` against the real Rego instead (see
//! `k8s/base/opa/navigator_test.rego`, run from `ci.yml`), which is
//! faster, deterministic, and cluster-free.
//!
//! ## Testing
//!
//! The orchestration shells out to `kubectl`/`curl` against a live
//! cluster, so it isn't unit-tested. The decision logic that *can*
//! drift — the seed thresholds, the counts parser, and the grant SQL —
//! is pure and covered by the `tests` module below.

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::{require_tools, run, use_kind_context, wait_for_condition, wait_rollout, KindConfig};

/// Minimum seed-row counts the deployed stack must show. Matches the
/// canonical seed in `store/seeds/`; a smaller count means seeding
/// silently failed.
const MIN_QUESTIONS: i64 = 8;
const MIN_JURISDICTIONS: i64 = 6;

/// Parse the `q|j` line `psql -At` prints for the seed-count query.
fn parse_seed_counts(out: &str) -> Result<(i64, i64)> {
    let line = out.trim();
    let (q, j) = line
        .split_once('|')
        .with_context(|| format!("expected `q|j` from psql, got {line:?}"))?;
    let q = q
        .trim()
        .parse::<i64>()
        .with_context(|| format!("parse question count {q:?}"))?;
    let j = j
        .trim()
        .parse::<i64>()
        .with_context(|| format!("parse jurisdiction count {j:?}"))?;
    Ok((q, j))
}

/// Whether the seed counts clear the minimums.
fn seed_counts_ok(questions: i64, jurisdictions: i64) -> bool {
    questions >= MIN_QUESTIONS && jurisdictions >= MIN_JURISDICTIONS
}

/// The SQL `grant_staff` runs to pre-seed the Staff demo user. Writes
/// the singular `role` column with `ON CONFLICT (email)` upsert; the
/// explicit id/timestamps are required because raw SQL bypasses the
/// `SeaORM` `ActiveModelBehavior` that fills them for app-side writes.
fn grant_staff_sql() -> &'static str {
    "INSERT INTO persons (id, name, email, oidc_subject, role, inserted_at, updated_at) \
     VALUES (gen_random_uuid(), 'Staff', 'staff@neonlaw.com', NULL, 'staff', now(), now()) \
     ON CONFLICT (email) DO UPDATE SET role = 'staff', updated_at = now();"
}

// ---------- orchestration (shell-out; not unit-tested) ----------

/// `devx e2e`: the full deployed-stack smoke check.
pub fn run_e2e(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kubectl", "curl"])?;
    use_kind_context(cfg)?;

    eprintln!("=== waiting for navigator-web rollout ===");
    wait_rollout("deployment", "navigator-web", cfg)?;

    eprintln!("=== checking dependent services ===");
    for dep in ["postgres", "fake-gcs-server", "keycloak", "opa"] {
        eprintln!("    waiting for deployment/{dep} rollout");
        wait_rollout("deployment", dep, cfg)?;
    }
    wait_for_restate(cfg)?;

    eprintln!("=== hitting the ingress ===");
    check_health()?;

    eprintln!("=== confirming seed data populated ===");
    check_seed(cfg)?;

    eprintln!("=== all checks passed ===");
    Ok(())
}

/// `devx grant-staff`: pre-seed the Staff demo user with the `staff`
/// role so the browser e2e's admin-gated walk can run.
pub fn grant_staff(cfg: &KindConfig) -> Result<()> {
    require_tools(&["kubectl"])?;
    use_kind_context(cfg)?;
    eprintln!("=== granting staff the staff role ===");
    run(&mut psql(cfg, grant_staff_sql()))?;
    eprintln!("=== verifying ===");
    run(&mut psql(
        cfg,
        "SELECT email, role FROM persons WHERE email = 'staff@neonlaw.com';",
    ))
}

/// Restate readiness depends on the broker. Restate Cloud has no
/// in-cluster `StatefulSet` — probe the tenant via the CLI; otherwise
/// wait on the Operator's `restate` `StatefulSet`. Either way the
/// worker Deployment must roll out.
fn wait_for_restate(cfg: &KindConfig) -> Result<()> {
    let broker = std::env::var("RESTATE_BROKER_URL").unwrap_or_default();
    if broker.contains("restate.cloud") {
        eprintln!("    Restate Cloud broker detected — probing tenant via CLI");
        require_tools(&["restate"])?;
        let out = Command::new("restate")
            .args(["-y", "deployment", "list"])
            .output()
            .context("run `restate -y deployment list`")?;
        let listing = String::from_utf8_lossy(&out.stdout);
        if !listing.contains("workflows-service") {
            eprintln!("{listing}");
            bail!("workflows-service not registered with Restate Cloud tenant");
        }
    } else {
        // The Operator places the cluster in its own `restate` namespace
        // (not cfg.namespace) and names the StatefulSet from the CR spec,
        // not literally "restate" — so wait on the RestateCluster CR's
        // `Ready` condition, the same contract `deploy`'s
        // wait_for_dep_rollouts uses.
        let (ns, resource, condition) = restate_ready_target();
        eprintln!("    waiting for {resource} {condition} in namespace {ns}");
        wait_for_condition(ns, resource, condition)?;
    }
    // workflows-service is a RestateDeployment CR (Operator-managed), not a
    // plain Deployment — `deployment/workflows-service` returns NotFound. It
    // lives in cfg.namespace (unlike the cluster). Wait on the CR's `Ready`
    // condition, the same contract `deploy`'s wait_for_dep_rollouts uses.
    eprintln!(
        "    waiting for {WORKFLOWS_SERVICE_READY_RESOURCE} Ready in namespace {}",
        cfg.namespace
    );
    wait_for_condition(&cfg.namespace, WORKFLOWS_SERVICE_READY_RESOURCE, "Ready")
}

/// The Operator-managed resource whose `Ready` condition gates
/// workflows-service readiness. It is a `RestateDeployment` CR, not a plain
/// `Deployment` — querying `deployment/workflows-service` returns `NotFound`.
const WORKFLOWS_SERVICE_READY_RESOURCE: &str = "restatedeployment/workflows-service";

/// The (namespace, resource, condition) the in-cluster Restate readiness
/// wait targets. The Restate Operator reconciles the `RestateCluster` CR
/// into a `StatefulSet` in a namespace named after the cluster (`restate`),
/// *not* in `cfg.namespace` and *not* under a guessable `StatefulSet` name —
/// so the readiness gate is the CR's own `Ready` condition. Pulled out so
/// the namespace/resource choice is unit-testable (see tests below).
fn restate_ready_target() -> (&'static str, &'static str, &'static str) {
    ("restate", "restatecluster/restate", "Ready")
}

/// Hit `/health` through the KIND ingress and require HTTP 200.
///
/// Two guards make this loud-but-bounded instead of an indefinite hang.
/// `--max-time` caps each individual request, so a wedged ingress that
/// accepts the connection but never answers can't block the whole `e2e`
/// step (that un-capped curl was a load-bearing reason a stuck deploy ran
/// for hours). The retry loop tolerates the few seconds the ingress can
/// lag behind a freshly-Ready pod, and every attempt logs its status so a
/// failure says *what* the ingress returned, not just "not 200".
fn check_health() -> Result<()> {
    let host = std::env::var("INGRESS_HOST").unwrap_or_else(|_| "localhost:8080".to_string());
    let url = format!("http://{host}/health");
    let deadline = Instant::now() + Duration::from_mins(1);
    let mut attempt = 0;
    loop {
        attempt += 1;
        let out = Command::new("curl")
            .args([
                "-sS",
                "--max-time",
                "10",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
            ])
            .arg("--resolve")
            .arg("localhost:8080:127.0.0.1")
            .arg(&url)
            .output()
            .context("curl /health")?;
        let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if status == "200" {
            eprintln!("    health OK ({status}) after {attempt} attempt(s)");
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "expected HTTP 200 from {url} within 60s; last status {status:?} after {attempt} attempt(s)"
            );
        }
        eprintln!("    /health not ready (status {status:?}); retrying in 2s [attempt {attempt}]");
        sleep(Duration::from_secs(2));
    }
}

/// Confirm the seed data populated past the minimum row counts.
fn check_seed(cfg: &KindConfig) -> Result<()> {
    let out = psql_capture(
        cfg,
        "select (select count(*) from questions), (select count(*) from jurisdictions);",
    )?;
    let (q, j) = parse_seed_counts(&out)?;
    if !seed_counts_ok(q, j) {
        bail!("expected at least {MIN_QUESTIONS} questions and {MIN_JURISDICTIONS} jurisdictions; got q={q} j={j}");
    }
    eprintln!("seed OK (q={q} j={j})");
    Ok(())
}

/// A `kubectl exec deployment/postgres -- psql …` command (exit-code
/// checked). `-c <sql>` runs one statement.
fn psql(cfg: &KindConfig, sql: &str) -> Command {
    let mut cmd = Command::new("kubectl");
    cmd.arg("--namespace")
        .arg(&cfg.namespace)
        .arg("exec")
        .arg("deployment/postgres")
        .arg("--")
        .arg("psql")
        .arg("-U")
        .arg("navigator")
        .arg("-d")
        .arg("navigator")
        .arg("-c")
        .arg(sql);
    cmd
}

/// Run a `psql -At -c <sql>` and capture its (tuples-only) stdout.
fn psql_capture(cfg: &KindConfig, sql: &str) -> Result<String> {
    let out = Command::new("kubectl")
        .arg("--namespace")
        .arg(&cfg.namespace)
        .arg("exec")
        .arg("deployment/postgres")
        .arg("--")
        .arg("psql")
        .arg("-At")
        .arg("-U")
        .arg("navigator")
        .arg("-d")
        .arg("navigator")
        .arg("-c")
        .arg(sql)
        .output()
        .context("kubectl exec psql")?;
    if !out.status.success() {
        bail!(
            "psql query failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seed_counts_reads_the_psql_line() {
        assert_eq!(parse_seed_counts("8|6").unwrap(), (8, 6));
        assert_eq!(parse_seed_counts("  12|9  \n").unwrap(), (12, 9));
        assert!(parse_seed_counts("nope").is_err());
        assert!(parse_seed_counts("8|notanumber").is_err());
    }

    #[test]
    fn seed_counts_ok_enforces_the_minimums() {
        assert!(seed_counts_ok(8, 6));
        assert!(seed_counts_ok(20, 10));
        assert!(!seed_counts_ok(7, 6));
        assert!(!seed_counts_ok(8, 5));
    }

    #[test]
    fn grant_staff_sql_targets_the_singular_role_column() {
        let sql = grant_staff_sql();
        // The fix for the schema drift: write `role`, not `roles`.
        assert!(sql.contains("role"));
        assert!(!sql.contains("roles"));
        assert!(!sql.contains("jsonb"));
        assert!(sql.contains("'staff'"));
        assert!(sql.contains("ON CONFLICT (email)"));
        assert!(sql.contains("staff@neonlaw.com"));
    }

    #[test]
    fn restate_readiness_waits_in_the_operator_namespace() {
        // Regression guard for the smoke-check failure where wait_for_restate
        // queried `statefulset/restate` in cfg.namespace (`navigator`) — the
        // Operator places it in the `restate` namespace, so the wait must
        // target that namespace and the RestateCluster CR, not a StatefulSet.
        let (ns, resource, condition) = restate_ready_target();
        assert_eq!(
            ns, "restate",
            "Restate Operator reconciles the cluster into its own `restate` namespace, not cfg.namespace"
        );
        assert!(
            resource.starts_with("restatecluster/"),
            "wait on the RestateCluster CR's Ready condition, not a guessed StatefulSet name: {resource}"
        );
        assert_eq!(condition, "Ready");
    }

    #[test]
    fn workflows_service_readiness_targets_the_restatedeployment_cr() {
        // Regression guard: workflows-service is an Operator-managed
        // RestateDeployment CR, not a plain Deployment — querying
        // `deployment/workflows-service` returns NotFound.
        assert!(
            WORKFLOWS_SERVICE_READY_RESOURCE.starts_with("restatedeployment/"),
            "wait on the RestateDeployment CR, not a plain Deployment: {WORKFLOWS_SERVICE_READY_RESOURCE}"
        );
    }
}
