---
name: council
description: >
  Twelve-perspective architectural review pattern ("The Council of Twelve"), each voice fusing a zodiac stance with a
  practitioner identity — Virgo the engineering manager chairs the council (opens with the framing, closes with
  consensus + action), Aries the incident commander names the gap, Scorpio the security/trust engineer cuts to the core,
  Aquarius the network/platform engineer surfaces the systems pattern, Cancer the new-hire applies beginner's mindset,
  Capricorn the graybeard guards long-term maintainability, Sagittarius the PM keeps the big picture, and so on. Trigger
  for architecture decisions, design planning, cross-cutting refactors, abstraction evaluation, doc-clarity reviews, and
  PR-sequencing calls ("one bundle or three?"). Skip for one-line fixes, simple lookups, mechanical refactors, and
  anything already decided. Render inline as Virgo's opening → eleven voices (Aries → Pisces) → Virgo's close (consensus
  + action) — not as twelve real subagents.
---

# The Council of Twelve

A multi-perspective review pattern. Twelve voices, each anchored to a
zodiac archetype with a stable role, weigh in on a load-bearing
decision; the council synthesizes their inputs into a consensus and one
concrete action. The point is **breadth of framing**, not depth of
investigation — twelve angles in the time it would otherwise take to
write one.

## When to invoke

- **Architecture decisions** — introducing a new crate, picking an
  abstraction, choosing between two designs.
- **Multi-piece feature planning** — work that spans 3+ files where a
  single linear pass would miss cross-cuts.
- **Cross-cutting refactors** — renames, migrations, sequencing
  decisions ("one PR or several?").
- **Doc-clarity reviews** — glossaries, READMEs, API docs — anywhere
  multiple audiences read the same words.
- **Abstraction pressure tests** — "Is this the right shape? Are we
  naming it correctly? What breaks in two years?"
- **Load-bearing claim audits** — when one assumption is doing a lot
  of work, run the council to find what it rests on.

## When NOT to invoke

- One-line fixes, lint corrections, formatter passes.
- Simple lookups ("where is X defined?", "what does Y do?").
- Tactical bug fixes where the root cause is already known.
- Anything already decided where the work is just typing.
- Mechanical refactors with a clear scope and no design choices.

The failure mode is running the council on trivia — it dilutes the
signal. If the answer would have been the same with one voice, don't
summon twelve.

## The twelve voices

Stable across invocations. Do not re-roll personas; the value compounds
when the cast stays the same. Each voice fuses **a zodiac stance** (how
to think) with **a practitioner identity** (the role whose day job
already embodies that stance). **Virgo chairs the council** — opens
with the framing and closes with consensus + action. The other eleven
contribute **one short, concrete sentence** in zodiac order Aries →
Pisces (Virgo's slot is skipped in the loop, since they bookend).

### Chair

- **Virgo** ♍ — *Engineering Manager — chair the council.* Open with
  one sentence naming the decision on the table. Run the room: hold
  every voice to precision — exact symbol names, exact line numbers,
  exact asymmetries; strike imprecise verbs ("filled-in copy" →
  "binding"); if a claim names a thing, the thing must exist at that
  path. Close with 3–5 synthesized consensus bullets and one concrete
  next step. The chair owns the framing and the close; don't
  disappear into a single peer voice in between.

### The eleven

- **Aries** ♈ — *Incident Commander — name the gap.* What is the most
  important thing missing, broken, or unstated? Don't bury the action
  behind context. Speak like an SRE on the bridge: name the fire so
  the chair can scope the response.
- **Taurus** ♉ — *Production Engineer — make it concrete.* Reject pure
  abstraction. Demand the file path, the line of code, the deploy
  log, the user moment. If you can't point to it in prod, it isn't
  real yet.
- **Gemini** ♊ — *API / Integration Engineer — notice the duality.*
  Same word, two meanings. Same shape, two layers. Same endpoint, two
  callers with different contracts. Where is something secretly
  overloaded?
- **Cancer** ♋ — *New Hire / Applicant-Reader — empathy for the
  reader.* Who actually shows up to this — a lawyer, a paralegal, an
  applicant, an engineer on day one, someone debugging at 2 a.m.?
  What do they see first? What confuses them? Ask the dumb question
  on purpose.
- **Leo** ♌ — *Tech Lead / DevRel — find the memorable line.* The
  one-sentence cadence the team will quote back ("The Template
  declares; Restate runs."). If you can't compress it, you don't
  understand it yet.
- **Libra** ♎ — *Release Manager — weigh the scope.* One PR or three?
  Doc-only or doc+code? What is the smallest change that preserves
  the load-bearing property? Mediate between "do everything now" and
  "ship the smallest useful piece."
- **Scorpio** ♏ — *Security / Trust & Safety Engineer — cut to the
  core.* What is the one claim everything else rests on? What hidden
  assumption silently breaks the design — or the trust model — if
  it's wrong? Find the load-bearing belief and pressure-test it.
- **Sagittarius** ♐ — *Product Manager — big picture.* Why does this
  matter beyond the immediate task? Who is it for, what changes for
  them, how does it tie back to the project's mission? Don't lose the
  larger arc to local optimization.
- **Capricorn** ♑ — *Graybeard / Staff Engineer — long-term
  maintainability.* What happens in two years? Will future readers
  understand this? Will the abstraction hold when the team has
  tripled? Favor convention over cleverness; remember what burned us
  last time.
- **Aquarius** ♒ — *Network / Platform Engineer — systems pattern.*
  Where else does this shape appear — in the codebase, in the
  cluster, on the wire? Can the abstraction be reused? Is the new
  code a special case of something already general? Watch the
  topology, not just the call site.
- **Pisces** ♓ — *Original Author / Migration Engineer — honor what
  works.* The current code is not broken because the new design is
  better. New layers add; they rarely need to replace. Be kind to
  the past — somebody shipped it for a reason.

A voice may pass — explicitly. "Pisces: nothing to add; the current
path already honors what works" is fine. Filling all eleven with empty
flavor text is worse than passing.

## Output shape

1. **Virgo opens** — one sentence as the chair: what is being
   reviewed and what decision is on the table. The chair owns the
   framing; don't split it into a standalone section.
2. **Findings** (optional) — short bullet list of *what is actually
   true in the code* the council is reviewing. Cite paths, symbols,
   line numbers. The council reacts to facts, not vibes.
3. **The eleven voices** — one line per voice, ordered Aries →
   Pisces (Virgo's slot is skipped, since they're chairing). Each
   must say something concrete about *this* matter — no generic
   philosophy.
4. **Virgo closes — consensus** — 3–5 synthesized bullets: the
   decisions the council agrees on, the trade-offs it surfaced, the
   asymmetries to call out. This is where the chair re-enters and
   weaves the eleven contributions into a single position.
5. **Action** — the concrete next step. Which files to edit. Which
   questions to ask the user. What to build now vs. defer. If the
   council surfaced a gap that requires the user's go/no-go, name
   that explicitly.

## Execution

- **Default: render inline as a synthesis.** A single response carries
  findings, twelve voices, consensus, and action. This is parallel
  **framing**, not parallel investigation — the synthesis happens in
  one head, which is fine and faster.
- **Don't spawn twelve real subagents** unless the user explicitly
  asks for it. Twelve subagents on one short artifact would be slow,
  expensive, and stochastic; the cost would dwarf the marginal
  insight.
- **Cite the codebase concretely.** Each voice should ground its
  claim in a file path, a line number, a real symbol when possible.
  "We should be clearer" is the failure mode; "the Notation entry
  overloads the word — promote the format-vs-row note to a callout"
  is the success mode.
- **Read first, council second.** Run the file reads and greps the
  council will need *before* convening, so each voice has something
  real to react to. A council without facts produces philosophy.

## Mini-example

A condensed real session (the [glossary `Template`/`Notation` clarity
review](../../../docs/glossary.md)):

> **Virgo (chair) opens.** Decision on the table: does the glossary
> make the Template (static blueprint) vs. Notation (running
> instance) distinction clear, and what's the smallest fix?
>
> **Findings.** Template entry leads with "the blueprint that produces
> a Notation" — true but buried. Notation entry overloads the word
> (row type + markdown format) in the same paragraph.
>
> **Voices.**
>
> - Aries: Lead with the verb. *Template declares; Notation runs.* Bury nothing.
> - Gemini: The dual meaning of "notation" is currently a footnote. Promote to a callout.
> - Scorpio: Steal the user's phrase: *"come to life."* That's the line.
> - …
>
> **Virgo (chair) closes — consensus.** Rewrite Template to lead with
> "static blueprint." Rewrite Notation to lead with "Template come to
> life," strike "filled-in copy" (a Notation is a *binding*, not a
> copy), promote the overloading note to a blockquote callout.
> Sharpen Questionnaire with the same pattern.
>
> **Action.** Three `Edit` calls against `docs/glossary.md`, lint
> with `cli validate --markdown-only --no-default-excludes`, report
> back.

That session produced four concrete edits and zero throat-clearing.

## Failure modes to avoid

- **Twelve generic statements.** If every voice says some variation of
  "we should be clear and well-organized," the council added nothing.
  Each voice's archetype must show through, or the format is
  decoration.
- **Council-as-stalling.** Don't summon the council to *avoid* making
  a decision. The council exists to *make* the decision, not defer
  it. End with concrete action.
- **Council-on-trivia.** Running the council on a rename or a one-line
  fix wastes the format. Save it for moments that earn twelve
  perspectives.
- **Council-without-reading.** Convening before the code has been read
  produces philosophical voices instead of grounded ones. Read first.
