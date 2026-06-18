# billing

The billing-provider seam for the matter lifecycle. When a matter closes — the firm's countersignature on the closing
letter — the firm bills the flat fee. Rather than couple the workflow to one accounting vendor, every caller targets the
small `BillingProvider` trait and the concrete provider is chosen at boot.

This crate holds **no `restate-sdk` dependency** — its heaviest deps are `reqwest` and `tokio`. That is deliberate: see
[Why a separate crate](#why-a-separate-crate) below.

## What it provides

- `BillingProvider` — the async trait every surface depends on: resolve-or-create a contact, raise an invoice.
- `XeroBillingProvider` — production. POSTs an `ACCREC` invoice to the Xero Accounting API and returns the `InvoiceID`
  as an `InvoiceId`. Auth is the client-credentials grant (`xero_auth::XeroClientCredentials`); access tokens are minted
  and cached with a refresh-before-expiry path, mirroring the DocuSign signature provider in `web`.
- `StubBillingProvider` — dev and tests. Records every call to an internal `Mutex` so a test can assert the step fired
  with the right invoice. Selected automatically when `XERO_*` is unset, so a fork boots and self-tests with no Xero
  account (the "one vendor account per environment" convention —
  [`docs/third-party-integrations.md`](../docs/third-party-integrations.md)).

Money is always cents (`i64`), never floats.

## Why a separate crate

The seam lives here, not inside `web`, so two callers on different runtimes can share it:

- `web` depends on `billing` and is **deliberately Restate-free** — it never pulls `restate-sdk` into its build or
  image, and re-exports the seam as `web::billing` / `web::xero_auth` so existing paths keep resolving.
- `billing-workflows` (the worker-side flows hosted by `workflows-service`) depends on **both** `billing` and
  `restate-sdk`.

Merging `billing` into `billing-workflows` would drag `restate-sdk` into `web`'s dependency graph; merging it into `web`
would cut the worker off from the seam. Keeping the trait in its own Restate-free crate is the same dependency-firewall
pattern as `workflows` (outbound) vs `workflows-service` (inbound) and `cloud`'s provider quarantine.

## Getting started

```bash
# Trait + stub round-trip + Xero client-credentials parsing. No Xero account needed.
cargo test -p billing
```

`XeroBillingProvider::from_env()` returns `None` when `XERO_*` is unset, so callers fall back to the stub. The free Xero
demo company is the one $0 path for a live integration test — see
[`docs/third-party-integrations.md`](../docs/third-party-integrations.md).
