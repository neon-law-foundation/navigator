#![allow(clippy::doc_markdown)]
//! Browser-driven end-to-end test against a live KIND cluster.
//!
//! The whole workspace is Rust only, and the WebDriver protocol
//! gets us a real Chromium session against `localhost:8080`.
//!
//! ## Prerequisites
//!
//! 1. KIND cluster up and seeded:
//!    `cargo run --release -p cli -- deploy` (see `docs/RUNBOOK.md`).
//! 2. Staff has the `staff` role granted in Postgres (RUNBOOK step 3).
//! 3. `chromedriver` (or `geckodriver`) running on
//!    `http://localhost:9515`:
//!
//!    ```sh
//!    chromedriver --port=9515
//!    ```
//!
//! ## Run
//!
//! These tests are not `#[ignore]`'d: each probes for the harness
//! ([`new_client_or_skip`]) and skips cleanly when chromedriver or the
//! web server isn't reachable, so a plain `cargo test` (and CI without
//! the harness) stays green. With the harness up they run automatically:
//!
//! ```sh
//! cargo test -p web --test browser_e2e -- --test-threads=1
//! ```
//!
//! `NAV_BASE_URL` overrides the target (default `http://localhost:8080`);
//! `WEBDRIVER_URL` overrides the chromedriver location.

use std::env;
use std::time::Duration;

use fantoccini::Locator;
use features::webdriver::{base_url, login_as_staff, new_client_or_skip, wait_for_text};
use sea_orm::{ColumnTrait, Database, EntityTrait, QueryFilter, QueryOrder};
use store::entity::notation_event;
use uuid::Uuid;

/// Extract the notation id from a `/portal/admin/notations/:id/step` path.
fn notation_id_from_step_path(path: &str) -> Option<Uuid> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["portal", "admin", "notations", id, "step"] => Uuid::parse_str(id).ok(),
        _ => None,
    }
}

#[tokio::test]
async fn home_page_renders() {
    let Some(c) = new_client_or_skip().await else {
        return;
    };
    c.goto(&format!("{}/", base_url())).await.unwrap();
    let title = c.title().await.unwrap();
    assert!(
        title.to_lowercase().contains("neon law"),
        "expected `neon law` in page title, got `{title}`",
    );
    c.close().await.unwrap();
}

#[tokio::test]
async fn design_page_highlights_its_code_snippets() {
    // The /design gallery is public (no login) and ships the vendored
    // highlight.js next to its code blocks. highlight.js adds the `hljs`
    // class to every `<code>` it processes, so waiting for
    // `code.language-rust.hljs` proves the snippets actually highlighted in
    // a real browser — the e2e counterpart to the build-time drift test
    // that proves the snippets match their source files.
    let Some(c) = new_client_or_skip().await else {
        return;
    };
    c.goto(&format!("{}/design", base_url())).await.unwrap();
    let highlighted = c
        .wait()
        .at_most(Duration::from_secs(10))
        .for_element(Locator::Css("code.language-rust.hljs"))
        .await
        .unwrap();
    let class = highlighted.attr("class").await.unwrap().unwrap_or_default();
    assert!(
        class.contains("hljs"),
        "expected highlight.js to add the hljs class, got `{class}`"
    );
    // The highlighter wraps tokens in <span class="hljs-…"> children; their
    // presence is the strongest signal styling was applied.
    let tokens = c
        .find_all(Locator::Css("code.language-rust .hljs-keyword"))
        .await
        .unwrap();
    assert!(
        !tokens.is_empty(),
        "expected highlighted keyword tokens in the Rust snippets"
    );
    c.close().await.unwrap();
}

#[tokio::test]
async fn staff_logs_in_and_lands_on_admin() {
    let Some(c) = new_client_or_skip().await else {
        return;
    };
    login_as_staff(&c).await;

    // Sanity: confirm there's at least one admin nav link visible.
    let _ = c
        .wait()
        .at_most(Duration::from_secs(10))
        .for_element(Locator::Css("a[href^='/portal']"))
        .await
        .unwrap();

    c.close().await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn staff_walks_the_full_retainer_questionnaire_end_to_end() {
    // Drives every leg of the stepwise retainer flow in a real
    // browser:
    //   1. POST /portal/admin/retainers/new          → /step
    //   2. POST /step × 4 (one question each) → result page
    //
    // Preconditions (beyond the module's chromedriver + KIND
    // requirements): the `onboarding__retainer` template must
    // have been imported via `navigator import templates/`
    // (RUNBOOK step 4), and `store/seeds/Question.yaml` must
    // have been seeded so the four walker question codes are
    // looked up successfully.
    let Some(c) = new_client_or_skip().await else {
        return;
    };
    login_as_staff(&c).await;

    // The four answers we'll submit, in walker order
    // (client_name → client_email → project_name →
    // product_description). The values are unique enough that
    // we can fish them back out of the rendered result page.
    let client_email = format!("walk-{}@example.com", std::process::id());
    let answers = [
        "Libra",
        client_email.as_str(),
        "Estate Plan — Libra",
        "Flat-fee estate planning for the Libra family",
    ];

    // --- Step 0: create the Notation -------------------------
    c.goto(&format!("{}/portal/admin/retainers/new", base_url()))
        .await
        .unwrap();
    c.wait()
        .at_most(Duration::from_secs(10))
        .for_element(Locator::Css("input[name='client_email']"))
        .await
        .unwrap();
    // Set values via JS instead of send_keys (chromedriver
    // intermittently drops keystrokes on freshly-rendered forms).
    // `dispatchEvent('input')` keeps React/maud-equivalent listeners
    // happy and runs the browser's `required` validation against
    // the new value before we submit.
    let set_input_script = "\
        const target = document.querySelector(arguments[0]); \
        target.value = arguments[1]; \
        target.dispatchEvent(new Event('input', {bubbles: true})); \
        target.dispatchEvent(new Event('change', {bubbles: true})); \
        return target.value;";
    c.execute(
        set_input_script,
        vec![
            serde_json::Value::String("input[name='client_email']".into()),
            serde_json::Value::String(client_email.clone()),
        ],
    )
    .await
    .unwrap();
    // `retainer_template_code` renders as a <select> dropdown (the
    // onboarding-template picker), not a text input — target the element
    // that actually exists, or `querySelector` returns null and the
    // `.value =` assignment throws.
    c.execute(
        set_input_script,
        vec![
            serde_json::Value::String("select[name='retainer_template_code']".into()),
            serde_json::Value::String("onboarding__retainer".into()),
        ],
    )
    .await
    .unwrap();
    // Submit the form directly — bypasses any quirks around
    // submit-button click-event delivery in fresh-loaded DOM.
    c.execute(
        "document.querySelector('form.admin-form').submit(); return true;",
        vec![],
    )
    .await
    .unwrap();

    // POST /retainers/new redirects to /portal/admin/notations/:id/step.
    // Capture the id while we're here — we'll use it after the
    // walk to read the journal directly via SeaORM and confirm
    // exactly five `notation_events` rows landed on the
    // questionnaire timeline.
    let started = std::time::Instant::now();
    let notation_id = loop {
        let url = c.current_url().await.unwrap();
        if let Some(id) = notation_id_from_step_path(url.path()) {
            break id;
        }
        assert!(
            started.elapsed() <= Duration::from_secs(10),
            "never landed on /portal/admin/notations/:id/step; last URL was {url}",
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    // --- Steps 1–4: walk the questionnaire -------------------
    for (i, value) in answers.iter().enumerate() {
        // Each step renders "step N of 4" — wait for the right
        // one to be sure we're looking at the form we expect.
        wait_for_text(&c, &format!("step {} of 4", i + 1), Duration::from_secs(10)).await;

        // Set the answer value via JS (chromedriver send_keys is
        // unreliable on freshly-rendered forms).
        c.execute(
            "\
            const target = document.querySelector(\
              'input[name=\"value\"], textarea[name=\"value\"]'); \
            target.value = arguments[0]; \
            target.dispatchEvent(new Event('input', {bubbles: true})); \
            target.dispatchEvent(new Event('change', {bubbles: true})); \
            return target.value;",
            vec![serde_json::Value::String((*value).to_string())],
        )
        .await
        .unwrap();
        c.execute(
            "document.querySelector('form.admin-form').submit(); return true;",
            vec![],
        )
        .await
        .unwrap();
    }

    // --- Result page -----------------------------------------
    // The fourth submit drives the post-intake workflow and
    // renders the result. The result page shows the workflow
    // terminal state and the substituted template body.
    wait_for_text(&c, "sent_for_signature__pending", Duration::from_secs(20)).await;
    let src = c.source().await.unwrap();
    for value in &answers {
        assert!(
            src.contains(value),
            "rendered retainer is missing `{value}`",
        );
    }

    c.close().await.unwrap();

    // --- Journal: read `notation_events` via SeaORM ----------
    // Talks to the in-cluster Postgres through `navigator start-dev-server`'s
    // port-forward (`localhost:15432`); `DATABASE_URL` is exported
    // by `.devx/env`. We read through the same SeaORM entity the
    // worker writes through, so the assertion exercises the wire
    // shape end-to-end without shelling out to `psql`.
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set — `set -a; source .devx/env; set +a`");
    let db = Database::connect(&database_url)
        .await
        .expect("connect to the port-forwarded Postgres");
    let events = notation_event::Entity::find()
        .filter(notation_event::Column::NotationId.eq(notation_id))
        .filter(notation_event::Column::MachineKind.eq(notation_event::MACHINE_QUESTIONNAIRE))
        .order_by_asc(notation_event::Column::Id)
        .all(&db)
        .await
        .expect("read notation_events from postgres");
    db.close().await.ok();

    // Five rows: BEGIN → client_name → client_email →
    // project_name → product_description → END. The walker
    // signals the worker once per question (four times) and once
    // more for the BEGIN-of-walk-to-END trailer in the last POST.
    assert_eq!(
        events.len(),
        5,
        "expected 5 questionnaire transitions for notation {notation_id}, got {events:?}",
    );
    let states: Vec<(&str, &str, &str)> = events
        .iter()
        .map(|e| {
            (
                e.from_state.as_str(),
                e.to_state.as_str(),
                e.condition.as_str(),
            )
        })
        .collect();
    assert_eq!(
        states,
        vec![
            ("BEGIN", "client_name", "_"),
            ("client_name", "client_email", "_"),
            ("client_email", "project_name", "_"),
            ("project_name", "product_description", "_"),
            ("product_description", "END", "_"),
        ],
        "questionnaire walked the wrong path",
    );
    // Payload assertions: the walker now threads the respondent's
    // answer through the signal so each of the four answered
    // transitions carries `{"answer_value": "..."}`. The trailing
    // `product_description → END` row has no answer and stays
    // NULL. Build the expected JSON via the same `answer_payload`
    // helper the worker uses so a future change to the JSON shape
    // can't desync the test from production.
    let expected_payloads: Vec<Option<String>> = answers
        .iter()
        .map(|v| Some(notation_event::answer_payload(v)))
        .chain(std::iter::once(None))
        .collect();
    let actual_payloads: Vec<Option<String>> = events.iter().map(|e| e.payload.clone()).collect();
    assert_eq!(
        actual_payloads, expected_payloads,
        "journal payload column does not match the answers the walker submitted",
    );
}

#[tokio::test]
async fn staff_user_can_hit_every_admin_route() {
    // Walks the same eight admin routes the in-process test
    // (`oidc_e2e::user_with_db_staff_role_can_hit_every_admin_route`)
    // covers, but through a real browser end-to-end.
    let routes = [
        "/portal/admin",
        "/portal/admin/people",
        "/portal/admin/entities",
        "/portal/admin/jurisdictions",
        "/portal/admin/entity-types",
        "/portal/admin/templates",
        "/portal/admin/questions",
        "/portal/projects",
    ];

    let Some(c) = new_client_or_skip().await else {
        return;
    };
    login_as_staff(&c).await;

    // Each portal route should render without a server-error
    // status. WebDriver doesn't expose HTTP status directly so we
    // check for a non-error body — the back-office views all carry
    // an `<a href="/portal">` link in their nav.
    for route in routes {
        c.goto(&format!("{}{route}", base_url())).await.unwrap();
        let nav_links = c
            .find_all(Locator::Css("a[href^='/portal']"))
            .await
            .unwrap();
        assert!(
            !nav_links.is_empty(),
            "expected portal nav on {route}; got no /portal links — was access denied?",
        );
    }

    c.close().await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn staff_opens_an_estate_matter_and_sees_the_transcript_form() {
    // Drives the Northstar estate front edge in a real browser:
    //   1. POST /portal/admin/retainers/new with onboarding__estate
    //   2. land on the matter page (/portal/projects/:id), not the walker
    //   3. the phone-friendly transcript-upload form is present
    //
    // Preconditions (beyond chromedriver + KIND): the canonical seed has
    // run so the `onboarding__estate` template exists. The client-side
    // approval walk is covered by the in-process integration test
    // `estate_review_gates.rs` (a WebDriver client-login helper is not
    // built yet).
    let Some(c) = new_client_or_skip().await else {
        return;
    };
    login_as_staff(&c).await;

    let client_email = format!("estate-{}@example.com", std::process::id());

    c.goto(&format!("{}/portal/admin/retainers/new", base_url()))
        .await
        .unwrap();
    c.wait()
        .at_most(Duration::from_secs(10))
        .for_element(Locator::Css("input[name='client_email']"))
        .await
        .unwrap();
    let set_input_script = "\
        const target = document.querySelector(arguments[0]); \
        target.value = arguments[1]; \
        target.dispatchEvent(new Event('input', {bubbles: true})); \
        target.dispatchEvent(new Event('change', {bubbles: true})); \
        return target.value;";
    c.execute(
        set_input_script,
        vec![
            serde_json::Value::String("input[name='client_email']".into()),
            serde_json::Value::String(client_email.clone()),
        ],
    )
    .await
    .unwrap();
    c.execute(
        set_input_script,
        vec![
            serde_json::Value::String("select[name='retainer_template_code']".into()),
            serde_json::Value::String("onboarding__estate".into()),
        ],
    )
    .await
    .unwrap();
    c.execute(
        "document.querySelector('form.admin-form').submit(); return true;",
        vec![],
    )
    .await
    .unwrap();

    // The estate flow lands on the matter page with the transcript form —
    // never the questionnaire walker. The matter page is project-scoped:
    // the staffer who opened it must be disclosed to it (a
    // `person_project_roles` staff-DRI row) or `can_see_project` 404s them.
    // `start_post` writes that row as part of creation, so the opener lands
    // on the transcript form rather than a "Not found" page. The estate
    // create also starts the workflow machine through Restate in-request, so
    // allow a generous budget for that cross-pod round-trip.
    wait_for_text(&c, "File the sitting transcript", Duration::from_secs(15)).await;
    let url = c.current_url().await.unwrap();
    assert!(
        url.path().starts_with("/portal/projects/"),
        "estate creation should land on the matter page, got {url}"
    );

    c.close().await.unwrap();
}
