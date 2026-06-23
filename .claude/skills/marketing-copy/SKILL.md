---
name: marketing-copy
description: >
  Authoring rules for client-facing marketing copy on neonlaw.com — firm pages under `web/content/marketing/`, the views
  under `views/src/pages/`, and any public surface where the firm advertises services (LinkedIn posts, YouTube content,
  talk titles when speaking *for the firm*). Trigger when adding or editing any of those files, drafting a hero line,
  naming a new service tier, writing a service description, or quoting a fee. Three load-bearing rules: (1) market only
  the firm's own value, never compare to or characterize "other lawyers / traditional firms"; (2) no hyperbole or
  unsubstantiable superlatives; (3) attorney-advertising compliant across the firm's bar admissions (CA, NV, WA) — no
  guarantees of results, no certification claims for SOC 2 / HIPAA, SLAs are service commitments not outcome promises.
  Skip for non-marketing surfaces (internal docs, code comments, ADRs) and for already-binding documents (signed
  retainers, filed pleadings) — those are reviewed by the attorney of record, not by this skill.
---

# Marketing copy on neonlaw.com

Every public-facing word the firm publishes is **lawyer advertising** under each bar's professional-responsibility rules
(RPC 7.1 family across CA, NV, WA — and the same line applies to LinkedIn posts, YouTube video titles and descriptions,
talk abstracts, and email signatures used in firm communications). This skill captures the three load-bearing rules for
that copy, plus the practice-specific patterns the firm has settled on (transparent flat fees,
readiness-not-certification for compliance work, SLAs framed as service commitments).

Pair this skill with [`markdown-lint`](../markdown-lint/SKILL.md) for the mechanical checks. This skill governs the
*content*; the lint command governs the *form*.

## The three rules

### 1. Market only the firm's own value — never compare to other lawyers

The subject of every sentence is **us, our clients, or the work** — never "other lawyers," "most firms," "traditional
attorneys," or any class of lawyer that isn't us. When tempted to write a contrast, write the affirmative instead.

- ❌ "Most lawyers charge several thousand dollars for a transactional matter."
- ❌ "Unlike traditional firms that bill by the hour, we…"
- ❌ "Other attorneys won't tell you their prices upfront."
- ✅ "Every engagement is quoted as a flat fee before we start."
- ✅ "Each retainer tier publishes a fixed monthly amount and a published turnaround commitment."

**Why:** two compounding reasons. (1) Brand — the firm wins by being clearly *itself* (transparent pricing, technical
fluency, access-to-justice mission). The reader does the comparison for free in their head; we never need to draft it.
(2) Ethics — the substantiable-comparison rule under RPC 7.1 across CA / NV / WA means any factual claim about "most
lawyers" must be defensible with a citation, which we cannot produce for casual marketing prose. Removing the comparison
removes the rule problem in one stroke.

The rule **does not** block factual claims about the access-to-justice gap itself (LSC Justice Gap Report stats on
`foundation.md` are about the people without help, not about lawyers as a competitor class). It blocks comparisons to
the legal profession as a class.

See [[feedback-no-comparative-marketing]] in user memory for the canonical statement.

### 2. No hyperbole, no superlatives, no unsubstantiable claims

The voice is **plain and precise**. We don't write copy the reader has to discount; we write copy the reader can verify.

- ❌ "The fastest contract review in the industry."
- ❌ "World-class compliance counsel."
- ❌ "Cutting-edge AI-powered legal services."
- ❌ "Best-in-class fractional GC."
- ❌ "Industry-leading SOC 2 expertise."
- ✅ "48-hour SLA on standard contract reviews at the Growth tier."
- ✅ "A licensed attorney who has shipped Rust to production reads your contracts."
- ✅ "Every retainer fee, SLA, and overage rate is published on this page."

The test: if the claim is not a number, a verifiable credential, a published commitment, or a description of work we
actually do, cut it. "Fast" without a number is hyperbole; "48-hour SLA" is a fact. "World-class" is a vibe; "five years
operating petabyte-scale data systems" is a credential.

This also rules out marketing-flavored adjectives that have become near-meaningless in the legal industry: *premier,
elite, leading, top-tier, expert, specialist* (the last two also carry specific bar-rule constraints in some
jurisdictions). When in doubt, drop the adjective.

### 3. Attorney-advertising compliant

The firm advertises across **California, Nevada, and Washington** (and any LinkedIn / YouTube content under the firm's
name reaches all three audiences). The rules below are the common denominator across those bars — copy that satisfies
them ships safely in all three.

**No guarantees of results, ever.** Anything that implies a specific legal outcome is forbidden under RPC 7.1 in every
jurisdiction we practice in.

- ❌ "We'll get you SOC 2 certified."
- ❌ "Guaranteed approval of your LLC formation."
- ❌ "Win your case — or your money back."
- ❌ "We'll close your Series A in 30 days."
- ✅ "We prepare your environment for the SOC 2 audit and quarterback the independent CPA firm that issues the
  attestation."
- ✅ "Formation filings submitted within five business days of intake."
- ✅ "48-hour SLA on standard contract reviews — a service commitment about our turnaround, not a promise about
  counterparty response or deal outcome."

**Compliance vocabulary is frozen.** For SOC 2 / HIPAA and other third-party attestation regimes, the firm is the
**readiness counsel** sitting beside the auditor — never the attester. The independence rules forbid the same firm from
advising and attesting.

| Allowed | Forbidden |
| --- | --- |
| Readiness | Certification |
| Advisory | Compliant |
| Prepare your environment for the audit | Get you SOC 2 |
| Quarterback the CPA auditor | We attest |
| HIPAA readiness | HIPAA certified |

Grep for the forbidden words on every PR that touches marketing copy. One slip is an unauthorized-attestation problem.

**SLAs are service commitments, not outcome promises.** Same principle: the firm controls its turnaround; it does not
control whether deals close, counterparties respond, or matters resolve favorably. Always write SLAs in terms of the
firm's work product, never in terms of the client's result.

**Bar admissions are facts, disclosed honestly.** Every firm-branded page footer carries the bar admissions (California
No. 337252, Nevada No. 13400, Washington), each hyperlinked to the issuing bar's public directory so anyone can verify.
Don't claim admissions the firm does not hold; don't hide the admissions the firm does hold. The bar-admissions strip
lives in `views/src/layout.rs`.

## Verify every fact with the user — never guess, never copy from a sibling page

Copy carries **concrete facts** that have exactly one correct value: the firm's and Foundation's street address and
suite number, each entity's state of incorporation, entity type, bar admission numbers, email addresses, and every
published fee. Get one wrong and it propagates — a stale "Washington 501(c)(3)" or a transposed suite number reads as
authoritative on a lawyer-advertising page.

The rule: **before you write or edit any copy, list every fact the copy will assert and confirm each one.** Ask the user
*all* of those questions up front — in one batch, for the whole piece — rather than drafting around the gaps or carrying
a value over from a neighboring page. Do not infer a fact from a sibling file; sibling files are where inconsistencies
hide.

Source-of-truth order:

1. **The seed data and templates** — `store/seeds/Address.yaml` (firm and Foundation addresses, suites, jurisdictions),
   `notation_templates/nonprofit/` (the Foundation is a **Nevada** 501(c)(3)), and the bar-admission strip in
   `views/src/layout.rs` (CA No. 337252, NV No. 13400, WA).
2. **The user** — for anything not pinned in the repo, or any value that looks even slightly off. Ask; don't assume.

Canonical facts as of this writing (confirm, don't trust blindly — they can change):

- **Neon Law (the firm)** — 5150 Mae Anne Ave Ste 405-9002, Reno, NV 89523.
- **Neon Law Foundation** — 5150 Mae Anne Ave Ste 405-9999, Reno, NV 89523; a **Nevada** 501(c)(3).
- **5150 Mae Anne Ave** is the **Ridgeview Mail Center** — a private-mailbox / packing-and-shipping center that is also
  the firm's Reno coworking partner. Describe it as a Reno business address, private mailbox, and coworking desk (as
  `corporate.md` does). Confirm any new on-site claims (offices, conference rooms) with the user before publishing.

When a fact cannot be confirmed against the repo or the user, name the gap explicitly in the draft and stop — the same
way the Legal Council surfaces a go/no-go it cannot answer. Do not invent the answer.

## House conventions that follow from the rules

- **Transparent flat fees, no ranges.** Every published price is a fixed number (`$1,000 LLC formation`, `$3,500/month
  Seed retainer`), never a range (`$3,500–$5,000`). Ranges read as evasive; fixed amounts are the brand.
- **Every firm fee is a multiple of $500.** Our own legal fees ladder in clean $500 steps — `$500`, `$1,000`, `$1,500`,
  `$2,500`, `$3,500`, and up. Third-party fees that pass through (state filing fees, court filing fees, the Ridgeview
  mailbox at `$300/year`, third-party registered-agent vendor fees) keep their actual amount and are labelled as
  pass-through. The $500 rounding discipline is the firm's; we do not round a third party's fee to fit. **Why:**
  legibility. A clean $500 ladder reads as a deliberate scale; arbitrary numbers read as bespoke quoting in disguise.
- **Terse sentences. No throat-clearing.** Cut auxiliaries when the active verb already carries the meaning; replace
  "never with X" with "with no X" (same meaning, sharper voice); drop "before we start" when the cadence already makes
  the timing clear.
  - ❌ "Every estate engagement is quoted at a fixed flat fee before we start. You get the document, the funding
    instructions, and the filings — never an hourly bill that grows while you read it."
  - ✅ "One fixed fee covers the document, the funding instructions, and the filings. No hourly bill — only the flat
    fee, quoted before we start."
  - ❌ "Government fees are pass-through at cost — we publish what the Secretary of State charges and collect it with
    the engagement fee, never with a markup."
  - ✅ "Government fees pass through at cost — what the Secretary of State charges, we collect with no markup."
- **Government fees are labelled and pass-through.** Every fee table on `corporate.md`, `estate.md`, and
  `fractional-gc.md` has a "Government fees" column making clear what's included (legal fee only) vs. what's
  pass-through at cost (state filing fees, court filing fees, recorder fees). Never mark up government fees.
- **Engagement letter governs.** Every page with a fee table closes with an "Engagement letter governs" section stating
  that scope, final fee, out-of-scope rate, and disengagement terms are confirmed in a written engagement letter signed
  before work begins. The published fees are a starting point, not a binding offer.
- **Access-to-justice cross-subsidy stays explicit.** The fractional-GC retainer funds the access-to-justice tier
  (`$500` wills, `$500` trusts, `$500` LLCs). When the flagship page and the access-to-justice services live on the same
  site, the connection between them must be stated — once on the flagship page, once on the mission page — or the
  mission frame reads as bait-and-switch under market-rate pricing.
- **Practice-area honesty.** Litigation is not a practice area; we say so and refer out. That's a strategic choice, not
  an apology — written affirmatively, not as a disclaimer.

## Language — marketing is the one localized page

English is the official language ([`CLAUDE.md`](../../../CLAUDE.md#human-language-english-first)); marketing pages are
the **only** fully-localized *pages* in the app. The `/es` Tier-A pages and the mission letter may be **transcreated**
(not literally translated — keep the bold-rights-fighter cadence) to reach clients in their own language, and each ships
only after a licensed attorney reviews it in-language; the architecture and review tiers live in
[`docs/i18n.md`](../../../docs/i18n.md). When localizing, carry facts and proper nouns verbatim (addresses, bar numbers,
fees, trademark names) — those are never translated in any locale.

## Where the rules live

- **Trigger files.** This skill should fire when authoring or editing any of:
  - `web/content/marketing/*.md` — the marketing markdown
  - `views/src/pages/*.rs` — view modules with hero copy / fallback copy
  - `views/src/brand.rs`, `views/src/layout.rs` — brand strings, footer text, navigation labels
  - `web/content/marketing/mission.md` — the canonical mission rendered at `/foundation/mission`
- **Reader register.** Address the reader as a bold rights-fighter — see [[feedback-brand-voice-bold-reader]] in user
  memory. Never label the reader "scared," "frightened," "vulnerable," or "in crisis." Empathy is a drafting lens, not a
  label.
- **Companion memory.** [[feedback-no-comparative-marketing]] captures Rule 1 as a persistent user preference.

## Before committing marketing copy

1. **Read the diff once for hyperbole.** Grep for *most lawyers, traditional, modern, premier, elite, leading, top,
   best, world-class, cutting-edge, industry-leading, fastest, cheapest*. Each hit is suspect.
2. **Read the diff once for results promises.** Grep for *guarantee, certified, compliant, will [verb], get you, close
   your, win, succeed, ensure your*. Each hit is suspect.
3. **Read the diff once for ranges in prices.** Grep for `$[0-9,]+\s*[-–]\s*\$`. Replace with a single fixed amount.
4. **Check every firm fee is divisible by $500.** Grep the diff for every `$` amount and confirm each one mod 500
   equals 0 — except third-party pass-throughs (Ridgeview `$300`, SoS filing fees, court fees), which keep their actual
   amount and are labelled as such.
5. **Read the diff once for absolutist words.** Grep for *never, always, guaranteed, ensured*. Replace with the
   affirmative ("with no markup", not "never with a markup"). Tighten any sentence longer than ~25 words.
6. **Run [`markdown-lint`](../markdown-lint/SKILL.md).** It must exit 0.
7. **Confirm every concrete fact.** Re-read the diff for any address, suite number, state of incorporation, entity
   type, bar number, email, or fee. Each must trace to `store/seeds/Address.yaml`, a template, the bar strip, or an
   explicit answer from the user — never carried over from a sibling page on faith.
8. **If the copy describes a new service or a service-tier change**, add or update the test in `web/tests/routes.rs`
   that asserts the route returns the page with the expected title.

## Mini-example — a hypothetical hero rewrite

> *Draft:* "Neon Law is your premier fractional general counsel — the fastest, most affordable, world-class legal team
> in the Pacific Northwest. Other firms charge thousands; we won't. Get SOC 2 certified in 30 days, guaranteed."

That paragraph trips every rule: comparison ("other firms"), four superlatives ("premier / fastest / most affordable /
world-class"), an unsubstantiable comparison ("charge thousands"), an attestation claim ("get SOC 2 certified"), an
outcome promise ("in 30 days, guaranteed"), and a geographic scope claim ("Pacific Northwest") the firm has not
validated.

> *Rewrite:* "Fractional general counsel for software and AI startups. A licensed attorney admitted in California,
> Nevada, and Washington reviews your contracts and prepares your environment for SOC 2 and HIPAA audits. Every retainer
> fee, SLA, and overage rate is published — see the [Fractional GC](/services/fractional-gc) page."

Every claim in the rewrite is a fact, a credential, or a published commitment. That's the bar.
