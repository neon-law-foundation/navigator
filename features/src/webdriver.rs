//! Shared `WebDriver` helpers for browser-driven scenarios.
//!
//! Gated behind the `webdriver` Cargo feature so the default BDD
//! build (which drives the router via `tower::ServiceExt::oneshot`)
//! doesn't have to compile fantoccini. The legacy
//! `web/tests/browser_e2e.rs` suite turns the feature on, and any
//! future `.feature` runners that need a real Chromium session can
//! do the same.

use std::env;
use std::time::Duration;

use fantoccini::{Client, ClientBuilder, Locator};
use serde_json::json;
use tokio::net::TcpStream;
use url::Url;

/// `NAV_BASE_URL` (default `http://localhost:8080`). The HTTP origin
/// that the browser navigates to.
#[must_use]
pub fn base_url() -> String {
    env::var("NAV_BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

/// `WEBDRIVER_URL` (default `http://localhost:9515`). Where
/// chromedriver/geckodriver is listening.
#[must_use]
pub fn webdriver_url() -> String {
    env::var("WEBDRIVER_URL").unwrap_or_else(|_| "http://localhost:9515".to_string())
}

/// Build a fantoccini client connected to chromedriver, running
/// headless by default. Set `WEBDRIVER_HEADED=1` to watch Chrome
/// step through the flow.
///
/// # Panics
///
/// Panics if chromedriver isn't reachable at [`webdriver_url`] — the
/// browser tests are `#[ignore]`'d so an unreachable driver is a
/// caller bug, not a transient flake.
pub async fn new_client() -> Client {
    let headed = env::var("WEBDRIVER_HEADED").is_ok();
    let mut args: Vec<&str> = vec![
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--window-size=1280,800",
    ];
    if !headed {
        args.push("--headless=new");
    }
    let caps: serde_json::Map<String, serde_json::Value> = json!({
        "goog:chromeOptions": { "args": args },
    })
    .as_object()
    .cloned()
    .unwrap();

    ClientBuilder::native()
        .capabilities(caps)
        .connect(&webdriver_url())
        .await
        .expect("connect to chromedriver — is it running on $WEBDRIVER_URL?")
}

/// Wait for the browser to land at exactly `{base_url}{path}`.
/// Uses fantoccini's `for_url` explicit wait — no sleep polling, no
/// manual deadline tracking.
///
/// # Panics
///
/// Panics if `path` doesn't combine with [`base_url`] into a valid
/// URL, or if the page never reaches the target within `timeout`.
pub async fn wait_for_path(c: &Client, path: &str, timeout: Duration) {
    let target = Url::parse(&format!("{}{path}", base_url())).expect("valid url");
    c.wait()
        .at_most(timeout)
        .for_url(&target)
        .await
        .expect("never reached expected URL");
}

/// Wait up to `timeout` for the page source to contain `needle`.
///
/// Fantoccini 0.21's `Wait` API only exposes `for_element` and
/// `for_url` — no generic predicate — so a page-source substring
/// check still has to poll. Kept as a tight helper so the polling
/// pattern lives in exactly one place.
///
/// # Panics
///
/// Panics if `needle` never appears within `timeout`, or if the
/// browser refuses a `source()` query.
pub async fn wait_for_text(c: &Client, needle: &str, timeout: Duration) {
    let started = std::time::Instant::now();
    loop {
        let src = c.source().await.unwrap();
        if src.contains(needle) {
            return;
        }
        assert!(
            started.elapsed() <= timeout,
            "never saw `{needle}` in page source within {timeout:?}",
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Drive the Keycloak login form for the bundled `staff/staff`
/// developer account and wait for the post-callback `/portal`
/// redirect to settle.
///
/// # Panics
///
/// Panics if the login form never renders, if any of the form-field
/// interactions fail, or if the page never lands on `/portal` within
/// 20 seconds.
pub async fn login_as_staff(c: &Client) {
    c.goto(&format!("{}/auth/login?return_to=/portal", base_url()))
        .await
        .unwrap();
    c.wait()
        .at_most(Duration::from_secs(20))
        .for_element(Locator::Css("input[name='username']"))
        .await
        .unwrap();
    c.find(Locator::Css("input[name='username']"))
        .await
        .unwrap()
        .send_keys("staff")
        .await
        .unwrap();
    c.find(Locator::Css("input[name='password']"))
        .await
        .unwrap()
        .send_keys("staff")
        .await
        .unwrap();
    c.find(Locator::Css("input[type='submit'], button[type='submit']"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    wait_for_path(c, "/portal", Duration::from_secs(20)).await;
}

/// True when both chromedriver ([`webdriver_url`]) and the target web
/// server ([`base_url`]) accept a TCP connection — i.e. the live browser
/// harness (`navigator e2e`: a KIND web server plus a running chromedriver)
/// is up.
///
/// Browser tests call this and skip when it returns `false`, so the
/// default `cargo test` (and CI without the harness) stays green while
/// the same tests run for real under `navigator e2e`. This is what lets the
/// browser suite drop its blanket `#[ignore]`: presence of the harness,
/// not a hand-passed `--ignored`, decides whether a scenario executes.
#[must_use]
pub async fn harness_ready() -> bool {
    port_open(&webdriver_url()).await && port_open(&base_url()).await
}

/// Connect a browser client, or return `None` (with a skip note) when
/// the live harness isn't up. Browser tests use this instead of
/// [`new_client`] so a missing chromedriver/server makes the scenario
/// skip cleanly rather than panic — the suite stays green everywhere and
/// the same test runs for real under `navigator e2e`.
pub async fn new_client_or_skip() -> Option<Client> {
    match harness_decision(harness_ready().await, require_harness()) {
        HarnessDecision::Connect => Some(new_client().await),
        // In CI the harness is always expected up, so an unreachable harness
        // is a real failure, not a green pass — panic for a non-zero exit.
        HarnessDecision::Fail => panic!(
            "NAV_REQUIRE_HARNESS=1 but the browser harness is unreachable: \
             chromedriver ({}) + web server ({}) not both reachable \
             — refusing to pass without asserting",
            webdriver_url(),
            base_url(),
        ),
        // Locally (NAV_REQUIRE_HARNESS unset) a missing harness skips cleanly
        // so a bare `cargo test` stays green without standing one up.
        HarnessDecision::Skip => {
            eprintln!(
                "skipping browser test: chromedriver ({}) + web server ({}) not both reachable \
                 — bring up the harness with `navigator e2e`",
                webdriver_url(),
                base_url(),
            );
            None
        }
    }
}

/// What [`new_client_or_skip`] should do, given whether the harness is
/// reachable and whether CI requires it. Pulled out as a pure function so
/// the gating policy — the thing that decides whether a missing harness is
/// a clean skip or a hard failure — is exhaustively unit-testable without a
/// live browser or any environment mutation.
#[derive(Debug, PartialEq, Eq)]
enum HarnessDecision {
    /// Harness is up — connect and run the scenario for real.
    Connect,
    /// Harness is down and CI required it — fail loudly (non-zero exit).
    Fail,
    /// Harness is down and it's optional — skip cleanly, stay green.
    Skip,
}

/// The pure gating rule. A reachable harness always connects; an
/// unreachable one fails when required (CI) and skips otherwise (local).
fn harness_decision(ready: bool, require: bool) -> HarnessDecision {
    match (ready, require) {
        (true, _) => HarnessDecision::Connect,
        (false, true) => HarnessDecision::Fail,
        (false, false) => HarnessDecision::Skip,
    }
}

/// Whether a missing harness must fail (not skip) the test. CI sets
/// `NAV_REQUIRE_HARNESS=1` so a self-skip can't pass green; locally the
/// var is unset and the harness-probe skip stays in effect. Accepts `1`
/// or `true` (case-insensitive); anything else (incl. unset) is `false`.
#[must_use]
pub fn require_harness() -> bool {
    std::env::var("NAV_REQUIRE_HARNESS")
        .ok()
        .is_some_and(|v| harness_required_from(&v))
}

/// Pure parse of the `NAV_REQUIRE_HARNESS` value: `1` or `true`
/// (case-insensitive) enable the require-harness gate; anything else is
/// off. Split out so the policy is unit-testable without mutating the
/// process environment.
fn harness_required_from(value: &str) -> bool {
    let v = value.trim();
    v == "1" || v.eq_ignore_ascii_case("true")
}

/// Best-effort TCP reachability probe for an `http(s)://host:port` URL,
/// with a short timeout so a missing harness fails fast rather than
/// hanging the suite.
async fn port_open(url_str: &str) -> bool {
    let Ok(u) = Url::parse(url_str) else {
        return false;
    };
    let Some(host) = u.host_str() else {
        return false;
    };
    let port = u.port_or_known_default().unwrap_or(80);
    matches!(
        tokio::time::timeout(Duration::from_secs(2), TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::{harness_decision, harness_required_from, HarnessDecision};

    #[test]
    fn harness_required_only_for_truthy_values() {
        assert!(harness_required_from("1"));
        assert!(harness_required_from("true"));
        assert!(harness_required_from("TRUE"));
        assert!(harness_required_from("  1 "));
        assert!(!harness_required_from("0"));
        assert!(!harness_required_from("false"));
        assert!(!harness_required_from(""));
        assert!(!harness_required_from("yes"));
    }

    #[test]
    fn harness_decision_covers_every_case() {
        // A reachable harness always runs the scenario for real, regardless
        // of the require flag.
        assert_eq!(harness_decision(true, true), HarnessDecision::Connect);
        assert_eq!(harness_decision(true, false), HarnessDecision::Connect);
        // An unreachable harness fails when CI required it (no false green)…
        assert_eq!(harness_decision(false, true), HarnessDecision::Fail);
        // …and skips cleanly when it's optional (local convenience).
        assert_eq!(harness_decision(false, false), HarnessDecision::Skip);
    }
}
