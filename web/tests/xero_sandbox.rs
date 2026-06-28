#![allow(clippy::doc_markdown)]
//! Live Xero **demo-company** smoke test — Layer 4 validation.
//!
//! Unlike the wiremock contract tests (which prove we are consistent
//! with *ourselves*), this proves we are consistent with *Xero*: it mints
//! a real client-credentials token against the demo company's custom
//! connection and drives the find-or-create contact path through the
//! actual Accounting API. It is the only test that catches a regression
//! in our understanding of Xero's API (a wrong `where` predicate, a bad
//! scope, a contact payload the server rejects, a moved field name).
//!
//! It deliberately exercises [`web::billing::BillingProvider::ensure_contact`]
//! twice with the same unique name: the first call CREATES the contact in
//! the demo org, the second must FIND it and return the *same*
//! `ContactID` — grounding the idempotency our code assumes Xero's unique
//! contact-name rule provides (wiremock can only assume it; this proves
//! it). The demo company resets periodically, so the throwaway contacts
//! it leaves behind are self-cleaning.
//!
//! Not `#[ignore]`'d, but a live external-API test, so it runs only when
//! `NAVIGATOR_RUN_LIVE_SANDBOX=1` is set (and self-skips green otherwise,
//! plus self-skips when no Xero creds are present). The explicit opt-in
//! keeps it from hitting Xero on any ambient-credentials `cargo test` —
//! notably the ship verify, which runs under Doppler `prd`. Run
//! locally against the demo company through Doppler:
//!
//! ```bash
//! NAVIGATOR_RUN_LIVE_SANDBOX=1 doppler run --project navigator --config dev -- \
//!   cargo test -p web --test xero_sandbox -- --nocapture
//! ```
//!
//! Required env — the CI `XERO_SANDBOX_*` scheme, each falling back to the
//! canonical `XERO_*` name so `source .env` (or `doppler run`) works
//! locally:
//! - `XERO_SANDBOX_CLIENT_ID` / `XERO_CLIENT_ID`,
//!   `XERO_SANDBOX_CLIENT_SECRET` / `XERO_CLIENT_SECRET` (client-credentials
//!   grant; see `web::xero_auth`),
//! - `XERO_SANDBOX_BASE_URL` / `XERO_BASE_URL` (defaults to the Accounting
//!   API base when unset),
//! - `XERO_TENANT_ID` — the demo org's tenant GUID. **Optional**: when
//!   unset, the test auto-discovers it from Xero's `/connections` endpoint
//!   (a custom connection binds to exactly one org).

use uuid::Uuid;

use web::billing::{BillingProvider, ContactRequest, XeroBillingProvider};
use web::xero_auth::XeroClientCredentials;

/// The Accounting API base used when no override is configured — the same
/// default [`XeroBillingProvider::from_env`] applies.
const XERO_API_BASE: &str = "https://api.xero.com/api.xro/2.0";

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Read a sandbox-scoped var, falling back to the canonical `XERO_*` name,
/// so a developer can `doppler run` (which uses the runtime scheme) to
/// drive the live test; CI still overrides with the sandbox names.
fn sandbox_or_canonical(sandbox_key: &str, canonical_key: &str) -> Option<String> {
    env(sandbox_key).or_else(|| env(canonical_key))
}

/// Discover the tenant GUID from Xero's `/connections` endpoint. A custom
/// connection is bound to exactly one organisation, so the first (only)
/// entry is the demo company. Lets the test run with just the client
/// id/secret when `XERO_TENANT_ID` is unset.
async fn discover_tenant_id(access_token: &str) -> Option<String> {
    let conns: serde_json::Value = reqwest::Client::new()
        .get("https://api.xero.com/connections")
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    conns
        .get(0)
        .and_then(|c| c.get("tenantId"))
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

#[tokio::test]
async fn sandbox_resolves_a_contact_idempotently() {
    // Live external API: opt in explicitly so it never runs on an
    // ambient-credentials `cargo test` (e.g. the ship verify, which
    // runs under Doppler `prd`). Only the dedicated nightly job and an
    // on-demand run set the flag; everything else self-skips green.
    if std::env::var("NAVIGATOR_RUN_LIVE_SANDBOX").is_err() {
        eprintln!("skipping live Xero sandbox test; set NAVIGATOR_RUN_LIVE_SANDBOX=1 to run");
        return;
    }
    // Self-skip off a runner without secrets (forks, local dev). Accept
    // either the CI sandbox scheme or the canonical `.env` scheme.
    let Some(auth) =
        XeroClientCredentials::from_sandbox_env().or_else(XeroClientCredentials::from_env)
    else {
        eprintln!("skipping: no Xero client-credentials env (XERO_SANDBOX_* or XERO_*)");
        return;
    };

    // Tenant id: explicit env when set, otherwise auto-discover it from
    // `/connections` so the client id/secret alone suffice.
    let tenant_id =
        if let Some(t) = sandbox_or_canonical("XERO_SANDBOX_TENANT_ID", "XERO_TENANT_ID") {
            t
        } else {
            let token = auth
                .mint_access_token()
                .await
                .expect("client-credentials grant must mint a sandbox token");
            let Some(t) = discover_tenant_id(&token).await else {
                eprintln!("skipping: no XERO_TENANT_ID and /connections returned none");
                return;
            };
            eprintln!("auto-discovered tenant {t}");
            t
        };

    let base_url = sandbox_or_canonical("XERO_SANDBOX_BASE_URL", "XERO_BASE_URL")
        .unwrap_or_else(|| XERO_API_BASE.to_string());

    let provider = XeroBillingProvider::with_client_credentials(base_url, tenant_id, auth);

    // A unique name so each run is independent: the second resolve can
    // only return the same id by FINDING what the first call created.
    let unique = Uuid::new_v4();
    let request = ContactRequest {
        name: format!("Neon Law Navigator Sandbox {unique}"),
        email: format!("sandbox+{unique}@example.com"),
    };

    // First resolve creates the contact in the demo org.
    let first = provider
        .ensure_contact(&request)
        .await
        .expect("Xero demo must create + return a contact id");
    assert!(!first.0.is_empty(), "a real Xero ContactID comes back");
    eprintln!("created contact {} ({})", first.0, request.name);

    // Second resolve must find the same contact — never a duplicate.
    let second = provider
        .ensure_contact(&request)
        .await
        .expect("second resolve must find the existing contact");
    assert_eq!(
        first, second,
        "find-or-create is idempotent on Xero's unique contact name"
    );
    eprintln!("re-resolved same contact {} — idempotent", second.0);
}
