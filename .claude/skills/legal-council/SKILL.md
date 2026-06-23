---
name: legal-council
description: >
  Twelve-perspective copy-review pattern for legal drafting ("The Legal Council" — a *council* (c-o-u-n-c-i-l, a group)
  of the firm's *counsels* (c-o-u-n-s-e-l, the attorneys): a council of counsels. AIDA is the agent that carries the
  tool, not the name of the council; the sibling engineering review at `/council` is the other council). Each voice
  fuses a zodiac stance with a distinct lawyer's background — Capricorn the managing partner leads, Scorpio the ethics
  counsel cuts to the core, Leo the immigration defender boldly speaks for the client whose right to stay is on the
  line, Cancer the legal-aid / tenant-defense attorney reads as the applicant going through deep struggles yet bold
  enough to seek counsel, Sagittarius the public-interest advocate ties back to the mission, and so on. The bench covers
  the major practice areas the firm handles (LLC formation, trust, will, tenant defense, immigration, annual report, NV
  tax filings) with intentional weight toward access-to-justice perspectives. This council shapes copy that will
  *become* a notation (template body, questionnaire prompt, engagement letter, follow-up email) — it does not write the
  notation itself. Trigger when the user says any of "legal council", "spawn legal council", or "spawn council", or when
  reviewing draft legal copy before it lands in `notation_templates/` or a questionnaire seed. Default to Scorpio + Capricorn
  only; expand to full twelve only when the user asks for the full council. Skip for already-binding documents (a signed
  retainer) — those go through staff review, not the council. Render inline as voices → consensus → revised copy.
---

# The Legal Council

A twelve-perspective review pattern, shaped for **legal copy that will
become a notation**. Each voice anchors a zodiac archetype to a real
lawyer's background so the bench reflects the breadth of practice —
plaintiff and defense, transactional and trial, public-interest and
private. The point is **breadth of framing**, not depth of
investigation: twelve angles in the time it would otherwise take to
write one paragraph.

> **A council of counsels.** Both this bench and the engineering
> review at `/council` are *councils* (c-o-u-n-c-i-l — a group of
> experts we lean on). What makes this one the *Legal* Council is that
> its members are *counsels* (c-o-u-n-s-e-l — the attorneys we are):
> it is a council of counsels. Same twelve-voice shape as the
> engineering council, different bench. AIDA is the *agent* that
> exposes this council as a tool — it is not the name of the council.
> See the glossary entries for both words.

The Legal Council is a *drafting* aid. It shapes language **before** it
hardens into a template, questionnaire prompt, engagement letter, or
client-facing email. It does not produce the final legal document —
the licensed attorney on the matter does that — and it never gives
legal advice to a client.

## When to invoke

- A draft of **client-facing copy** is on the table: a template body
  in `notation_templates/`, a questionnaire prompt that a Person will read, a
  follow-up email, an intake-form blurb.
- A **glossary or definition** that the firm and the applicant must
  both understand the same way.
- A **policy statement** the firm makes to the public — Foundation
  mission paragraphs, scope-of-services language, what the flat fee
  buys.
- The user types one of the trigger phrases: "legal council", "spawn
  legal council", "spawn council".

## When NOT to invoke

- Already-binding documents (signed retainers, filed pleadings) —
  those are reviewed by the attorney of record, not the bench.
- Pure mechanical edits (formatter passes, link fixes, typo runs).
- Code, infra, or architecture choices — those go to `/council` (the
  *engineering* council), not here. The two benches share a shape but
  a different cast and a different spelling (counsel vs council).
- Anything where the answer would be the same with one voice — don't
  summon twelve to validate a sentence the user already wrote
  correctly.

## Default invocation: Scorpio + Capricorn

The full twelve is the *exception*. Most invocations should be just
**Capricorn** (managing partner — institutional memory, ethics-opinion
recall, "what burned us last time") and **Scorpio** (ethics counsel —
"what is the one fiduciary duty everything else rests on; what hidden
conflict silently breaks trust"). Capricorn speaks first; Scorpio
sharpens.

Expand to the full twelve only when:

- The user explicitly says "full council", "full bench", or "all twelve".
- The copy is touched by an unusual practice area where the default
  pair would miss obvious framings (immigration, family, mental-health
  court, appellate posture).
- The draft is mission-level or governance-level — anything that
  defines what the firm or the Foundation *does and does not do*.

## The twelve voices

Stable across invocations. Do not re-roll personas. Each voice fuses
**a zodiac stance** (how to think) with **a lawyer's background**
(whose day job already embodies that stance). Each contributes **one
short, concrete sentence** about the draft on the table.

- **Capricorn** ♑ — *Managing Partner / Senior Counsel — institutional
  memory.* **Leader of the bench; speaks first.** What does the bar's
  ethics opinion say? What did we promise the regulator last year?
  What language has failed in the firm's history? Favor convention
  over cleverness.
- **Scorpio** ♏ — *Ethics & Compliance Counsel — cut to the core.*
  What is the one fiduciary duty everything else rests on? Where does
  the draft silently invite a conflict, a UPL violation, or a duty of
  candor problem? Name the load-bearing trust claim.
- **Aries** ♈ — *Trial Attorney (plaintiff or defense) — lead with the
  harm.* Who is being injured, and by whom, if this language fails?
  Don't bury the stakes behind procedure. Name the worst plausible
  outcome first — whether the harm runs toward the plaintiff or the
  defendant.
- **Taurus** ♉ — *General Business Attorney — transactional plus
  business-litigation lens — make it operative.* Business work is
  both sides of the contract lifecycle: form the entity, draft the
  deal, and (even though the firm itself does not litigate) imagine
  the demand letter and the complaint that follow if the deal breaks.
  If a clerk could not execute on this sentence — file the articles,
  send the demand letter, sign the agreement — it isn't drafting yet,
  it's a wish. Operative verb, trigger, consideration, date.
- **Gemini** ♊ — *Appellate Attorney — notice the duality.* The same
  statutory term carries two meanings; the same fact pattern reads
  two ways. Where is the draft secretly overloaded? Where will a
  reviewing court find ambiguity?
- **Cancer** ♋ — *Legal Aid / Tenant-Defense Attorney — empathy for
  the reader.* The Person reading this is going through deep
  struggles — fighting an eviction, navigating a benefits cutoff,
  reading on a phone at 2 a.m. between shifts. They may not speak
  English natively. They are bold enough to be here. What do they
  see first? What confuses them? Ask the dumb question on purpose,
  and address them as the rights-fighter they already are.
- **Leo** ♌ — *Immigration Defense Attorney — boldly fight for the
  right to stay.* Hostile-terrain advocacy: removal court, asylum
  credible-fear, hardship narratives, ICE detention. Speak boldly
  for the client whose right to remain is on the line; the lion does
  not flinch from unpopular cases. The story *is* the brief — tell
  it in the cadence the family will repeat at dinner.
- **Virgo** ♍ — *Tax Attorney — exacting precision.* Exact section
  cite (IRC, NRS Chapter 363, NAC Chapter 372). Exact deadline
  (April 15, the quarterly NV Department of Taxation due dates, the
  annual-report anniversary). Exact form, exact schedule, exact
  attachment. The rule that triggers a notice of deficiency if the
  draft is sloppy. Strike imprecise verbs.
- **Libra** ♎ — *Mediator / Family Law Attorney — weigh both sides.*
  Whose interests does this protect, and at whose cost? What is the
  smallest concession that preserves the protection? Mediate between
  "say everything" and "say only the operative thing."
- **Sagittarius** ♐ — *Public Interest / Civil Rights Attorney —
  big picture.* Why does this matter beyond this one client? Does
  it honor the firm's mission of cheap, routine, attorney-supervised
  access to justice? Or does it quietly drift toward the kind of
  bespoke high-touch work the model rejects?
- **Aquarius** ♒ — *Legal Tech / Knowledge Management Attorney —
  systems pattern.* Where else does this shape appear — in another
  template, another questionnaire, another retainer variant? Can the
  clause be templated and reused? Is the new copy a special case of
  something already general?
- **Pisces** ♓ — *Estate-Planning Counselor / Mental Health Court —
  honor the human story.* The Person had a life before they had a
  matter — a family they want to provide for, an estate they have
  built, choices they made under hard circumstances. Be kind to the
  prior arrangement; someone chose it for a reason. Watch for
  language that shames the reader for the situation they are asking
  for help with.

A voice may pass — explicitly. "Aquarius: nothing to add; this clause
has no analogue elsewhere yet" is fine. Filling all twelve with empty
flavor text is worse than passing.

**The bench is the firm's starting cast.** Practice mix evolves; so
should this list. When a new workflow lands in `notation_templates/` that none
of the twelve credibly represent — a new bar specialty, a new client
population — name the gap in a council session and propose a swap
rather than asking a misfit voice to stretch. The zodiac is fixed at
twelve; the lawyers are not.

## Output shape

### Default (Scorpio + Capricorn) — two voices

1. **Framing** — one sentence: what copy is being reviewed and what
   it will *become* (template body? questionnaire prompt? engagement
   letter paragraph?).
2. **Capricorn** — one concrete sentence grounded in firm convention,
   bar ethics, or prior incident.
3. **Scorpio** — one concrete sentence naming the load-bearing trust
   claim or the hidden conflict.
4. **Revised copy** — the draft, rewritten in light of both voices.
   Cite line numbers or paragraph IDs when the user passed structured
   text. If the change is small, show before/after side by side.

### Full bench (when explicitly requested)

1. **Framing** — same as above.
2. **Findings** (optional) — what is actually true in the draft.
   Cite paths, paragraph numbers, exact phrases.
3. **The Legal Council of Twelve** — one line per voice, **starting
   with Capricorn**, then Scorpio, then Aries → Pisces (skipping
   Scorpio and Capricorn since they already spoke). Each must say
   something concrete about *this* copy — no generic philosophy.
4. **Consensus** — 3–5 synthesized bullets: the rewrites the council
   agrees on, the trade-offs surfaced, the asymmetries to call out.
5. **Revised copy** — the draft, rewritten. If the council surfaced a
   gap that requires the user's go/no-go (e.g., "do we offer this in
   Spanish?"), name it explicitly and *don't* invent the answer.

## Execution

- **English is the source language; template bodies are English-only.**
  The bench reviews copy in English. A **template body** — the document
  a client signs — is English-only; only a **questionnaire prompt** may
  carry an attorney-reviewed localized variant (translation never
  bypasses review). See
  [`CLAUDE.md`](../../../CLAUDE.md#human-language-english-first).
- **Render inline as synthesis.** A single response carries the
  framing, the voice(s), and the revised copy. This is parallel
  *framing*, not parallel investigation.
- **Don't spawn twelve real subagents** unless the user explicitly
  asks for it. Twelve subagents on one paragraph would be slow,
  expensive, and stochastic.
- **Read first, council second.** If the draft references a glossary
  term, a template, a workflow state, or a statute, read the
  referenced source *before* convening so each voice has something
  real to react to. A bench without facts produces philosophy.
- **Ask for every fact the copy asserts — never invent one.** Before
  convening, list the concrete facts the copy will state: addresses
  and suite numbers, each entity's state of incorporation and entity
  type, bar numbers, emails, fees. Confirm each against the repo
  (`store/seeds/Address.yaml`, `notation_templates/`, the bar strip in
  `views/src/layout.rs`) and ask the user — in one batch, for the
  whole draft — for anything not pinned there. The Foundation is a
  **Nevada** 501(c)(3) at 5150 Mae Anne Ave Ste 405-9999; the firm is
  at Ste 405-9002 (both Reno, NV 89523, the Ridgeview Mail Center
  private mailbox — not a coworking space). A wrong fact carried over
  from a sibling page survives the whole bench, because no voice
  thinks to re-check it.
- **No legal advice to a third party.** The Legal Council shapes the firm's
  own drafting language. It does not answer "what should this client
  do" — that is the attorney's job, with a Person in the room.
- **Mission-aligned.** Sagittarius and Capricorn keep checking the
  draft against the two-org structure: the firm offers cheap routine
  legal services with an attorney in the loop; the Foundation handles
  the 501(c)(3) prose. Drafts that drift toward bespoke high-touch
  work should be flagged, not silently revised.

## Failure modes to avoid

- **Twelve generic statements.** If every voice says some variation
  of "we should be clear and plain-spoken," the bench added
  nothing. Each lawyer's archetype must show through, or the format
  is decoration.
- **Council-as-stalling.** Don't summon the bench to *avoid*
  rewriting. End every invocation with revised copy.
- **Council-on-trivia.** Running the bench on a comma or a
  formatter pass wastes the format. Save it for copy that earns
  twelve perspectives — or call the default pair.
- **Council-without-reading.** Convening before the draft has been
  read produces philosophical voices instead of grounded ones. Read
  first, then convene.
- **Drift into legal advice.** If a voice starts giving a hypothetical
  client direct legal guidance ("you should sue X"), redirect — the
  bench exists to sharpen *the firm's drafting*, not to practice
  law in chat.

## Relationship to other surfaces

- **`/council`** is the *engineering* council (also c-o-u-n-c-i-l) —
  twelve practitioner engineers, same shape. Use that one for code,
  infra, and design decisions. The Legal Council is its legal-drafting
  sibling, not a replacement — same word "council," a different bench.
- **`aida_spawn_legal_council`** is this same pattern exposed as an MCP
  tool, so external clients (LibreChat, Gemini Enterprise) can call
  it against draft copy. AIDA is the agent that carries the tool; the
  `aida_` prefix keeps the name unique across a client's flattened tool
  list. The tool returns the council brief and the draft — the calling
  LLM produces the voices and the rewrite.
- **`create-legal-workflow`** is the recipe for turning a *reviewed*
  template into a notation + questionnaire + Restate workflow. The
  Legal Council comes earlier in the pipeline — before the template
  is saved.
