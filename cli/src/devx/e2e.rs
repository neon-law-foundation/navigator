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
//! for every rollout, hits `/health` through the ingress, asserts a
//! fixed table of OPA policy decisions, and confirms the seed data
//! landed; `grant_staff` pre-seeds the Staff demo user so the browser
//! e2e can reach `/admin`. The one deliberate change: `grant_staff`
//! writes the singular `persons.role` column (the schema collapsed
//! `roles` JSON → `role` in migration `m20260619_…`), fixing drift the
//! old `ci-grant-staff.sh` carried.
//!
//! ## Testing
//!
//! The orchestration shells out to `kubectl`/`curl` against a live
//! cluster, so it isn't unit-tested. The decision logic that *can*
//! drift — the OPA case table, the expected-response shape, the seed
//! thresholds, the counts parser, and the grant SQL — is pure and
//! covered by the `tests` module below.

use std::io::Read;
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::{require_tools, run, use_kind_context, wait_for_condition, wait_rollout, KindConfig};

/// Local host port the OPA port-forward binds for the policy probe.
/// Distinct from the standard 8181 a `devx up` may already forward, so
/// the two don't collide when e2e runs against a live dev loop.
const OPA_PROBE_PORT: u16 = 28181;

/// Minimum seed-row counts the deployed stack must show. Matches the
/// canonical seed in `store/seeds/`; a smaller count means seeding
/// silently failed.
const MIN_QUESTIONS: i64 = 8;
const MIN_JURISDICTIONS: i64 = 6;

/// One OPA authorization decision the smoke test pins. `input` is the
/// JSON body sent to `/v1/data/navigator/authz/allow`; `expected` is
/// the boolean the policy must return. A drift here (a dropped rule, a
/// stale `ConfigMap`) fails the gate before prod.
struct OpaCase {
    desc: &'static str,
    expected: bool,
    input: String,
}

/// The fixed table of policy decisions, one input per allow-rule plus
/// the deny cases. Sessions carry the singular `role` field OPA
/// evaluates against (post `roles[] → role` collapse).
fn opa_cases() -> Vec<OpaCase> {
    let admin = session("a@neonlaw.com", "admin");
    let staff = session("s@neonlaw.com", "staff");
    let client = session("c@example.com", "client");
    vec![
        OpaCase {
            desc: "admin → /portal",
            expected: true,
            input: req(&["portal"], "GET", &admin),
        },
        OpaCase {
            desc: "client → /portal",
            expected: true,
            input: req(&["portal"], "GET", &client),
        },
        OpaCase {
            desc: "anonymous → /portal",
            expected: false,
            input: req_anon(&["portal"], "GET"),
        },
        OpaCase {
            desc: "client → /portal/projects",
            expected: true,
            input: req(&["portal", "projects"], "GET", &client),
        },
        OpaCase {
            desc: "staff → /portal/admin/people",
            expected: true,
            input: req(&["portal", "admin", "people"], "GET", &staff),
        },
        OpaCase {
            desc: "client → /portal/admin/people",
            expected: false,
            input: req(&["portal", "admin", "people"], "GET", &client),
        },
        OpaCase {
            desc: "staff → /mcp",
            expected: true,
            input: req(&["mcp"], "POST", &staff),
        },
        OpaCase {
            desc: "client → /mcp",
            expected: false,
            input: req(&["mcp"], "POST", &client),
        },
        OpaCase {
            desc: "staff → /api/aida/rpc",
            expected: true,
            input: req(&["api", "aida", "rpc"], "POST", &staff),
        },
        OpaCase {
            desc: "client → /api/aida/rpc",
            expected: false,
            input: req(&["api", "aida", "rpc"], "POST", &client),
        },
        OpaCase {
            desc: "anonymous → /openapi.json",
            expected: true,
            input: req_anon(&["openapi.json"], "GET"),
        },
    ]
}

/// A session object literal with the given email + role.
fn session(email: &str, role: &str) -> String {
    format!(r#"{{"sub":"x","email":"{email}","exp":9999999999,"role":"{role}","csrf_token":""}}"#)
}

/// An OPA query body for `path`/`method` carrying `session`.
fn req(path: &[&str], method: &str, session: &str) -> String {
    format!(
        r#"{{"input":{{"path":{},"method":"{method}","session":{session}}}}}"#,
        json_string_array(path)
    )
}

/// An OPA query body with a null (anonymous) session.
fn req_anon(path: &[&str], method: &str) -> String {
    format!(
        r#"{{"input":{{"path":{},"method":"{method}","session":null}}}}"#,
        json_string_array(path)
    )
}

/// Render `["a","b"]` from a path slice.
fn json_string_array(parts: &[&str]) -> String {
    let inner = parts
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}

/// The exact response body OPA returns for a boolean decision.
fn opa_expected_body(expected: bool) -> String {
    format!("{{\"result\":{expected}}}")
}

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
        wait_rollout("deployment", dep, cfg)?;
    }
    wait_for_restate(cfg)?;

    eprintln!("=== hitting the ingress ===");
    check_health()?;

    eprintln!("=== probing OPA policy decisions ===");
    check_opa(cfg)?;

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
        wait_for_condition(ns, resource, condition)?;
    }
    wait_rollout("deployment", "workflows-service", cfg)
}

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
fn check_health() -> Result<()> {
    let host = std::env::var("INGRESS_HOST").unwrap_or_else(|_| "localhost:8080".to_string());
    let out = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}"])
        .arg("--resolve")
        .arg("localhost:8080:127.0.0.1")
        .arg(format!("http://{host}/health"))
        .output()
        .context("curl /health")?;
    let status = String::from_utf8_lossy(&out.stdout);
    let status = status.trim();
    if status != "200" {
        bail!("expected HTTP 200 from /health, got {status}");
    }
    eprintln!("health OK ({status})");
    Ok(())
}

/// Port-forward OPA, wait for it to accept connections, then assert
/// every decision in [`opa_cases`].
fn check_opa(cfg: &KindConfig) -> Result<()> {
    let _pf = PortForward::spawn(cfg, "svc/opa", OPA_PROBE_PORT, 8181)?;
    wait_for_port(OPA_PROBE_PORT, Duration::from_secs(10))?;
    for case in opa_cases() {
        let out = Command::new("curl")
            .args(["-fsS", "-X", "POST"])
            .arg(format!(
                "http://127.0.0.1:{OPA_PROBE_PORT}/v1/data/navigator/authz/allow"
            ))
            .args(["-H", "content-type: application/json", "--data"])
            .arg(&case.input)
            .output()
            .with_context(|| format!("curl OPA decision: {}", case.desc))?;
        if !out.status.success() {
            bail!(
                "OPA query failed for {}: {}",
                case.desc,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let got = String::from_utf8_lossy(&out.stdout);
        let got = got.trim();
        let want = opa_expected_body(case.expected);
        if got != want {
            bail!(
                "OPA decision drift: {} expected {want}, got {got}",
                case.desc
            );
        }
        eprintln!("    {} → {}", case.desc, case.expected);
    }
    eprintln!("OPA policy OK");
    Ok(())
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

/// A `kubectl port-forward` child that is killed when dropped.
struct PortForward {
    child: Child,
}

impl PortForward {
    fn spawn(cfg: &KindConfig, target: &str, host_port: u16, svc_port: u16) -> Result<Self> {
        let child = Command::new("kubectl")
            .arg("--namespace")
            .arg(&cfg.namespace)
            .arg("port-forward")
            .arg(target)
            .arg(format!("{host_port}:{svc_port}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn kubectl port-forward {target}"))?;
        Ok(Self { child })
    }
}

impl Drop for PortForward {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Block until `127.0.0.1:port` accepts a connection, or time out.
fn wait_for_port(port: u16, timeout: Duration) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(mut stream) = TcpStream::connect_timeout(
            &addr.parse().expect("valid loopback addr"),
            Duration::from_millis(500),
        ) {
            // Drain nothing — just confirm the listener is live.
            let _ = stream.read(&mut [0u8; 0]);
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for OPA port-forward on {addr}");
        }
        sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opa_cases_cover_every_documented_decision() {
        let cases = opa_cases();
        // One row per assertion the old e2e.sh made.
        assert_eq!(cases.len(), 11);
        // The deny cases are present and false; the allow cases true.
        let denied: Vec<_> = cases
            .iter()
            .filter(|c| !c.expected)
            .map(|c| c.desc)
            .collect();
        assert!(denied.contains(&"anonymous → /portal"));
        assert!(denied.contains(&"client → /mcp"));
        assert!(denied.contains(&"client → /api/aida/rpc"));
        assert!(denied.contains(&"client → /portal/admin/people"));
    }

    #[test]
    fn opa_case_inputs_are_well_formed_json() {
        for case in opa_cases() {
            let parsed: serde_json::Value = serde_json::from_str(&case.input)
                .unwrap_or_else(|e| panic!("case {:?} input is not JSON: {e}", case.desc));
            // Every body has an `input.path` array and a `method`.
            assert!(parsed["input"]["path"].is_array(), "{}", case.desc);
            assert!(parsed["input"]["method"].is_string(), "{}", case.desc);
        }
    }

    #[test]
    fn opa_sessions_use_the_singular_role_field() {
        // Guards against regressing to the collapsed `roles[]` shape.
        let staff = session("s@neonlaw.com", "staff");
        let parsed: serde_json::Value = serde_json::from_str(&staff).unwrap();
        assert_eq!(parsed["role"], "staff");
        assert!(parsed.get("roles").is_none());
    }

    #[test]
    fn opa_expected_body_is_compact_json() {
        assert_eq!(opa_expected_body(true), r#"{"result":true}"#);
        assert_eq!(opa_expected_body(false), r#"{"result":false}"#);
    }

    #[test]
    fn json_string_array_renders_a_path() {
        assert_eq!(
            json_string_array(&["portal", "projects"]),
            r#"["portal","projects"]"#
        );
        assert_eq!(json_string_array(&["portal"]), r#"["portal"]"#);
    }

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
}
