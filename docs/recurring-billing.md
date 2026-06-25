# Recurring subscription billing

Neon Law Navigator bills two kinds of money through the same `billing::BillingProvider` Xero seam:

- **Matter-close flat fees** — raised once, when the firm signs a matter's closing letter (Northstar, Nest). See the
  `MatterCloseInvoice` workflow in `billing-workflows`.
- **Recurring subscriptions** — raised every month, for products billed on a schedule (Nexus at $2,222/mo, Nautilus at
  $66/mo). This document covers that path: the `RecurringBilling` workflow.

The `products` table is the single source of truth for price. A product with `billing_kind: recurring` is a
subscription; the workflow drives entirely off that flag, so making a product recurring is the only thing needed to
start billing it monthly — there is no hard-coded product list.

## The model

- `products` (`store::entity::product`) — the catalog. `list_price_cents`, `cadence`, `billing_kind`, `currency`,
  `xero_item_code`, and `account_code` (the Xero chart-of-accounts code the revenue posts to) all live on the row, so
  the invoice draws price *and* account from one place.
- `subscriptions` (`store::entity::subscription`, migration `m20260715`) — one active recurring engagement: who is
  billed (`contact_name` / `contact_email`, with soft `person_id` / `entity_id` / `project_id` links), the
  `product_code`, the `status` (`active` | `paused` | `cancelled`), `started_at`, and the durable idempotency ledger
  `last_invoiced_period` (`YYYY-MM`, UTC). A discount mirrors `billing::LineDiscount`'s two shapes via
  `discount_percent` / `discount_amount_cents` (at most one set; both null bills at list).

## The workflow

`RecurringBilling` (`billing-workflows::recurring`) is the schedule-driven sibling of `MatterCloseInvoice`. Each run,
for one billing period (`YYYY-MM`, the current UTC month):

1. load the `recurring` products (`store::products::recurring`) — the workflow's only product list;
2. select the due `active` subscriptions (`store::subscriptions::due_for_period`) — those whose `last_invoiced_period`
   is behind the period (never billed, or billed for an earlier month);
3. for each, build the invoice from the product row (price + account code), `ensure_contact`, then `create_invoice`;
4. on success, advance `last_invoiced_period` to the period — so a re-run never re-bills it.

It bills the full period — no proration. A per-subscription provider error is recorded in the diagnostic email and the
subscription stays due (retried next run); a DB error aborts the run and Restate retries the whole step.

The workflow is hosted by the `workflows-service` worker (one endpoint for every workflow — no per-workflow pod), fired
by the `recurring-trigger` `CronJob` (`k8s/overlays/kind/recurring-trigger/`,
`examples/deploy/k8s/exports/cron-recurring-trigger.yaml`). The CronJob runs daily; the monthly period guard makes the
daily cadence safe (at most one invoice per subscription per month) and picks up a mid-month subscription on the next
run. It is surfaced on `/portal/admin/schedules`.

## Idempotency — two layers, both required

1. **Durable (ours).** `last_invoiced_period` advances only *after* `create_invoice` returns Ok. A subscription already
   billed for the period is simply not re-selected next run. This is the real defense: it holds across a re-run days
   later.
2. **Boundary (Xero).** The `create_invoice` `matter_id` parameter is sent as Xero's `Idempotency-Key`. We derive a
   stable UUIDv5 from `(subscription_id, period)` under a fixed workspace namespace, so a double-POST in the same period
   dedupes at Xero; a new period yields a new key and bills again. Xero's idempotency window is only hours, so this
   guards a concurrent double-send, while the durable layer guards a later re-run — belt and suspenders, not redundant.

The one residual risk is a crash *between* Xero's 200 and the local `mark_invoiced` write: the Xero key protects a
re-run within hours, but not days later. Persisting the raised invoice id locally before returning (as the matter-close
path does into `xero_invoices`) would close it; that is a reasonable follow-up, tracked separately.

## Why we generate our own invoices (not Xero's repeating invoices)

Xero's `/RepeatingInvoices` endpoint can own the recurrence so Xero generates each month's invoice from a template. We
keep generation in our own scheduler instead — the engineering council reviewed this and was unanimous:

- **One schedule-owner.** With a Xero-owned template, both our `subscriptions.status` and Xero's template state believe
  they own the schedule; pause/cancel from the portal becomes advisory (you would have to round-trip Xero to actually
  stop a charge). Owning generation keeps one writer.
- **The billing source of truth stays in our DB.** Per-engagement discounts and pause/cancel apply locally without
  round-tripping Xero templates, and the on-call answer to "why was this billed?" is one query, not a Xero UI.
- **Xero stays swappable.** Recurrence lives behind the `BillingProvider` seam, so swapping Xero for another accounting
  vendor is one new `impl BillingProvider`, not a re-implementation of the schedule.
- **We reuse what already works.** `create_invoice`'s `Idempotency-Key` header and the `StubBillingProvider` tests were
  built for matter-close replays; recurring reuses them unchanged.

**Out of scope (deliberately).** Failed-charge dunning (retrying a declined/unpaid invoice, notifying the client) is not
built in either option. Owning generation is what keeps that door open; a Xero-owned template would wall it off. It is a
separate follow-up. Client-facing pause/cancel self-service is also deferred — an admin sets `status`; the workflow only
ever bills `active` rows.
