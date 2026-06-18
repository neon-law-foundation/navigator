#![allow(clippy::doc_markdown)]
//! Browser-driven accessibility gate — runs axe-core (WCAG 2.0/2.1
//! level A + AA) against the portal's create forms in a real Chromium
//! session.
//!
//! This is the deep, engine-backed counterpart to the dependency-free
//! `views/tests/accessibility.rs` structural gate. Same prerequisites
//! and skip policy as `browser_e2e.rs`: a live KIND cluster +
//! `chromedriver` on `$WEBDRIVER_URL`, Staff granted `staff`. Not
//! `#[ignore]`'d — it probes for the harness and skips cleanly when
//! absent, so it runs automatically once the harness is up:
//!
//! ```sh
//! cargo test -p web --test accessibility_e2e -- --test-threads=1
//! ```
//!
//! axe-core is vendored under `tests/assets/axe.min.js` and injected
//! here, at test time, over WebDriver. It is never linked from the app
//! layout and never served to users (see `tests/assets/README.md`).
//!
//! ## Scope
//!
//! We start deliberately narrow: axe audits only the form itself
//! ([`AXE_SCOPE`] = `form.admin-form`) so the first green run proves
//! the inject → run → collect → assert pipeline against the markup we
//! actually own. Broadening is a one-line change to [`AXE_SCOPE`]:
//! `form.admin-form` → `main` (adds the card heading + error banner)
//! → `document` (adds the shared nav/footer/banner — i.e. a full
//! page-level audit, which also unlocks landmark / lang / title
//! rules). Widen once this baseline is consistently green.

use std::time::Duration;

use fantoccini::{Client, Locator};
use features::webdriver::{base_url, login_as_staff, new_client_or_skip};

/// axe-core, injected into the page at test time only.
const AXE_SRC: &str = include_str!("assets/axe.min.js");

/// CSS selector axe scopes its audit to. Start tight (the form),
/// widen later — see the module-level "Scope" notes.
const AXE_SCOPE: &str = "form.admin-form";

/// Inject axe-core and run it over `scope` (a CSS selector). Returns
/// one human-readable line per WCAG A/AA violation.
async fn axe_violations(c: &Client, scope: &str) -> Vec<String> {
    c.execute(AXE_SRC, vec![]).await.expect("inject axe-core");
    let raw = c
        .execute_async(
            "const done = arguments[arguments.length - 1];\
             const root = document.querySelector(arguments[0]);\
             if (!root) { done(JSON.stringify([{id: 'axe-scope-missing', \
               help: 'no element matched ' + arguments[0], impact: 'serious', nodes: []}])); }\
             else { axe.run(root, {runOnly: {type: 'tag', \
               values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']}})\
               .then(r => done(JSON.stringify(r.violations)))\
               .catch(e => done(JSON.stringify([{id: 'axe-run-error', \
                 help: String(e), impact: 'serious', nodes: []}]))); }",
            vec![serde_json::Value::String(scope.to_owned())],
        )
        .await
        .expect("run axe-core");

    let json = raw.as_str().unwrap_or("[]");
    let violations: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    violations
        .iter()
        .map(|v| {
            let id = v["id"].as_str().unwrap_or("?");
            let impact = v["impact"].as_str().unwrap_or("unknown");
            let help = v["help"].as_str().unwrap_or("");
            let targets: Vec<String> = v["nodes"]
                .as_array()
                .map(|nodes| {
                    nodes
                        .iter()
                        .filter_map(|n| n["target"].as_array())
                        .map(|t| {
                            t.iter()
                                .filter_map(serde_json::Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect()
                })
                .unwrap_or_default();
            format!("[{impact}] {id}: {help} — at {}", targets.join("; "))
        })
        .collect()
}

/// Navigate to `route`, wait for the form, and fail with a readable
/// report if axe finds any WCAG A/AA violation.
async fn assert_route_passes_axe(c: &Client, route: &str) {
    c.goto(&format!("{}{route}", base_url())).await.unwrap();
    c.wait()
        .at_most(Duration::from_secs(10))
        .for_element(Locator::Css(AXE_SCOPE))
        .await
        .unwrap();
    let violations = axe_violations(c, AXE_SCOPE).await;
    assert!(
        violations.is_empty(),
        "axe found {} WCAG A/AA violation(s) within `{AXE_SCOPE}` on {route}:\n  {}",
        violations.len(),
        violations.join("\n  "),
    );
}

#[tokio::test]
async fn portal_create_forms_pass_axe_wcag_a_and_aa() {
    // The four create forms exercise every FormCard control type
    // (text, email, required selects, optional select, intro prose,
    // CSRF hidden input). Edit/detail pages share the same component
    // and are covered structurally by views/tests/accessibility.rs.
    let routes = [
        "/portal/admin/people/new",
        "/portal/admin/entities/new",
        "/portal/projects/new",
        "/portal/admin/retainers/new",
    ];

    let Some(c) = new_client_or_skip().await else {
        return;
    };
    login_as_staff(&c).await;
    for route in routes {
        assert_route_passes_axe(&c, route).await;
    }
    c.close().await.unwrap();
}
