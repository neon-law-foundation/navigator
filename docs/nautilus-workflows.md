# Neon Law Nautilus — correspondence workflows (build index)

Nautilus is the firm's $66/month debt-collection shield. The product page, `/services/nautilus` route, nav, marketing
tests, and the compliance contract at [`nautilus-design.md`](nautilus-design.md) have shipped. This doc is the
engineering build index for the five Restate-durable workflows that actually run it: collector mail comes to the firm
and goes back out as attorney-signed letters under the client's FDCPA / FCRA rights.

Read the compliance contract first — it is the source of truth for the scope boundary and the statutory hooks. This file
is the source of truth for *how the workflows are wired*. They are complementary: the design doc says what is allowed,
this index says how it is built.

## Shared context (applies to every numbered workflow)

- **Email engine (live in prod).** `parse.neonlaw.com` MX → SendGrid Inbound Parse → `/webhook/sendgrid/inbound` →
  `.eml` in GCS, then the `web` threading + relay path; outbound goes back through the same relay. The staff-reply
  `@approve` command is the attorney-approval gate — reuse it, never reinvent it.
- **One worker.** Every workflow binds onto the existing `workflows-service` Restate endpoint — one worker, never a
  per-workflow pod. This is idiomatic Restate: many handlers, one deployment.
- **Recipe.** Follow the `create-legal-workflow` skill — (1) `.feature` first, (2) template + questionnaire, (3) seeded
  questions, (4) workflow YAML from the shared step library, (5) Restate handlers. Use only Person / Entity / role nouns
  from [`glossary.md`](glossary.md).
- **Matter lifecycle.** A Nautilus engagement is a `projects` matter opened by `onboarding__` and closed by
  `closing__letter` when the representation ends.

## Guardrails (every outbound letter, every workflow)

These restate the compliance contract so a workflow PR cannot drift from it:

- A licensed attorney reviews and signs **every** outbound letter via the `@approve` gate — modeled in the spec as a
  `staff_review` state. No letter auto-sends (no UPL).
- The fee is a flat **$66/month** — never a percentage of debt, never contingent on settling a balance.
- **No template, questionnaire, or copy advertises bankruptcy assistance** — that would trip the 11 U.S.C. §528
  debt-relief-agency disclosures. Bankruptcy is a referral, never handled in-workflow.
- A collection lawsuit, a summons, or a viable FDCPA damages claim is **litigation** → refer to litigation counsel
  (Sethi Legal), never answered as correspondence.

## The shared step chain

Every Nautilus letter is the same three-state spine drawn from the shared step library in
[`workflows::step`](../workflows/src/step.rs):

1. `document_open__<letter>` — the runtime renders the letter template into a PDF blob and persists it via
   `cloud::StorageService`. No human in the loop yet.
2. `staff_review` — the attorney reads the rendered letter and approves or rejects it. This state **is** the `@approve`
   gate; it is the unauthorized-practice-of-law control.
3. `email_send__<letter>` (or `mailroom_send__<letter>` for physical mail) — the runtime delivers the approved letter
   through the relay; the worker advances only on a 2xx.

The gate is enforced in code, not prose: `staff_review_gates_filing` in
[`workflows::guardrail`](../workflows/src/guardrail.rs) proves no path from a `document_open__*` fill state reaches a
submission state without passing a `staff_review` state in between. Every Nautilus workflow spec inherits that
invariant, so an auto-send path fails the test rather than reaching a client.

## The shared template library

All five letters carry role-scoped signature anchors so the **attorney** signs, and each rides the step chain above.
Each lands under `notation_templates/nautilus/` with a paired `workflows/specs/<code>.yaml` registered in
`workflows::specs::BUNDLED_SPEC_YAML` and pinned by `workflows/tests/spec_coherence.rs`:

- `notice_of_representation` — FDCPA 15 U.S.C. §1692c(a)(2) — built in workflow 01.
- `debt_validation` — FDCPA 15 U.S.C. §1692g — built in workflow 03.
- `cease_communication` — FDCPA 15 U.S.C. §1692c(c) — built in workflow 04.
- `fcra_dispute` — FCRA 15 U.S.C. §1681i — built in workflow 04.
- `settlement_letter` — client-directed, no cut — built in workflow 05.

The FDCPA writing requirements make the render-before-send chain load-bearing: a §1692c(c) cease notice and a §1692g
dispute must be **in writing**, and a §1681i reinvestigation is triggered by a written dispute. Rendering the letter to
a durable blob before delivery is what puts each right in writing.

- 15 U.S.C. §1692c:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1692c&num=0&edition=prelim>
- 15 U.S.C. §1692g:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1692g&num=0&edition=prelim>
- 15 U.S.C. §1681i:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1681i&num=0&edition=prelim>

## Build sequence

Build each workflow as one PR, in order — each declares its dependencies:

1. **01 — Intake & notice of representation.** Onboard the client, sign the engagement letter, set the $66/mo billing,
   collect the creditor list, and fan out `notice_of_representation` to every known collector. Depends on nothing.
2. **02 — Inbound triage.** Classify each inbound collector `.eml` against active matters and route it; the
   deadline-tracking spine. Depends on workflow 01 and the email engine.
3. **03 — Debt validation.** The §1692g validation request, the 30-day timer, and classifying the verification. Depends
   on workflow 02.
4. **04 — Cease-communication & FCRA dispute.** The §1692c(c) cease letter and the §1681i credit dispute. Depends on
   workflow 02.
5. **05 — Settlement & referral.** Client-directed settlement correspondence (no cut) and the lawsuit/summons →
   litigation-counsel referral branch. Depends on workflow 02.

The client-facing UX contract (one-tap forward, a visible sent-letters timeline with the signing attorney and tracked
deadlines, the unmissable flat-fee trust line) lives in [`nautilus-design.md`](nautilus-design.md) and is built by
workflows 01–02.

Each PR ends with the standard pre-commit gate — `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, plus markdown lint for any `.md`. Tests land in the same commit as the implementation.
