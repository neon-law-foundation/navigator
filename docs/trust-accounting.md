# Client trust accounting — the IOLTA ledger and its deferred banking provider

How Neon Law Navigator tracks client funds it holds in trust: the money a client pays **in advance** of the work, held
until it is earned, and refunded if the engagement ends early. This is the accounting layer behind the firm-standard
retainer billing model — see [`xero-billing.md`](xero-billing.md) for the invoice-out side.

The whole module lives in `store/src/trust.rs`.

## Why this exists, and why it is not the bank

A matter's fee terms are agreed per engagement and stated in the retainer's clauses, so a retainer may collect an
advance against them. Under NRPC 1.5 and California Rule 1.15, an advance fee is **not earned on receipt**: it is held
in the client trust account, earned as the work is performed, and any unearned remainder is **refunded** when the matter
closes early. That earned/unearned split and the refund are a bar-rules obligation — no bank, payment processor, or
accounting package computes them for us. So the firm-specific logic lives in `store::trust` as pure, provider- and
asset-agnostic Rust.

Navigator **does not custody funds** — the invariant from [`xero-billing.md`](xero-billing.md) holds. A real banking /
trust-ledger provider (Modern Treasury, Increase, Column) will own USD settlement and the bank-statement leg of
reconciliation; on-chain rails will own crypto settlement. This module owns only the **legal-meaning overlay**: which
engagement a movement belongs to, what it means, and the running trust position per matter. The provider is **not chosen
yet**, so we commit to no financial schema — see [Deferred banking provider](#deferred-banking-provider) below.

## What a trust ledger has to do (IOLTA)

Trust accounting is regulated bookkeeping. A compliant setup keeps three balances that must always agree — the **monthly
three-way reconciliation**:

1. **The bank statement** — what the bank says is in the pooled trust account.
2. **The trust master ledger** — the firm's own book balance for that account.
3. **The per-matter balances** — how much of the pooled money belongs to each client/matter, summing to the master.

Navigator supplies balances 2 and 3: every engagement has its own **individual client ledger** (`trust:<project>`), and
the master is their sum. Balance 1 — the bank statement — is what the deferred banking provider supplies later. Because
Navigator holds no funds, it cannot *break* reconciliation; it exists to make balances 2 and 3 auditable at any time.

## The model: immutable double-entry postings

Each trust movement is a **double-entry posting** — value flows from one account to another — recorded as one immutable
event. There are three kinds:

| Kind | Flow | When |
| --- | --- | --- |
| `deposit` | client → `trust:<project>` | an advance collected at signing (or a later top-up) |
| `earned_draw` | `trust:<project>` → `operating` | fee earned as work is performed, drawn to the firm |
| `refund` | `trust:<project>` → client | unearned balance returned at early close |

The accounts are `client:<project>` (the client's own funds), `trust:<project>` (this matter's individual client
ledger), and `operating` (the firm's earned revenue). The trust balance for a matter is everything that flowed **into**
`trust:<project>` minus everything that flowed **out**. Because earned money is drawn out, whatever remains in trust is
by definition still **unearned** — so `held == unearned == refund-on-close`, and `store::trust::Position` exposes all
three off the same fold.

### Proration

Where the fee runs monthly, the first partial month due at signing is `monthly_fee × (days_remaining ÷ days_in_month)`,
where `days_remaining` equals `days_in_month − day_of_month` — the balance of the signing month **after** the signing
date. Any signing on **July 8** prorates to `(31 − 8) / 31` of the fee, collected at signing; a signing on the last day
of the month prorates to `0`, so the first full month bills at the next period boundary. The fee may **flex by phase**:
a litigation matter stepping from pleadings to discovery to trial prorates the **new** phase's monthly fee the same way
from its effective date, tagged with `Movement::phase`. `prorate_first_month` rounds to the nearest cent (half up) and
is exercised against a table of signing-date cases — month lengths, both February variants, and the boundaries.

### Storage — an append-only journal, no new table

A trust movement is recorded as an immutable JSON event on the existing `notation_events` journal under the
`trust_ledger` machine-kind, anchored to the engagement's **retainer notation**. The notation already carries the
`project_id` / `person_id` / `entity_id` links. That journal is hardened by a `BEFORE UPDATE OR DELETE` trigger that
raises on any mutation, so trust postings are **tamper-evident for free**: once written, a row can be neither edited nor
deleted, and a correction is a new reversing posting. Reusing the journal means the ledger adds **no schema** of its own
— nothing to migrate when the model changes.

## Assets: USD and crypto

A movement records the **asset actually moved** (`asset` + `amount`) as a decimal *string* — never a float, and never
assuming USD cents (a wei-denominated ETH amount overflows `i64`). The **fee obligation is denominated in USD**, so the
accounting math runs on the common denominator `usd_value_cents` — the USD value credited at receipt — while `asset` /
`amount` / `rail` / `external_ref` preserve the in-kind record for audit.

The legal frame differs by asset. Pooled **USD** held for clients is the classic **IOLTA** concern. Client **crypto** is
not IOLTA at all — it is **safekeeping of client property** under RPC 1.15's property branch, tracked in kind like a
held stock certificate rather than swept into a pooled interest-bearing account. The schema does not bake in "cents in a
USD bank account," so both fit the same postings.

## Deferred banking provider

The provider — and whether a client pays in USD, USDC, or BTC — is deliberately **not chosen yet**. The seam is
`Movement::external_ref` (a bank posting id or an on-chain tx hash) plus the `rail` token (`manual` today; `bank` /
`crypto` later). When a provider is wired, its settlement reference mirrors into `external_ref` and these immutable
postings replay onto the provider's ledger — with no migration to unwind, because the movements were never coupled to a
provider-specific schema. Until then, movements are recorded `manual`, and the firm reconciles the ledger against the
real trust bank account by hand.
