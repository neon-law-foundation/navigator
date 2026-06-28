#![allow(clippy::doc_markdown)]
//! Live DocuSign **demo/sandbox** smoke test — Layer 4 validation.
//!
//! Unlike the wiremock contract tests (which prove we are consistent
//! with *ourselves*), this proves we are consistent with *DocuSign*: it
//! mints a real access token via JWT grant, builds a retainer PDF with
//! anchored signature tabs, creates a real envelope in the demo
//! environment, and requests an embedded recipient-view signing URL for
//! the captive client. It is the only test that can catch a regression
//! in our understanding of DocuSign's API (a wrong tab field name, a bad
//! anchor, an envelope the server rejects, a broken recipient view).
//!
//! What it cannot do without a human (documented inline at the end):
//! drive the signing ceremony to `completed`, download the executed
//! documents, and capture real Connect completion/decline payloads + the
//! HMAC header. Those steps ground the webhook + HMAC and must be run by
//! hand against the sandbox Connect log.
//!
//! Not `#[ignore]`'d, but a live external-API test, so it runs only when
//! `NAVIGATOR_RUN_LIVE_SANDBOX=1` is set (and self-skips green otherwise,
//! plus self-skips when no DocuSign JWT env is present). The explicit
//! opt-in keeps it from firing a real envelope on any ambient-credentials
//! `cargo test` — notably the ship verify, which runs under Doppler
//! `prd` (a `na4.docusign.net` PRODUCTION base URL). Run locally with the
//! sandbox vars set:
//!
//! ```bash
//! NAVIGATOR_RUN_LIVE_SANDBOX=1 cargo test -p web --test docusign_sandbox
//! ```
//!
//! Required env — the CI `DOCUSIGN_SANDBOX_*` scheme, each falling back
//! to the canonical `DOCUSIGN_*` name so `source .env` works locally:
//! - `DOCUSIGN_SANDBOX_INTEGRATION_KEY` / `DOCUSIGN_INTEGRATION_KEY`,
//!   `DOCUSIGN_SANDBOX_USER_ID` / `DOCUSIGN_USER_ID`,
//!   `DOCUSIGN_SANDBOX_RSA_KEY` / `DOCUSIGN_PRIVATE_KEY` (JWT grant; see
//!   `web::docusign_auth`),
//! - `DOCUSIGN_SANDBOX_BASE_URL` / `DOCUSIGN_BASE_URL`
//!   (e.g. `https://demo.docusign.net/restapi`),
//! - `DOCUSIGN_SANDBOX_ACCOUNT_ID` / `DOCUSIGN_ACCOUNT_ID`,
//!   `DOCUSIGN_SANDBOX_SIGNER_EMAIL` / `DOCUSIGN_SIGNER_EMAIL`.

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use web::docusign_auth::DocuSignJwtAuth;
use web::signature::{
    RecipientView, SignatureField, SignatureFieldKind, SignatureManifest, SignatureProvider,
    SignatureRecipient,
};
use web::signature_render::expand_signatures;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Read a sandbox-scoped var, falling back to the canonical `DOCUSIGN_*`
/// name. This lets a developer simply `source .env` (which uses the
/// runtime scheme) to drive the live test, instead of exporting a
/// parallel `DOCUSIGN_SANDBOX_*` set; CI still overrides with the
/// sandbox names.
fn sandbox_or_canonical(sandbox_key: &str, canonical_key: &str) -> Option<String> {
    env(sandbox_key).or_else(|| env(canonical_key))
}

#[tokio::test]
async fn sandbox_accepts_a_retainer_envelope_with_anchor_tabs() {
    // Live external API that can reach PRODUCTION DocuSign when canonical
    // `DOCUSIGN_*` creds are ambient (the ship verify runs under
    // Doppler `prd`, whose base URL is `na4.docusign.net`). Opt in
    // explicitly so it never fires a real envelope on a normal test run —
    // only the dedicated nightly job and an on-demand run set the flag.
    if std::env::var("NAVIGATOR_RUN_LIVE_SANDBOX").is_err() {
        eprintln!("skipping live DocuSign sandbox test; set NAVIGATOR_RUN_LIVE_SANDBOX=1 to run");
        return;
    }
    // Self-skip off a runner without secrets (forks, local dev). Accept
    // either the CI sandbox scheme or the canonical `.env` scheme.
    let Some(auth) = DocuSignJwtAuth::from_sandbox_env().or_else(DocuSignJwtAuth::from_env) else {
        eprintln!("skipping: no DocuSign JWT env (DOCUSIGN_SANDBOX_* or DOCUSIGN_*)");
        return;
    };
    let (Some(base_url), Some(account_id), Some(signer_email)) = (
        sandbox_or_canonical("DOCUSIGN_SANDBOX_BASE_URL", "DOCUSIGN_BASE_URL"),
        sandbox_or_canonical("DOCUSIGN_SANDBOX_ACCOUNT_ID", "DOCUSIGN_ACCOUNT_ID"),
        sandbox_or_canonical("DOCUSIGN_SANDBOX_SIGNER_EMAIL", "DOCUSIGN_SIGNER_EMAIL"),
    ) else {
        eprintln!("skipping: base URL / account id / signer email not set");
        return;
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let token = auth
        .mint_access_token(now)
        .await
        .expect("JWT grant must mint a sandbox access token");

    // Render a tiny retainer whose body anchors a client signature.
    let (typst, fields) =
        expand_signatures("Sandbox retainer.\n\nSign below:\n\n{{client.signature}}\n");
    assert!(!fields.is_empty(), "the body must place a signature field");
    let pdf = pdf::render(&typst).expect("signature blocks compile to a PDF");

    // One real signer (the dev account's signer email) keyed to the
    // anchored field. Mark them CAPTIVE (`client_user_id`) so this also
    // grounds the embedded-signing path: DocuSign suppresses the email
    // and we drive `createRecipientView` below.
    let client_user_id = "sandbox-embedded-client".to_string();
    let manifest = SignatureManifest {
        recipients: vec![SignatureRecipient {
            role: "client".into(),
            email: signer_email.clone(),
            name: "Sandbox Signer".into(),
            routing_order: 1,
            client_user_id: Some(client_user_id.clone()),
        }],
        fields: vec![SignatureField {
            recipient_role: "client".into(),
            kind: SignatureFieldKind::Signature,
            anchor: fields[0].anchor.clone(),
        }],
    };

    let provider = web::signature::DocuSignSignatureProvider::new(
        base_url,
        account_id,
        token,
        "support@neonlaw.com",
        "Neon Law",
    );
    let id = provider
        .send_for_signature(Uuid::new_v4(), &pdf, &manifest)
        .await
        .expect("DocuSign demo must accept our envelope + anchored tabs");
    assert!(!id.0.is_empty(), "a real envelope id comes back");
    eprintln!("created sandbox envelope {}", id.0);

    // Embedded-signing grounding (Phase 0.3): request a recipient view
    // for the captive signer. This proves the `views/recipient` path,
    // the clientUserId match, and that DocuSign issues a real signing
    // URL — the URL a human then opens to actually sign.
    let signing_url = provider
        .create_recipient_view(
            &id,
            &RecipientView {
                return_url: "https://www.neonlaw.com/portal".into(),
                email: signer_email,
                name: "Sandbox Signer".into(),
                client_user_id,
            },
        )
        .await
        .expect("DocuSign demo must issue an embedded recipient view URL");
    assert!(
        signing_url.starts_with("https://"),
        "recipient view returns a real signing URL: {signing_url}"
    );
    eprintln!("embedded signing URL: {signing_url}");

    // Ground the scheme-less OAuth-base hardening against the REAL demo
    // OAuth server (not just the unit test in `docusign_auth`). Prod's
    // `DOCUSIGN_OAUTH_BASE` was once configured without a scheme
    // (`account.docusign.com`), which made `mint` build a relative token
    // URL and fail. Build an auth whose base is deliberately scheme-less
    // and confirm it still mints a real token — i.e. `normalize_oauth_base`
    // produces a URL DocuSign actually accepts.
    let (Some(ik), Some(user), Some(pem)) = (
        sandbox_or_canonical(
            "DOCUSIGN_SANDBOX_INTEGRATION_KEY",
            "DOCUSIGN_INTEGRATION_KEY",
        ),
        sandbox_or_canonical("DOCUSIGN_SANDBOX_USER_ID", "DOCUSIGN_USER_ID"),
        sandbox_or_canonical("DOCUSIGN_SANDBOX_RSA_KEY", "DOCUSIGN_PRIVATE_KEY"),
    ) else {
        eprintln!("skipping scheme-less grounding: JWT grant env not fully present");
        return;
    };
    // `account-d.docusign.com` — the demo OAuth host, WITHOUT a scheme.
    let scheme_less = DocuSignJwtAuth::new(ik, user, "account-d.docusign.com", pem.into_bytes());
    let token2 = scheme_less
        .mint_access_token(now)
        .await
        .expect("a scheme-less demo OAuth base must still mint a real token");
    assert!(
        !token2.is_empty(),
        "normalize_oauth_base yields a token URL DocuSign accepts"
    );
    eprintln!(
        "scheme-less OAuth base minted a live demo token ({} chars)",
        token2.len()
    );

    // NOTE — remaining grounding that needs a HUMAN (cannot be automated
    // here; DocuSign has no API to programmatically click "sign"):
    //   1. Open `signing_url`, sign, and let the envelope reach
    //      `completed`. Then `provider.fetch_completed_documents(&id)`
    //      should return the combined signed PDF + the Certificate of
    //      Completion (the `/documents/combined` + `/documents/certificate`
    //      paths). Before completion DocuSign returns 409/404 for those,
    //      so we do not assert the download here.
    //   2. From the sandbox Connect log, capture a real `completed` and a
    //      real `declined` payload and pin them as fixtures, then assert
    //      `ConnectPayload::is_completed` / `is_declined` (the hermetic
    //      tests in `esignature_webhook` currently use hand-written shapes).
    //   3. Configure a sandbox Connect webhook with an HMAC key, capture
    //      the `X-DocuSign-Signature-1` header on a real delivery, and
    //      confirm `web::webhook_auth::verify_hmac_sha256_b64` accepts it
    //      — the single most important thing to ground, since the prod
    //      HMAC key is still a placeholder.
}
