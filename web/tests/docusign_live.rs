#![allow(clippy::doc_markdown)]
//! Live DocuSign tests that hit the REAL API and are **never** run in
//! CI/CD — they are on-demand grounding tools, run by hand under
//! `doppler run`.
//!
//! Two tiers, each behind its OWN opt-in env flag so neither can fire
//! from `cargo test --workspace` (the main CI command sets no flag). No
//! workflow runs these: the former `docusign-sandbox.yml` canary was
//! removed when CI/CD collapsed to three workflows (PR / cron / tag — see
//! CLAUDE.md), so every live DocuSign path is now on-demand only, run by
//! hand under `doppler run`:
//!
//! 1. [`prod_jwt_and_billing_plan_checkpoint`] — gated on
//!    `NAVIGATOR_RUN_LIVE_PROD_CHECK=1`. FREE: mints a prod JWT and reads
//!    the account billing plan. Burns no envelope. The cheap checkpoint
//!    that confirms prod auth works AND prints our real envelope
//!    allowance. Run under Doppler `prd`:
//!
//!    ```bash
//!    doppler run --project navigator --config prd -- \
//!      env NAVIGATOR_RUN_LIVE_PROD_CHECK=1 \
//!      cargo test -p web --test docusign_live -- --nocapture prod_jwt_and_billing
//!    ```
//!
//! 2. [`emailed_envelope_to_a_real_signer`] — gated on
//!    `NAVIGATOR_RUN_LIVE_EMAILED_ENVELOPE=1` AND a required, no-default
//!    `NAVIGATOR_LIVE_SIGNER_EMAIL`. BILLABLE on prod (1 envelope of the
//!    monthly allowance) — it sends a real, emailable envelope a human
//!    then signs from their inbox, which fires the real Connect webhook
//!    to our website and so grounds the HMAC end to end. Demo (Doppler
//!    `dev`) is free + watermarked; prod (`prd`) is the real artifact.
//!    The recipient email has NO default, so the test can never silently
//!    email anyone:
//!
//!    ```bash
//!    # Free dry run on the demo account:
//!    doppler run --project navigator --config dev -- \
//!      env NAVIGATOR_RUN_LIVE_EMAILED_ENVELOPE=1 \
//!          NAVIGATOR_LIVE_SIGNER_EMAIL=nick@shook.family \
//!      cargo test -p web --test docusign_live -- --nocapture emailed_envelope
//!
//!    # The real, billable prod send (1 envelope):
//!    doppler run --project navigator --config prd -- \
//!      env NAVIGATOR_RUN_LIVE_EMAILED_ENVELOPE=1 \
//!          NAVIGATOR_LIVE_SIGNER_EMAIL=nick@shook.family \
//!      cargo test -p web --test docusign_live -- --nocapture emailed_envelope
//!    ```
//!
//! DO NOT add a workflow that sets either flag, and DO NOT set
//! `NAVIGATOR_LIVE_SIGNER_EMAIL` in CI — that is the contract that keeps
//! a real envelope from being sent on an automated run.

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use web::docusign_auth::DocuSignJwtAuth;
use web::signature::{
    SignatureField, SignatureFieldKind, SignatureManifest, SignatureProvider, SignatureRecipient,
};
use web::signature_render::expand_signatures;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Read a sandbox-scoped var, falling back to the canonical `DOCUSIGN_*`
/// name, so the same test drives demo (`dev`) or prod (`prd`) purely off
/// which Doppler config it runs under.
fn sandbox_or_canonical(sandbox_key: &str, canonical_key: &str) -> Option<String> {
    env(sandbox_key).or_else(|| env(canonical_key))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// FREE prod checkpoint: mint a JWT and read the billing plan. No
/// envelope is created, so this costs nothing against the allowance.
#[tokio::test]
async fn prod_jwt_and_billing_plan_checkpoint() {
    if std::env::var("NAVIGATOR_RUN_LIVE_PROD_CHECK").is_err() {
        eprintln!(
            "skipping prod checkpoint; set NAVIGATOR_RUN_LIVE_PROD_CHECK=1 (run under Doppler prd)"
        );
        return;
    }
    let Some(auth) = DocuSignJwtAuth::from_env() else {
        eprintln!("skipping: no DocuSign JWT env (DOCUSIGN_INTEGRATION_KEY/_USER_ID/_PRIVATE_KEY)");
        return;
    };
    let (Some(base_url), Some(account_id)) = (
        sandbox_or_canonical("DOCUSIGN_SANDBOX_BASE_URL", "DOCUSIGN_BASE_URL"),
        sandbox_or_canonical("DOCUSIGN_SANDBOX_ACCOUNT_ID", "DOCUSIGN_ACCOUNT_ID"),
    ) else {
        eprintln!("skipping: base URL / account id not set");
        return;
    };

    // 1. Mint — the free proof that prod keypair + consent are in place.
    let token = auth
        .mint_access_token(now_secs())
        .await
        .expect("prod JWT grant must mint an access token (needs prod keypair + consent)");
    assert!(!token.is_empty(), "a real access token comes back");
    eprintln!("prod JWT mint OK ({} char token)", token.len());

    // 2. Read the billing plan — read-only GET, burns no envelope. Dump
    //    the JSON so we can SEE the plan name + envelope allowance and
    //    settle the "~40 vs 50" question against ground truth.
    let url = format!(
        "{}/v2.1/accounts/{}/billing_plan",
        base_url.trim_end_matches('/'),
        account_id
    );
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .expect("billing_plan request sends");
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "billing_plan must return 2xx, got {status}: {body}"
    );
    // Pretty-print the relevant slice if it parses; else the raw body.
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(json) => {
            let plan = &json["billingPlan"];
            eprintln!("plan name: {}", plan["planName"]);
            eprintln!(
                "billing plan JSON:\n{}",
                serde_json::to_string_pretty(&json).unwrap_or(body)
            );
        }
        Err(_) => eprintln!("billing plan (raw): {body}"),
    }
}

/// BILLABLE on prod: send a real, emailed envelope to a real signer.
/// This is the round-trip that, once the human signs, drives the real
/// Connect webhook to our website and grounds the HMAC. Requires an
/// explicit recipient — there is no default, so an automated run with
/// the flag but no email simply skips.
#[tokio::test]
async fn emailed_envelope_to_a_real_signer() {
    if std::env::var("NAVIGATOR_RUN_LIVE_EMAILED_ENVELOPE").is_err() {
        eprintln!(
            "skipping live emailed envelope; set NAVIGATOR_RUN_LIVE_EMAILED_ENVELOPE=1 to run"
        );
        return;
    }
    // Hard requirement: an explicit recipient. Without it we never send,
    // so the test cannot email a hardcoded or stale address by accident.
    let Some(signer_email) = env("NAVIGATOR_LIVE_SIGNER_EMAIL") else {
        eprintln!(
            "skipping: set NAVIGATOR_LIVE_SIGNER_EMAIL to the real signer (no default — \
             this guards against an accidental send)"
        );
        return;
    };
    let Some(provider) = web::signature::DocuSignSignatureProvider::from_env() else {
        eprintln!("skipping: no DocuSign provider env (DOCUSIGN_BASE_URL/_ACCOUNT_ID + auth)");
        return;
    };

    // A retainer whose body anchors the client's signature.
    let (typst, fields) =
        expand_signatures("Retainer.\n\nPlease sign below:\n\n{{client.signature}}\n");
    assert!(!fields.is_empty(), "the body must place a signature field");
    let pdf = pdf::render(&typst).expect("signature blocks compile to a PDF");

    // EMAILED (non-captive) signer: no `client_user_id`, so DocuSign
    // emails `signer_email` a real signing link instead of issuing an
    // embedded view. This is the human-in-the-loop path the captive
    // sandbox test cannot exercise.
    let manifest = SignatureManifest {
        recipients: vec![SignatureRecipient {
            role: "client".into(),
            email: signer_email.clone(),
            name: "Real Signer".into(),
            routing_order: 1,
            client_user_id: None,
        }],
        fields: vec![SignatureField {
            recipient_role: "client".into(),
            kind: SignatureFieldKind::Signature,
            anchor: fields[0].anchor.clone(),
        }],
    };

    let id = provider
        .send_for_signature(Uuid::new_v4(), &pdf, &manifest)
        .await
        .expect("DocuSign must accept the envelope + email the signer");
    assert!(!id.0.is_empty(), "a real envelope id comes back");
    eprintln!("sent emailed envelope {} to {signer_email}", id.0);
    eprintln!(
        "NEXT: the signer opens the DocuSign email and signs. On completion, Connect POSTs to \
         our website (/webhook/esignature/<secret>); confirm HMAC by tailing the web pod for \
         'esignature webhook: signature event' (good) vs 'signature verification failed' (bad)."
    );
}
