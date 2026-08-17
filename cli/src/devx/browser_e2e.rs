//! `dev browser-e2e` — reproduce `deploy.yml`'s `integration` browser
//! gate locally with one command.
//!
//! The nightly deploy's `integration` job is the gate that keeps
//! failing on browser/accessibility regressions, but until now there
//! was no single command that ran that same gate on a dev box — so a
//! `serious` axe violation only surfaced after a `YY.M.D` tag was cut.
//! Reproducing it meant hand-assembling the harness the CI job wires
//! automatically: the pinned Chrome build, chromedriver, the store
//! reachability, the Lawyer grant, and the exact env the suites read.
//!
//! This command assembles that harness against the caller's worktree:
//!
//! 1. resolve + cache the **pinned** Chrome for Testing build for the
//!    host ([`super::chrome`]) — the same version `deploy.yml` pins,
//! 2. start that build's chromedriver on a free port,
//! 3. verify the web server is reachable (web runs
//!    on the host — this command does not manage either lifecycle),
//! 4. `grant-lawyer` against the caller's store (the worktree
//!    database the host `web` actually reads), and
//! 5. run `browser_e2e` + `accessibility_e2e` with the CI env
//!    (`CHROME_BINARY` / `WEBDRIVER_URL` / `NAV_BASE_URL` /
//!    `NAV_REQUIRE_HARNESS=1`) so a "harness
//!    unreachable" self-skip panics instead of passing green.
//!
//! Login-gated suites reach this bar on a worktree only because the
//! KIND Rauthy client now registers a trailing-wildcard
//! `http://localhost:*` redirect (see
//! `k8s/overlays/kind/rauthy/local-fixture.yaml`); before that fix every
//! derived worktree port 400'd at the redirect and the browser walk
//! timed out.
//!
//! ## Testing
//!
//! The orchestration shells out to `chromedriver`/`cargo test` against
//! a live cluster, so it isn't unit-tested (same policy as
//! `super::e2e`). The pure decision logic that can drift — base-URL
//! resolution, the assembled test env, and URL host/port parsing — is
//! covered by the `tests` module below.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::{chrome, e2e, require_tools, run};

/// The two suites the CI `integration` gate runs, in order.
const SUITES: [&str; 2] = ["browser_e2e", "accessibility_e2e"];

/// `dev browser-e2e`: run the deploy.yml browser gate against the
/// caller's worktree.
pub fn run_browser_e2e(base_url: Option<&str>) -> Result<()> {
    require_tools(&["cargo"])?;
    let base_url = resolve_base_url(base_url, std::env::var("PORT").ok().as_deref())?;

    // The browser walk hits the host web server, which must already be up.
    // This command drives the pinned browser against it — it does not stand
    // up the KIND fixture or the host `web` (the worktree loop owns those).
    let (web_host, web_port) = host_port_from_url(&base_url)?;
    eprintln!("=== verifying web reachable at {base_url} ===");
    wait_for_host_port(&web_host, web_port)
        .context("web server unreachable — start it with `cargo run -p neon`")?;

    // Resolve the pinned Chrome build and launch its chromedriver on a
    // free port; the guard kills it when this function returns.
    let cft = chrome::resolve()?;
    let driver_port = free_port()?;
    let _driver = Chromedriver::start(&cft.chromedriver, driver_port)?;
    let webdriver_url = format!("http://127.0.0.1:{driver_port}");

    // Lawyer must be `lawyer` in the store the host `web` reads, so the grant
    // resolves its endpoint from the same `NAVIGATOR_SURREAL_*` the host
    // `web` was started with.
    e2e::grant_lawyer_at()?;

    let env = cargo_test_env(&cft.chrome, &webdriver_url, &base_url);
    for suite in SUITES {
        eprintln!(
            "=== cargo test -p server --test {suite} (pinned Chrome, NAV_REQUIRE_HARNESS=1) ==="
        );
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "-p", "server", "--test", suite])
            .args(["--", "--test-threads=1"]);
        for (key, value) in &env {
            cmd.env(key, value);
        }
        run(&mut cmd).with_context(|| {
            format!(
                "{suite} failed — a real failure, or (with NAV_REQUIRE_HARNESS=1) a harness \
                 the suite could not reach"
            )
        })?;
    }
    eprintln!("=== browser gate green: {} ===", SUITES.join(" + "));
    Ok(())
}

/// Resolve the web base URL: an explicit `--base-url`/`$NAV_BASE_URL`
/// wins, else derive `http://localhost:$PORT` from the worktree
/// `.devx/env`. Pure.
fn resolve_base_url(explicit: Option<&str>, port_env: Option<&str>) -> Result<String> {
    if let Some(url) = explicit.and_then(non_empty) {
        return Ok(url.to_string());
    }
    if let Some(port) = port_env.and_then(non_empty) {
        return Ok(format!("http://localhost:{port}"));
    }
    bail!("no web URL — pass --base-url, set NAV_BASE_URL, or source .devx/env for PORT")
}

/// The env the browser/accessibility suites read, faithful to the
/// deploy.yml `integration` step. `CHROME_BINARY` pins the driver to
/// the resolved build; `NAV_REQUIRE_HARNESS=1` turns a self-skip into a
/// hard failure. Pure.
fn cargo_test_env(
    chrome: &Path,
    webdriver_url: &str,
    base_url: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("CHROME_BINARY", chrome.display().to_string()),
        ("WEBDRIVER_URL", webdriver_url.to_string()),
        ("NAV_BASE_URL", base_url.to_string()),
        ("NAV_REQUIRE_HARNESS", "1".to_string()),
    ]
}

/// Parse `host` + `port` out of a URL, applying the scheme's default
/// port when none is given. Pure.
fn host_port_from_url(raw: &str) -> Result<(String, u16)> {
    let url = url::Url::parse(raw).with_context(|| format!("parse URL {raw:?}"))?;
    let host = url
        .host_str()
        .with_context(|| format!("URL {raw:?} has no host"))?
        .to_string();
    let port = url
        .port_or_known_default()
        .with_context(|| format!("URL {raw:?} has no port and no known default"))?;
    Ok((host, port))
}

/// Wait up to 30s for `host:port` to accept a TCP connection. Unlike
/// `super::wait_for_tcp` (numeric `SocketAddr` only), this resolves
/// hostnames like `localhost` via `ToSocketAddrs`, which the URLs in
/// `NAV_BASE_URL` carries.
fn wait_for_host_port(host: &str, port: u16) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(addrs) = (host, port).to_socket_addrs() {
            for addr in addrs {
                if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for {host}:{port} to accept connections");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Poll chromedriver's `WebDriver` `/status` endpoint until it reports `ready`,
/// mirroring deploy.yml's `curl http://localhost:9515/status` gate. Uses a raw
/// HTTP/1.1 GET over a `TcpStream` so the synchronous harness needs no async
/// runtime or extra dependency. `ready:true` means the driver can create a
/// session; a bound socket alone does not.
fn wait_for_chromedriver_ready(host: &str, port: u16) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if chromedriver_status_ready(host, port) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for chromedriver {host}:{port}/status to report ready");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// One `GET /status` probe: connects, sends the request, and returns `true`
/// only when the JSON body carries `"ready":true`. Any connect/IO/parse failure
/// is a not-ready signal, so the caller keeps polling until the deadline.
fn chromedriver_status_ready(host: &str, port: u16) -> bool {
    let Ok(mut addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let request =
        format!("GET /status HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    // Best-effort read; chromedriver closes the connection after the body.
    let _ = stream.read_to_string(&mut response);
    status_body_is_ready(&response)
}

/// True when an HTTP `/status` response is `200 OK` and its body reports
/// `"ready":true` (whitespace-tolerant). Kept pure so it is unit-testable
/// without a live chromedriver.
fn status_body_is_ready(response: &str) -> bool {
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return false;
    };
    if !headers
        .lines()
        .next()
        .is_some_and(|status_line| status_line.contains("200"))
    {
        return false;
    }
    body.split_whitespace()
        .collect::<String>()
        .contains("\"ready\":true")
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Ask the OS for a free TCP port by binding to `:0` and reading it
/// back. A tiny race exists between release and chromedriver's bind,
/// acceptable for a dev harness.
fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind ephemeral port")?;
    let port = listener.local_addr().context("read bound port")?.port();
    drop(listener);
    Ok(port)
}

/// A running chromedriver child that is killed on drop, so the browser
/// gate never leaks a driver process when the command returns or errors.
struct Chromedriver {
    child: Child,
}

impl Chromedriver {
    fn start(binary: &Path, port: u16) -> Result<Self> {
        eprintln!(
            "=== starting chromedriver ({}) on :{port} ===",
            binary.display()
        );
        // `--allowed-origins=*` mirrors the deploy.yml chromedriver invocation
        // exactly: WebDriver clients send no Origin header so current builds
        // accept without it, but chromedriver builds that enforce request
        // origins would pass the TCP readiness check and then reject the first
        // session — a harness failure under NAV_REQUIRE_HARNESS=1 that CI never
        // hits. Matching the flag keeps local parity honest.
        let child = Command::new(binary)
            .arg(format!("--port={port}"))
            .arg("--allowed-ips=127.0.0.1")
            .arg("--allowed-origins=*")
            .spawn()
            .with_context(|| format!("spawn chromedriver {}", binary.display()))?;
        let driver = Self { child };
        // Poll the WebDriver `/status` endpoint the same way deploy.yml does
        // (`curl http://localhost:9515/status`): a bound TCP port only proves the
        // socket is listening, not that the HTTP API can serve a session. On a
        // loaded machine chromedriver binds well before `/status` reports
        // `ready`, so gating on `/session`-readiness — not a fixed sleep — keeps
        // the local harness from failing where CI would still be polling.
        wait_for_chromedriver_ready("127.0.0.1", port)
            .context("chromedriver never reported ready on /status")?;
        Ok(driver)
    }
}

impl Drop for Chromedriver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_base_url_prefers_explicit_then_port() {
        assert_eq!(
            resolve_base_url(Some("http://localhost:3091"), Some("3091")).unwrap(),
            "http://localhost:3091"
        );
        assert_eq!(
            resolve_base_url(None, Some("3091")).unwrap(),
            "http://localhost:3091"
        );
        assert_eq!(
            resolve_base_url(Some("  "), Some("3050")).unwrap(),
            "http://localhost:3050"
        );
        assert!(resolve_base_url(None, None).is_err());
        assert!(resolve_base_url(Some(""), Some("")).is_err());
    }

    #[test]
    fn cargo_test_env_carries_the_ci_contract() {
        let env = cargo_test_env(
            Path::new("/cache/chrome"),
            "http://127.0.0.1:9611",
            "http://localhost:3091",
        );
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("CHROME_BINARY"), Some("/cache/chrome"));
        assert_eq!(get("WEBDRIVER_URL"), Some("http://127.0.0.1:9611"));
        assert_eq!(get("NAV_BASE_URL"), Some("http://localhost:3091"));
        assert_eq!(get("NAV_REQUIRE_HARNESS"), Some("1"));
    }

    #[test]
    fn host_port_from_url_reads_explicit_and_default_ports() {
        assert_eq!(
            host_port_from_url("http://localhost:3091").unwrap(),
            ("localhost".to_string(), 3091)
        );
        assert_eq!(
            host_port_from_url("ws://navigator:navigator@localhost:20024/db").unwrap(),
            ("localhost".to_string(), 20024)
        );
        assert_eq!(
            host_port_from_url("http://example.test/page").unwrap(),
            ("example.test".to_string(), 80)
        );
        assert!(host_port_from_url("not a url").is_err());
    }

    #[test]
    fn status_body_is_ready_gates_on_200_and_ready_true() {
        // chromedriver's real `/status` payload once it can serve a session.
        let ready = "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\n\r\n\
             {\"value\":{\"ready\":true,\"message\":\"ChromeDriver ready for new sessions.\"}}";
        assert!(status_body_is_ready(ready));
        // Whitespace between the key and value must not matter.
        let spaced = "HTTP/1.1 200 OK\r\n\r\n{\"value\": {\"ready\" : true } }";
        assert!(status_body_is_ready(spaced));
    }

    #[test]
    fn status_body_is_ready_rejects_not_ready_and_errors() {
        // Bound but still starting up: `ready:false`.
        let not_ready = "HTTP/1.1 200 OK\r\n\r\n{\"value\":{\"ready\":false}}";
        assert!(!status_body_is_ready(not_ready));
        // Non-200 responses never count as ready.
        let unavailable = "HTTP/1.1 503 Service Unavailable\r\n\r\n{\"value\":{\"ready\":true}}";
        assert!(!status_body_is_ready(unavailable));
        // A truncated / header-less read is not ready.
        assert!(!status_body_is_ready("garbage"));
        assert!(!status_body_is_ready(""));
    }
}
