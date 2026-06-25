# DocuSign e-signature — setup, signing flow, and production cutover

How Neon Law Navigator sends a retainer for signature, how a client signs it inside the portal, and how **one** DocuSign
app serves both the demo and production environments. Secrets are selected per environment by Doppler config (`dev` =
demo, `prd` = production) — see [`secrets-doppler.md`](secrets-doppler.md); the env-file fallback convention is in
[`third-party-integrations.md`](third-party-integrations.md). This page is the DocuSign specifics.

The signature seam lives in [`web/src/signature.rs`](../web/src/signature.rs) (the `SignatureProvider` trait + the
DocuSign and stub impls), JWT-grant auth in [`web/src/docusign_auth.rs`](../web/src/docusign_auth.rs), the embedded
signing route in [`web/src/esign_view.rs`](../web/src/esign_view.rs), and the completion webhook in
[`web/src/esignature_webhook.rs`](../web/src/esignature_webhook.rs). An unconfigured vendor falls back to the in-process
stub, so a fresh checkout boots and self-tests without a DocuSign account.

## One app, two environments

Neon Law Navigator uses **one DocuSign app** — integration key `43daaa90` ("Neon Law") — for development, testing,
**and** production. The separate "Neon Law Sandbox" app was retired on 2026-06-04. DocuSign's own model makes this safe:
at Go-Live the integration key is *copied*, not moved —

> When you Go-Live, your integration key is copied to the production environment, rather than moved. This means you can
  continue using the same integration key in the demo environment and in production. Configuration settings such as
  secrets and redirect URIs are not copied automatically. After promotion, you must configure these values separately in
  the production environment.

— [Docusign Go-Live](https://developers.docusign.com/platform/go-live/)

So **only the integration-key GUID crosses environments — everything else is per-environment and is not copied**; you
configure each separately:

| Shared across environments | Per-environment (configured separately; not copied by Go-Live) |
| --- | --- |
| the `43daaa90` integration-key GUID | account id, REST base, OAuth host, RSA keypair, consent, Connect HMAC key |

We confirmed empirically that the RSA keypair does not carry over: pointing `dev` at `43daaa90` with the old sandbox-app
keypair returned `no_valid_keys_or_signatures`. The OAuth host also differs — `account-d.docusign.com` for demo,
`account.docusign.com` for production.

### Testing never burns the production envelope quota

The environment is selected by **which account, base, and OAuth host the credentials point at — not by the app.** So
`cargo test`, CI, and the `#[ignore]` live test read **Doppler `dev`**, which targets the **demo environment**, which
DocuSign documents as a free, isolated sandbox whose envelopes are watermarked and not legally binding, per the
[Developer account](https://developers.docusign.com/platform/account/) page:

> A Docusign developer account (sometimes referred to as a demo account) enables you to develop and test your app in the
  developer environment … which is isolated from the production environment. … a developer account … provides a free
  sandbox environment … any documents sent are purely for testing and are not legally binding.

So a full end-to-end integration test — render, create the envelope, sign it embedded, reach `completed`, download the
executed documents, and receive the completion webhook — runs entirely against that free demo environment and consumes
**none** of the production envelope allowance (~40/month on Starter). The production allowance is touched only by real
production traffic; demo envelopes are non-binding and auto-purge after ~30 days, which is why production stays
separate-by-config rather than sharing the demo environment for real client work.

**The rule that protects this:** `cargo test`, CI, and the live test bind to Doppler `dev` only — never wire `prd`
DocuSign creds into a test or CI path. The production allowance is reserved for real client retainers and any
deliberate, manually-run prod smoke test.

**Accepted trade-off:** the production-live key GUID now also lives in `dev` and on dev machines. Production stays gated
by its own account, RSA keypair, consent, and HMAC key (none shared with demo), so a leaked demo key cannot authenticate
against — let alone send an envelope from — production.

## Which templates are signed (template-agnostic send path)

The send path is keyed off the notation's **template code**, not the retainer. `drive_post_questionnaire_workflow`
([`web::retainer_walk`](../web/src/retainer_walk.rs)) resolves the workflow spec via
`workflows::bundled_spec_yaml(code)`, renders to the generic per-notation storage keys (`notations/{id}/document.pdf`,
`signed-document.pdf`, `certificate-of-completion.pdf`), and resolves the captive signer from the questionnaire answers
when present and otherwise from the notation's bound Person row. Adding a signed template is a template + spec, not a
new handler — the spec just needs the retainer's shape: an `intake_persisted__*` → `staff_review` →
`document_open__*_pdf` → `sent_for_signature__pending` chain.

Signed templates today:

- **`onboarding__retainer`** — the engagement letter; client signs, firm countersigns. **`trusts__nevada`** — the Nevada
  revocable trust instrument; the settlor signs as `client`, the attorney countersigns as `firm`. The trust instrument
  is valid e-signed (NRS 163.008 — no witnesses or notary required), but any deed funding **real property** into the
  trust must be notarized and recorded as a separate step; the template states this caveat and the deed is **not**
  e-signed here.

Deliberately **not** e-signed: `will__simple` (Nevada wills need two attesting witnesses + a notarized self-proving
affidavit, NRS 133.040/133.050, or the NRS 133.085 qualified-custodian path) keeps its in-person `testator_signature` →
`witnesses` → `notarization` flow; `closing__letter` is firm correspondence (firm signature only).

## Authentication: JWT grant

The provider authenticates with **JWT grant** — it signs a short-lived RSA assertion with the firm's integration key and
impersonated user, exchanges it for an access token, and caches that token (re-minting 300 s before expiry). A static
`DOCUSIGN_ACCESS_TOKEN` is kept only as a local/demo fallback. The integration key is the JWT `iss`; the OAuth secret is
*not used* by JWT grant — supplying the secret where the integration key belongs yields `issuer_not_found`.

**Why JWT grant, not Authorization Code / PKCE.** PKCE is an extension of the *interactive* Authorization Code flow: it
needs a human at a browser to log in, uses a one-time `code_verifier`/`code_challenge` (random strings, not the RSA
keys), and still performs a token exchange. For a server that sends envelopes unattended that is strictly worse. JWT
grant is the server-to-server flow built for exactly this case, and it is already minimal: one cached token exchange,
re-minted before expiry, no human in the loop after the one-time consent. There is no DocuSign mode that signs each REST
call with the RSA key directly — every call needs a Bearer token, so the private key's only job is to mint that token.
The integration key + user id + account id are required regardless of grant type: every call is scoped to an account and
user.

Required env (canonical `DOCUSIGN_*` names — same names in `.env` for sandbox and `.env.production` for prod):

- `DOCUSIGN_INTEGRATION_KEY` — the app's Integration Key / OAuth client id (the JWT `iss`). **Not** the OAuth secret.
  `DOCUSIGN_USER_ID` — the impersonated API user GUID (the JWT `sub`); the "API Username", not the email.
  `DOCUSIGN_ACCOUNT_ID` — the API Account ID GUID. `DOCUSIGN_PRIVATE_KEY` — the RSA private-key PEM whose public half is
  registered on the app. `DOCUSIGN_BASE_URL` — the eSignature REST base; `https://demo.docusign.net/restapi` for the
  sandbox. `DOCUSIGN_OAUTH_BASE` — optional OAuth host; defaults to the demo host `https://account-d.docusign.com`.
  `DOCUSIGN_SIGNER_EMAIL` — the firm countersignature inbox and the live test's signer. `DOCUSIGN_HMAC_KEY` — the
  DocuSign Connect HMAC key used to verify completion webhooks (see below).

## Sandbox setup (one-time)

The sandbox/developer account is **permanent and free** — watermarked, non-binding envelopes that consume no allowance.
It is the only environment used for dev, CI, and the `#[ignore]` live test. Navigate (deep-links redirect): sign in at
`https://account-d.docusign.com`, then **Settings (gear) → Integrations → Apps and Keys**. That page shows the User ID,
the API Account ID, the Account Base URI, and your apps with their Integration Keys + RSA keypair management.

1. **Create the app** (`Add App and Integration Key`) → copy the **Integration Key** → `DOCUSIGN_INTEGRATION_KEY`.
2. **Add an RSA keypair** under the app's Authentication. DocuSign shows the private key once — copy it into
   `DOCUSIGN_PRIVATE_KEY` (or register your existing public key so the key already in `.env` matches).
3. Add a **Redirect URI** to the app — needed only to land the one-time consent click. Use a dedicated, app-controlled
   path, `https://www.neonlaw.com/docusign/consent-callback`, kept **distinct from** the OIDC `/auth/callback`
   ([`web::oauth`](../web/src/oauth.rs)): JWT grant never sends an auth code back, so this URI is ceremonial and must
   not collide with the Google-login callback. `web` serves it as a small "Consent recorded" confirmation page (exempt
   from the private-mode gate) so the operator lands on a confirmation rather than a 404.
4. From **My Account Information** copy the **API Account ID** → `DOCUSIGN_ACCOUNT_ID` and the **User ID** GUID →
   `DOCUSIGN_USER_ID`.
5. **Grant one-time consent** — open this in a browser logged into the sandbox and click **Allow** (substitute the
   integration key + a registered redirect):

   ```text
   https://account-d.docusign.com/oauth/auth?response_type=code&scope=signature%20impersonation&client_id=KEY&redirect_uri=REDIRECT
   ```

   JWT grant returns `consent_required` until this is done. Consent is scoped to the **(integration key × impersonated
   user)** pair, so sign in as the *same* user whose GUID is in `DOCUSIGN_USER_ID`. If `consent_required` persists
   *after* a successful Allow, the cause is almost always a user mismatch: one email can have multiple DocuSign
   memberships (e.g. a demo and a production account, or two demo accounts), and consent was recorded for the wrong user
   id. Confirm the **User ID** at the top of Apps and Keys equals `DOCUSIGN_USER_ID`, and that the consent browser is
   logged into the sandbox account that owns the app — not the production account.

## Running the live test (Phase 0 grounding)

The `#[ignore]` test in [`web/tests/docusign_sandbox.rs`](../web/tests/docusign_sandbox.rs) mints a JWT token, creates a
real sandbox envelope with an anchored signature tab, and requests an embedded recipient-view URL. It self-skips when
the env is absent, so it is safe in the default suite.

```bash
set -a && source .env && set +a
cargo test -p web --test docusign_sandbox -- --ignored --nocapture
```

Success prints `created sandbox envelope <id>` and `embedded signing URL: https://…`. Common errors map directly:

- `issuer_not_found` — wrong integration key (likely the OAuth secret). `consent_required` — redo the consent grant.
  `invalid_grant` / `no_valid_keys` — the private key is not the one registered on the app.
  `USER_DOES_NOT_BELONG_TO_ACCOUNT` — the user / account pair does not match.

What the live run still cannot automate (no API auto-signs an envelope): driving the ceremony to `completed`,
downloading the executed documents, and capturing a real Connect completion/decline payload plus the
`X-DocuSign-Signature-1` header. Those steps ground the webhook + HMAC and must be run by hand against the Connect log.

## Client delivery: embedded vs emailed

Each notation carries a `delivery` column (`m20260708_add_delivery_to_notations`) that selects, per matter, how the
client recipient is addressed when the single send path builds the signature manifest. The firm always countersigns
second (`routingOrder` 2) as a non-captive recipient — it receives the usual emailed link — regardless of `delivery`.

- **`embedded`** (the default; the standalone retainer walk) — the client is a **captive** recipient: the manifest sets
  `client_user_id` (derived from the notation), so DocuSign suppresses the signing email and the client signs *inside*
  Neon Law Navigator. `GET /portal/admin/notations/:id/sign` ([`web::esign_view`](../web/src/esign_view.rs)) requests a
  short-lived, single-use recipient-view URL via `SignatureProvider::create_recipient_view` (which POSTs
  `envelopes/{id}/views/recipient`, matching the recipient on the email / userName / clientUserId triple) and iframes
  it. The URL expires in minutes, so the page is rendered fresh per request. The stub returns a deterministic URL in
  dev/KIND. This fits an in-office signing or a logged-in portal session.
- **`emailed`** (the matter-open form) — the client is **non-captive**: the manifest omits `client_user_id`, so DocuSign
  emails the client a signing link they open from their own inbox. This is the right experience for a client whose
  matter an admin opens from the "new project" page (`POST /portal/projects` with "Send retainer for signature"): that
  client is not in the room and has no portal session yet, so a captive embedded recipient would leave them with nothing
  to sign. Same send path, same `send_for_signature` call — only the recipient's captive flag differs.

## Completion webhook + HMAC

DocuSign Connect POSTs to `/webhook/esignature/:secret`. The handler verifies the raw-body HMAC
(`X-DocuSign-Signature-1`) **before** parsing, classifies the event (`completed` → `signature_received`;
`declined`/`voided` → `signature_declined`; everything else → 200 no-op), signals the workflow, and on completion
archives the signed PDF + Certificate of Completion to object storage (best-effort).

> **Production readiness gate.** The prod `DOCUSIGN_HMAC_KEY` is currently a generated placeholder (a boot invariant in
  [`web/src/config.rs`](../web/src/config.rs)). E-sign is safe to run, but **not client-ready** until DocuSign Connect
  on the production account is configured with an HMAC key and the matching value is set in the prod Secret.

## Production cutover (Phase 2)

Production is the **same app**, promoted through Go-Live and pointed at the production account by Doppler `prd`. Go-Live
copies the integration key into production (~1–3 business days) and DocuSign gates approval on a track record of API
usage — so driving sandbox envelopes to `completed` both grounds the flow and earns the promotion. Configuration does
not copy; set up the production environment separately:

1. **Promote + prod auth.** Complete Go-Live, then on `account.docusign.com` add a **production RSA keypair**
   to the same app and **grant consent** for the prod user. The OAuth host becomes `account.docusign.com`; the REST base
   is the account's assigned base from `/oauth/userinfo` (e.g. `https://na4.docusign.net/restapi`).
2. **Prod secrets (Doppler `prd` → Secret Manager → `navigator-web-secrets`).** The identifiers
   (`DOCUSIGN_INTEGRATION_KEY`, `DOCUSIGN_USER_ID`, `DOCUSIGN_ACCOUNT_ID`, `DOCUSIGN_BASE_URL`, `DOCUSIGN_OAUTH_BASE`)
   are staged in `prd`; add the prod `DOCUSIGN_PRIVATE_KEY` and the `DOCUSIGN_HMAC_KEY` (replacing the placeholder). The
   boot invariant rejects the placeholder in prod, so use the `power-push` pre-deploy Secret check.
3. **DocuSign Connect (prod account).** Configure a webhook → `https://www.<domain>/webhook/esignature/<secret>`,
   subscribed to envelope **completed**, **declined**, **voided**, HMAC enabled with the **prod** key — a *different*
   key from demo, since Connect (and its HMAC key) is account-level.
4. **Deploy + verify.** `power-push` (ships both images at HEAD), confirm the boot invariant passes, and round-trip a
   real envelope through the prod webhook.

## Related

- [`third-party-integrations.md`](third-party-integrations.md) — the two-account, env-file convention this follows.
  [`.env.example`](../.env.example) — the canonical per-variable reference (JWT-grant preferred, static fallback).
  `prompts/esignature-e2e-and-production.md` — the build-out kickoff (gitignored; the durable decisions live here).
