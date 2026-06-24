# Neon Law Nautilus — debt-shield design

Nautilus is the firm's $66/month debt-collection correspondence shield. A licensed attorney becomes the consumer's
address of record and answers debt collectors under the consumer's federal rights — by letter, with an attorney signing
every one. It runs on the inbound-email engine and the `@approve` attorney-approval gate that already serve the firm in
production; the [`/services/nautilus`](../web/content/marketing/nautilus.md) product page, route, and nav already ship.

This document is the canonical compliance contract for the offering. The Restate workflow PRs (intake, triage, debt
validation, cease/FCRA dispute, settlement/referral) each cite it rather than re-deriving the scope boundary. Every
statutory claim below is grounded in an official U.S. government source so a future reader can re-verify it as the law
moves.

## The scope boundary (read this first)

Nautilus v1 is a **correspondence shield only**. It is what it does — and just as load-bearing, what it deliberately is
not:

1. **A flat legal fee, never contingent.** The fee is a flat **$66/month** for legal representation in debt-collection
   communications. It is never a percentage of the debt, never contingent on reducing or settling a balance, and never
   sold by outbound telephone solicitation. The number stays $66 whether the balance is $500 or $50,000.
2. **No debt settlement for a cut, no debt management.** Nautilus asserts rights and demands validation; it does not
   renegotiate, settle, or alter the terms of a debt as its business.
3. **No bankruptcy assistance.** No template, questionnaire, or marketing surface advertises or sells bankruptcy help.
   Bankruptcy is a referral, never handled in-workflow.
4. **No litigation.** A collection lawsuit, a summons, or a viable damages claim is litigation — referred to [litigation
   counsel](/services/litigation), never answered as correspondence.

These four hold the product clear of three regimes that would otherwise reach it. Each carve-out is grounded below.

### Why the FTC Telemarketing Sales Rule advance-fee ban does not reach us

The TSR bans collecting any fee for a *debt relief service* before at least one of the consumer's debts has actually
been renegotiated or settled (16 C.F.R. §310.4(a)(5)). It does not reach Nautilus, on two independent grounds:

- **Not a "debt relief service."** A debt relief service is one represented "to renegotiate, settle, or in any way alter
  the terms of payment or other terms of the debt" (16 C.F.R. §310.2(o)). Nautilus answers collectors and asserts
  FDCPA/FCRA rights; it does not alter the terms of any debt, so §310.4(a)(5) does not apply.
- **Not "telemarketing."** Part 310 applies only to "telemarketing" — inducing a purchase via outbound interstate
  telephone calls (16 C.F.R. §310.2(gg)). Nautilus enrolls clients in writing, not by outbound solicitation.

There is no general attorney exemption for inbound calls about debt relief services in 16 C.F.R. §310.6 — the protection
is that Nautilus is neither a debt relief service nor sold by telemarketing, so the ban never attaches.

- 16 C.F.R. §310.4: <https://www.govinfo.gov/content/pkg/CFR-2023-title16-vol1/xml/CFR-2023-title16-vol1-sec310-4.xml>
- 16 C.F.R. §310.2: <https://www.govinfo.gov/content/pkg/CFR-2023-title16-vol1/xml/CFR-2023-title16-vol1-sec310-2.xml>

### Why the bankruptcy "debt relief agency" disclosures are not triggered

The "debt relief agency" label, and the §527/§528 disclosures it carries (including the mandated "We are a debt relief
agency. We help people file for bankruptcy" advertising statement), attach only to a person who provides "bankruptcy
assistance … in return for the payment of money" (11 U.S.C. §101(12A)). *Milavetz, Gallop & Milavetz, P.A. v. United
States*, 559 U.S. 229 (2010), confirms the label reaches attorneys **who provide bankruptcy assistance** — and is
tethered to that assistance. Because Nautilus provides no bankruptcy assistance, it is not a debt relief agency and owes
none of the 11 U.S.C. §§526–528 disclosures. A future bankruptcy-prep tier would take on that label deliberately; it is
out of v1.

- 11 U.S.C. §101: <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title11-section101&num=0&edition=prelim>
- 11 U.S.C. §528: <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title11-section528&num=0&edition=prelim>
- *Milavetz* (U.S. Reports vol. 559): <https://www.govinfo.gov/content/pkg/USREPORTS-559/pdf/USREPORTS-559.pdf>

### Why no debt-adjuster / prorater license is needed in CA / NV / WA

Each state exempts an attorney rendering services in the practice of law from its debt-adjuster licensing scheme. The
exemption is **conditional**, not categorical: it holds only while the work is genuine practice of law in an
attorney-client relationship, with fees flowing to the firm and an attorney owning every matter.

- **California** — Cal. Fin. Code §12100(c) exempts "the services of a person licensed to practice law in this state,
  when the person renders services in the course of his or her practice as an attorney-at-law." (The Rosenthal Act, Cal.
  Civ. Code §1788 et seq., governs collection *conduct* — it is not a license Nautilus must obtain.)
  <https://leginfo.legislature.ca.gov/faces/codes_displaySection.xhtml?lawCode=FIN&sectionNum=12100.>
- **Nevada** — NRS 676A.140 excludes "[l]egal services provided in an attorney-client relationship by an attorney
  licensed … to practice law in this State" from "debt-management services."
  <https://www.leg.state.nv.us/nrs/nrs-676a.html>
- **Washington** — RCW 18.28.010(1)(a) provides that attorneys-at-law "performing services solely incidental to the
  practice of their professions" are not "debt adjusters." <https://app.leg.wa.gov/RCW/default.aspx?cite=18.28.010>

### Unauthorized practice of law

A licensed attorney reviews and signs **every** outbound letter via the `@approve` gate — the staff-reply approval
bridge already live in production. No letter auto-sends. The attorney is load-bearing, per
[`mission.md`](../web/content/marketing/mission.md): the fee buys an actual lawyer in the loop, not software pretending
to be one. Every Nautilus Restate workflow PR reuses this `@approve` gate as its UPL control; none introduces an
auto-send path.

### The engagement letter governs

A written engagement letter signed before representation begins states the exact scope, the $66 monthly fee, any
out-of-scope rate, and how either party may end the representation. The no-contingency rule lives in the engagement
letter itself, not only in marketing. Nautilus guarantees no particular result and does not erase debts the client owes
— it makes sure the client's rights are used and that collectors deal with the client's lawyer. The letter is compliant
across the firm's California, Nevada, and Washington admissions.

## The four core letters and their statutory hooks

Each letter carries role-scoped signature anchors so the **attorney** signs, and each goes out only through the
`@approve` gate.

- **Notice of representation** — FDCPA 15 U.S.C. §1692c(a)(2). Once a collector knows the consumer is represented by an
  attorney whose name and address it can ascertain, it must deal with counsel, not the consumer.
- **Debt validation** — FDCPA 15 U.S.C. §1692g. A written dispute within the 30-day window obliges the collector to
  cease collection until it mails verification of the debt.
- **Cease communication** — FDCPA 15 U.S.C. §1692c(c). A written notice to stop communicating requires the collector to
  cease, subject to three narrow notice exceptions.
- **FCRA credit-report dispute** — FCRA 15 U.S.C. §1681i. The credit reporting agency must conduct a free, reasonable
  reinvestigation within 30 days of receiving the dispute.

Official sources for the operative text:

- 15 U.S.C. §1692c:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1692c&num=0&edition=prelim>
- 15 U.S.C. §1692g:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1692g&num=0&edition=prelim>
- 15 U.S.C. §1681i:
  <https://uscode.house.gov/view.xhtml?req=granuleid:USC-prelim-title15-section1681i&num=0&edition=prelim>

The CFPB's Regulation F (12 C.F.R. part 1006) is the FDCPA implementing rule — §1006.6 restates the
attorney-representation and cease-communication limits, and §1006.34 prescribes the modern validation notice. It is the
layer most likely to drift first, so workflow copy should track it as the regulation, with the statute as the anchor.

## The referral seams

Nautilus refers out the moment a matter leaves correspondence:

- **Collection lawsuit or summons** → [litigation counsel](/services/litigation) (Sethi Legal). Never answered as a
  letter.
- **Viable FDCPA damages claim** → litigation counsel. Asserting a right by letter is in scope; suing on a violation is
  not.
- **Bankruptcy is the right answer** → outside bankruptcy attorney. Nautilus neither advises on nor assists with filing.

## Intake & portal UX contract

The Client Council's findings are requirements for the surfaces that workflows 01–02 build:

- **One-tap forward.** Forwarding the collector envelope is a single action that accepts a phone photo *or* a forwarded
  email — never a demand for a scanned PDF.
- **A sent-letters timeline.** The client sees each letter, the attorney who signed it, the date sent, and the deadline
  being tracked — so protection is visible, not asserted.
- **The trust line is unmissable.** "$66/month flat — we never take a percentage of your debt" appears on intake,
  pricing, and the portal header.
- **Privacy-safe notifications.** Neutral notification subject lines; collector detail lives only behind authentication,
  for the client protecting a debt from their household.
- **Plain language.** "Address of record" carries a one-line plain gloss; the rights are stated plainly, the statute
  numbers stay in this design doc.

## Build sequence

Nautilus engagements are `projects` matters opened by `onboarding__` and closed by `closing__letter`. The workflows ride
the existing `workflows-service` Restate worker — one worker, no per-workflow pod — and the existing inbound-email
engine and `@approve` gate. Build order, each as one PR:

1. **01 — Intake & notice of representation** (`notice_of_representation`, §1692c(a)(2)).
2. **02 — Inbound triage** — classify each inbound collector `.eml` against active matters; the deadline-tracking spine.
3. **03 — Debt validation** (`debt_validation`, §1692g; 30-day timer).
4. **04 — Cease-communication & FCRA dispute** (`cease_communication`, §1692c(c); `fcra_dispute`, §1681i).
5. **05 — Settlement & referral** (`settlement_letter`, client-directed, no cut; lawsuit/summons → litigation referral).

See [`docs/glossary.md`](glossary.md) for the Person / Entity / role vocabulary these workflows use, and the
[`agent-workflows.md`](agent-workflows.md) for the feature-first recipe each PR follows.
