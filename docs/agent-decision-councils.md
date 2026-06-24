# Agent decision councils

Navigator uses three lightweight councils as decision protocols. They are not separate products, real subagents, or
marketing personas. They are repeatable review lenses that any LLM or human maintainer can use after reading the real
code, docs, copy, or screen under discussion.

Councils do not replace the GitOps flow in [`agent-workflows.md`](agent-workflows.md). Use them inside one of the two
codebase actions — create a PR, or review/update an existing PR — when the decision needs more than a linear pass.

Use a council when a decision is broad enough that one linear pass is likely to miss a stakeholder, trust boundary, or
long-term maintenance cost. Do not use a council for one-line fixes, formatter passes, simple lookups, or work that is
already decided.

## The three councils

- **Engineering Council** — the people who build Navigator. Use for architecture, refactors, abstractions, and doc
  clarity. The normal form is the full twelve voices.
- **Legal Council** — the counsels who draft legal copy. Use before copy becomes a template, prompt, email, or
  engagement paragraph. Default to Capricorn + Scorpio; use the full twelve for mission-level or unusual practice-area
  copy.
- **Client Council** — the people the firm serves. Use for client-facing product, intake, pricing, portal, and
  onboarding decisions. Default to Libra + Pisces; use the full twelve for mission-level or practice-specific client
  surfaces.

All three follow the same rule: read first, then convene. The voices react to facts, not vibes.

## Engineering Council

The Engineering Council is the build-side council. Use it for architecture decisions, design planning, cross-cutting
refactors, abstraction pressure tests, PR sequencing, and documentation clarity reviews.

Virgo chairs. The chair opens by naming the decision, holds the review to concrete paths and symbols, then closes with
consensus and one next action. The other voices contribute one concrete sentence in zodiac order:

- Aries, incident commander: name the missing or broken thing.
- Taurus, production engineer: make the claim concrete in a file, deploy, or user moment.
- Gemini, API/integration engineer: notice overloaded words, dual contracts, and layer confusion.
- Cancer, new-hire reader: ask what a first-time reader sees and misunderstands.
- Leo, tech lead/devrel: find the memorable line the team can repeat.
- Libra, release manager: weigh scope and sequencing.
- Scorpio, security/trust engineer: pressure-test the load-bearing assumption.
- Sagittarius, product manager: keep the mission and user impact visible.
- Capricorn, staff engineer: guard long-term maintainability.
- Aquarius, platform engineer: surface the broader systems pattern.
- Pisces, original author/migration engineer: preserve what already works.

Output shape: Virgo opens, facts if useful, eleven voices, Virgo closes with consensus, then the concrete action.

## Legal Council

The Legal Council is the drafting-side council: a council of counsels. It shapes the firm's own legal drafting before
copy becomes a Notation template body, questionnaire prompt, engagement letter paragraph, follow-up email, or public
policy statement. It does not give legal advice to a client and does not replace attorney review.

Default to two voices:

- Capricorn, managing partner/senior counsel: institutional memory, ethics opinions, bar-facing commitments, and prior
  incidents.
- Scorpio, ethics and compliance counsel: the fiduciary duty, conflict, UPL, candor, or trust claim everything rests on.

Use the full bench only when the user asks for it, the copy touches an unusual practice area, or the copy defines the
firm's or Foundation's mission. The full bench starts with Capricorn, then Scorpio, then Aries through Pisces:

- Aries, trial attorney: lead with the harm.
- Taurus, business attorney: make the language operative.
- Gemini, appellate attorney: find ambiguity and dual meanings.
- Cancer, legal-aid/tenant-defense attorney: read as the stressed applicant.
- Leo, immigration defense attorney: speak boldly for the right to remain.
- Virgo, tax attorney: demand exact cites, dates, forms, and triggers.
- Libra, mediator/family-law attorney: weigh protection against cost.
- Sagittarius, public-interest/civil-rights attorney: check the access-to-justice mission.
- Aquarius, legal-tech/knowledge-management attorney: find reusable drafting patterns.
- Pisces, estate-planning counselor/mental-health-court lens: honor the human story.

Legal Council output should end with revised copy or a named go/no-go question. Never invent facts. Confirm addresses,
fees, entity facts, bar numbers, dates, and citations against repo sources or the user.

## Client Council

The Client Council is the served-side council. Use it for intake flows, questionnaire ordering, portal UX, pricing copy,
onboarding, error states, referral boundaries, and any decision where the core question is whether a real person walks
in and stays.

Default to two voices:

- Libra, prospective client at the threshold: does this feel worth it, trustworthy, and easier than going elsewhere?
- Pisces, overwhelmed person who almost did not reach out: is the door easy enough for someone with nothing left to
  give?

Use the full bench only when the user asks for it, the decision is mission-level, or a practice-specific client would
otherwise be missed. Libra chairs. The other voices are:

- Aries, tenant facing eviction: speed is survival.
- Taurus, first-time LLC founder: does the product feel solid enough to trust?
- Gemini, bilingual immigrant family: where does one-world wording fail two-world lives?
- Cancer, family caregiver: what asks too much of an exhausted household?
- Leo, wronged client who wants to sue: honor the dignity of a no-litigation referral.
- Virgo, meticulous compliance filer: eliminate vague deadlines, forms, and obligations.
- Scorpio, client with a matter they are ashamed of: guard privacy and avoid shame.
- Sagittarius, dreamer-builder: preserve momentum and horizon.
- Capricorn, elder planning a legacy: keep gravity and long-term meaning.
- Aquarius, collective organizer: fit nonstandard entities and communities.
- Pisces, overwhelmed person: guard the access-to-justice door.

Client Council output should end with the concrete product or copy action, or the user's go/no-go if the council exposes
a real strategic fork.

## Shared guardrails

- A council is a synthesis pattern, not a stall. It should make a decision clearer and end with action.
- Cite real files, routes, screens, symbols, or copy when they exist.
- Keep English as the source language for portal UI, docs, internal artifacts, and legal template bodies. Localized
  questionnaire prompts are allowed only through the attorney-reviewed translation path in [`i18n.md`](i18n.md).
- Respect the role model in [`access-model.md`](access-model.md): every Person has one `persons.role`; project scope is
  separate in `person_project_roles.participation`.
- For telemetry and cloud operations, log identifiers and counts, never client content. Client names, answer bodies,
  email addresses, document bodies, and privileged substance do not belong in spans, metrics, logs, or public docs.

## Website publication

This file is top-level `docs/*.md`, so it is automatically published at `/docs/agent-decision-councils`. Keep council
guidance here so Claude, Codex, Cursor, Gemini, and future LLM agents can all read the same concise protocol.
