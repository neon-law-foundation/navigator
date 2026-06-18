# Xero billing — setup, invoice flow, and production cutover

How Navigator raises the matter-close flat fee as a Xero invoice, how that invoice's paid-status is reconciled back into
the portal, and how **one custom connection per organisation** keeps test invoices off the live ledger. Secrets are
selected per environment by Doppler config (`dev` = demo company, `prd` = live organisation) — see
[`secrets-doppler.md`](secrets-doppler.md); the env-file fallback convention is in
[`third-party-integrations.md`](third-party-integrations.md). This page is the Xero specifics.

The billing seam lives in [`billing/src/lib.rs`](../billing/src/lib.rs) (the `BillingProvider` trait + the
`XeroBillingProvider` and `StubBillingProvider` impls), client-credentials auth in
[`billing/src/xero_auth.rs`](../billing/src/xero_auth.rs), and the worker-side nightly reconcile in
[`billing-workflows`](../billing-workflows/). The crate is re-exported as `web::billing` so the app and tests share one
trait. An unconfigured vendor falls back to the in-process stub, so a fresh checkout boots and self-tests without a Xero
account.

## One connection per organisation (the contrast with DocuSign)

DocuSign promotes **one app** across environments — at Go-Live the integration key is *copied*, so demo and production
share a GUID. **Xero is the opposite.** A Xero *custom connection* is a machine-to-machine app bound to exactly **one
organisation**, so each environment needs its **own** connection:

| Environment | Connection | Organisation | Ledger weight | Cost |
| --- | --- | --- | --- | --- |
| dev / CI | sandbox custom connection | the free **demo company** | none — resets periodically | free |
| production | live custom connection | the firm's real **organisation** | real receivables | $5/mo USD |

Both use the same `XERO_*` variable names — the env *file* is the namespace (`.env` = sandbox, `.env.production` =
live), so no code branches on environment. The connected org is fixed per connection and sent as the `Xero-Tenant-Id`
header on every Accounting API call.

### Testing never touches the live ledger

The demo company is the one free, non-binding target: invoices and contacts created there carry no receivable weight and
are self-cleaning (the demo org resets periodically). That is why dev, CI, and the live grounding test all point at the
demo company — a leaked sandbox secret cannot raise a real invoice against a real client.

## What gets invoiced (the matter-close flat fee)

When the firm closes a matter, `web` raises the matter's flat fee as a Xero **`ACCREC`** (accounts-receivable) invoice
through the pluggable `billing_provider` seam — the `Arc<dyn billing::BillingProvider>` field on the app state in
[`web::lib`](../web/src/lib.rs). Two calls happen in order:

1. **`ensure_contact`** — find-or-create the client as a Xero contact, keyed on the unique contact name, returning a
   stable `ContactID`. Re-running never duplicates the contact.
2. **`create_invoice`** — post an authorised `ACCREC` invoice with the fee as a decimal line amount, the
   `Xero-Tenant-Id` header, and an idempotency key so a retry never double-bills.

The invoice id and paid-status are mirrored into the `xero_invoices` table, which backs the per-project invoice card in
the portal. Navigator raises invoices and mirrors their status; it **never holds client funds, card data, or bank
credentials** — Xero reconciles against the firm's bank (Mercury) itself. The integration boundary is the Xero
Accounting API and nothing beyond it.

## Where the price comes from (the product catalog)

A product's list price has one source of truth: the **`products`** table, seeded from
[`store/seeds/Product.yaml`](../store/seeds/Product.yaml) and read through `store::products`. Before the catalog, a
price was duplicated across the marketing frontmatter, a hand-written `flat_fee_cents()` `match`, and the Xero invoice —
changing Nexus $5,000 → $2,222 touched ~10 places. Now `web::retainer_walk::flat_fee_cents` resolves the matter-close
fee from the catalog, so the advertised price and the invoiced price cannot drift.

Each row is keyed by a stable product `code` (`northstar`, `nest`, `nexus`, `nautilus`, `litigation`) — the marketing/
Xero identity, **not** a template prefix. The billing trigger is a separate, explicit column,
`matter_close_template_code`: the originating onboarding template whose matter-close raises the flat fee
(`onboarding__estate` for Northstar — the marketed name and the template that opens the matter deliberately diverge). A
fee is raised only for products whose `billing_kind` is `matter_close_flat`; Nautilus (`recurring`) and 1337 (`hourly`)
carry a list price for reference but raise no matter-close fee.

### Discounts: list price is data, a discount is an event

The catalog holds exactly **one** list price per product. An admin-discretion discount is a separate recorded event,
never a second price. Navigator is the system of record for the *decision* — the columns `discount_pct` /
`discount_amount_cents`, `discount_reason`, `discount_approved_by`, `discount_approved_at` on the originating notation
(`store::notations::record_discount`) are the audit trail. Xero does the client-facing math: the discount rides the
invoice line as `DiscountRate` (a percentage) or `DiscountAmount` (a currency amount), so the client sees list −
discount.

A discount only ever goes **down** from list (RPC 7.1 — billing below an advertised flat fee is truthful; above it is
misleading). The below-only guardrail is enforced in code at raise time
(`billing::MatterCloseInvoiceRequest::validate_discount`): a percent over 100, or a flat amount larger than the line's
gross, is rejected before any Xero call. The local `xero_invoices` mirror records the **net** amount, matching what the
client is billed.

## Authentication: client-credentials grant (preferred)

A custom connection authenticates with the OAuth 2.0 **client-credentials** grant — no user, no redirect, no consent
ceremony. [`XeroClientCredentials`](../billing/src/xero_auth.rs) mints a short-lived Accounting API token and refreshes
it itself, so there is no 30-minute token to rotate by hand. Set the client-credentials pair to activate it:

- `XERO_CLIENT_ID`, `XERO_CLIENT_SECRET` — the custom connection's credentials (the secret is shown **once** at
  creation).
- `XERO_TENANT_ID` — the connected org's GUID (`Xero-Tenant-Id` header). Optional for the live test, which can
  auto-discover it from the `/connections` endpoint since a custom connection binds to one org.
- `XERO_SCOPE` — optional; defaults to `accounting.contacts accounting.invoices`.

A static `XERO_ACCESS_TOKEN` is accepted as a fallback for a quick local smoke test, but Xero expires it in ~30 minutes
and it is ignored when the client-credentials pair is set. The real provider activates when `XERO_TENANT_ID` is present
together with **either** the client-credentials pair **or** a static access token; otherwise `web` uses the stub.

## Sandbox setup (one-time) — sign up and create the custom connection

1. **Create a Xero developer account.** Sign up free at [developer.xero.com](https://developer.xero.com/) and sign in to
   **My Apps**.
2. **Have a demo company.** From your Xero account, enable the **Demo Company** (My Xero → "Try the demo company"). It
   is free, pre-populated, and resets periodically — the right target for dev and CI.
3. **Create a custom connection.** In My Apps → **New app** → choose **Custom connection** (the machine-to-machine,
   client-credentials app type). Name it (e.g. `Navigator (demo)`), and add the **integrator** email that will authorise
   it.
4. **Select scopes.** Grant exactly `accounting.contacts` and `accounting.invoices`. A custom connection offers only
   granular scopes — the legacy parent `accounting.transactions` is **not** offered, and requesting it fails token
   minting with `invalid_scope`.
5. **Authorise the connection against the demo company.** The integrator opens the authorisation link Xero generates and
   connects it to the **demo company** org. This binds the connection to that one organisation.
6. **Copy the credentials.** From the connection's Configuration, copy the **Client ID** → `XERO_CLIENT_ID` and generate
   the **Client Secret** (shown once) → `XERO_CLIENT_SECRET`. Set `XERO_TENANT_ID` to the demo org's GUID (or leave it
   unset locally and let the live test discover it).

For production, repeat steps 3–6 with a **separate** custom connection authorised against the firm's **live**
organisation (a paid $5/mo single-org app), and put those credentials in `.env.production` / the `prd` Doppler config.

## Running the live test (grounding)

The live test in [`web/tests/xero_sandbox.rs`](../web/tests/xero_sandbox.rs) mints a real client-credentials token
against the demo-company connection and drives `ensure_contact` twice with the same unique name — the first call
**creates** the contact, the second must **find** it and return the *same* `ContactID`. This is the only test that
catches a regression in our understanding of Xero's API (a wrong `where` predicate, a bad scope, a rejected payload). It
self-skips green when no creds are present and runs only under the explicit `NAVIGATOR_RUN_LIVE_SANDBOX=1` opt-in, so it
never fires on an ambient-credentials `cargo test`:

```bash
NAVIGATOR_RUN_LIVE_SANDBOX=1 doppler run --project navigator --config dev -- \
  cargo test -p web --test xero_sandbox -- --nocapture
```

It reads the CI `XERO_SANDBOX_*` names first, each falling back to the canonical `XERO_*` name, so a local `source .env`
(or `doppler run`) drives it without separate sandbox vars.

## Paid-status reconciliation

Raising the invoice is only half the loop — the firm gets paid in Xero, and that status has to come back. The nightly
`ReconcileInvoices` workflow (worker-side, in [`billing-workflows`](../billing-workflows/)) calls `get_invoice` for each
mirrored invoice and folds Xero's paid-status into the `xero_invoices` table, so the per-project invoice card in the
portal flips to **Paid** without anyone re-keying it. Like every workflow, it is hosted by `workflows-service` — no
per-workflow worker pod.

## Production cutover

1. Create the **live** custom connection (separate from the demo one) authorised against the firm's real organisation.
2. Put its `XERO_CLIENT_ID` / `XERO_CLIENT_SECRET` / `XERO_TENANT_ID` in the `prd` Doppler config (rendered into the
   `navigator-web-secrets` Secret), never in source.
3. Confirm the live org grants the same `accounting.contacts accounting.invoices` scopes.
4. Verify with one real matter close that the `ACCREC` invoice appears in the live org and the portal card mirrors it.

## Related

- [`third-party-integrations.md`](third-party-integrations.md) — the per-environment vendor-account convention and the
  full integration catalog.
- [`docusign-esignature.md`](docusign-esignature.md) — the sibling e-signature integration (one app, two environments).
- [`secrets-doppler.md`](secrets-doppler.md) — how `dev` / `prd` secrets are selected and rendered.
