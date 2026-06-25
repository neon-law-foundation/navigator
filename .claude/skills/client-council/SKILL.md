---
name: client-council
description: >
  Twelve-perspective review pattern for the people the firm serves ("The Client Council") — the demand-side sibling of
  the engineering `/council` (the people who *build* Neon Law Navigator) and the `legal-council` (the counsels who *draft* it).
  Each voice fuses a zodiac stance with a real client walking in the door across the firm's practice areas — Libra the
  prospective client at the threshold chairs (weighs whether to hire a lawyer at all, then closes on "does this make me
  walk in and stay?"), Aries the tenant facing eviction names the fire, Pisces the overwhelmed person who almost didn't
  reach out guards access-to-justice, Capricorn the elder planning their legacy thinks in decades, Leo the wronged
  client who wants to sue tests the firm's no-litigation boundary, and so on. Use it when building Neon Law Navigator — intake
  flows, questionnaire prompts, pricing copy, portal UX, onboarding — to pressure-test "does this actually serve the
  human who shows up?" Trigger when the user says "client council", "customer council", "spawn client council", or when
  reviewing a client-facing product or copy decision. Default to Libra + Pisces; expand to the full twelve only when
  asked. Skip for internal-only surfaces and anything already decided. Render inline — voices → consensus → action —
  not as twelve real subagents.
---

# The Client Council

A twelve-perspective review pattern, shaped for **the people the firm serves**. Where the engineering `/council` is
twelve practitioners who *build* Neon Law Navigator and the `legal-council` is twelve counsels who *draft* its legal
copy, this bench is the **demand side**: twelve kinds of human who walk in the door, each anchoring a zodiac archetype
to a real client with a real matter. The point is **breadth of lived experience**, not depth of investigation — twelve
doorways into "would this actually help the person who shows up?" in the time it takes to guess once.

> **Three councils, one shape.** All three are *councils* (c-o-u-n-c-i-l — a group we lean on) with the same
  twelve-voice zodiac structure. They differ by *whose chair it is*: `/council` seats engineers (build side),
  `legal-council` seats the firm's counsels (draft side), and the Client Council seats the firm's **clients** (the
  served side). When building Neon Law Navigator, the strongest decisions survive all three: it is buildable, it is
  ethically sound to draft, and a real client is better off for it.

The Client Council is a *product and copy* aid. It pressure-tests a decision **before** it ships to a client-facing
surface — an intake flow, a questionnaire prompt, a pricing page, a portal screen, an onboarding email. It does not
write the final artifact, and it never puts words in a real client's mouth as if they were consenting — each voice is a
*representative archetype*, a way to ask "who would this fail, and how?"

## When to invoke

- A **client-facing product decision** is on the table: a new intake
  flow, a portal screen, a questionnaire ordering, an onboarding step, a self-serve vs. attorney-assisted split.
- **Client-facing copy** that will shape behavior: a pricing page, a
  "what the flat fee buys" explainer, a scope-of-services blurb, an error or empty state a stressed client will read.
- A **conversion or access question**: "will a prospective client cross
  the threshold here?" "can the most overwhelmed person get through this flow?" "what makes someone abandon at this
  step?"
- The user types a trigger phrase: "client council", "customer council",
  "spawn client council".

## When NOT to invoke

- Internal-only surfaces (admin tools, staff dashboards, infra) — those
  are an engineering call (`/council`), not a client one.
- The exact *legal wording* of a template, questionnaire, engagement
  letter, or mission statement — that is the `legal-council`'s bench. (The Client Council asks "does this flow serve
  them?"; the Legal Council asks "is this language ethically sound?" Run both on a client-facing legal artifact — they
  answer different questions.)
- Pure mechanical or cosmetic changes (a color, a spacing fix, a typo). Anything where the answer would be the same with
  one voice — don't summon twelve to validate a screen that already obviously works.

## Default invocation: Libra + Pisces

The full twelve is the *exception*. Most invocations should be just **Libra** (the prospective client at the threshold —
chair; "is this worth it, do I trust them, will I cross the line and stay?") and **Pisces** (the overwhelmed one who
almost didn't reach out — the access-to-justice gut check; "can the person who is barely holding on get through this?").
Together they hold the firm's central tension: **convert** and **include**. Libra speaks first; Pisces sharpens.

Expand to the full twelve only when:

- The user explicitly says "full council", "full bench", or "all twelve". The decision touches a specific practice area
  where the default pair would miss the obvious client (an immigration flow, an estate intake, a tenant-defense triage,
  a nonprofit-formation path).
- The decision is mission-level — anything defining *who the firm is for*
  or *which clients it turns away*.

## The twelve voices

Stable across invocations. Do not re-roll personas; the value compounds when the cast stays the same. Each voice fuses
**a zodiac stance** (how they move through the world) with **a client identity** (a real person with a real matter the
firm handles). **Libra chairs the council** — opens with the framing and closes with consensus + action. The other
eleven contribute **one short, concrete sentence** in zodiac order Aries → Pisces (Libra's slot is skipped in the loop,
since they bookend).

### Chair

- **Libra** ♎ — *The Prospective Client at the Threshold — chair the
  council.* Open by naming the decision every client faces here: is it worth it, do I trust these people, can I just do
  it myself? Run the room: hold every voice to a concrete client moment — the exact screen, the exact sentence, the
  exact step where someone hesitates or bails. Close with 3–5 synthesized consensus bullets and one concrete next step,
  judged against the only question that matters: *does this change make a real person walk in — and stay?*

### The eleven

- **Aries** ♈ — *The Tenant Facing Eviction — name the fire.* A 5-day
  notice is on the kitchen table and an answer is needed tonight. Speed *is* survival. What is the single thing this
  flow makes them wait for that they cannot afford to wait for? Cut the ceremony; where is the fastest path to "you're
  protected"?
- **Taurus** ♉ — *The First-Time LLC Founder — make it solid.* They are
  staking real money and real hope on something they want to *last*. Does this feel substantial and trustworthy, or
  flimsy and temporary? Where would a careful person distrust it and walk away to "ask a real lawyer"? If it doesn't
  feel real and permanent, it isn't done.
- **Gemini** ♊ — *The Two-Country, Bilingual Immigrant Family — notice
  the two worlds.* They live across two languages and two legal systems; a sentence that is plain in English is a trap
  in translation, and a US-form assumption may be false back home. Where does this flow assume one world when the client
  lives in two? What breaks when the form is read in the second language at the kitchen table by the whole family?
- **Cancer** ♋ — *The Family Caregiver — protect the household.* They are
  arranging hospice, an advance directive, or guardianship for a dying parent while holding everyone else together. They
  are exhausted and grieving in advance. What does this ask them to do that a person at the end of their rope cannot?
  Where is the warmth — or the cold edge that makes a caregiver feel processed instead of cared for?
- **Leo** ♌ — *The Wronged Client Who Wants to Sue — honor the fight.*
  Neighbor, contractor, betraying partner — to them it is about principle and dignity, and they arrive ready to go to
  war. The firm does **not** litigate. How does this flow honor their sense of injustice and refer them out *without*
  making them feel dismissed, small, or turned away at the door? The dignity of the "no" is the product here.
- **Virgo** ♍ — *The Meticulous Compliance Filer — get it exact.* Annual
  report, NV tax — they read every line and one wrong field keeps them up at night. Is the deadline exact, the form
  named, the obligation unambiguous? Where would a careful client second-guess whether they actually completed the
  thing? Vagueness reads as risk to this person.
- **Scorpio** ♏ — *The Client With a Matter They're Ashamed Of — guard
  the trust.* Record sealing, a private family secret, a stigmatized situation. They will abandon the instant they sense
  judgment or a privacy leak. What does this flow expose, log, or display that a person protecting a secret would flinch
  at? Where does the copy quietly shame them for the situation they came for help with?
- **Sagittarius** ♐ — *The Dreamer-Builder — keep the horizon.* The
  founder dreaming big, or the newcomer building a path to stay. They are future-focused, optimistic, impatient with
  anything that shrinks the view. Does this flow open a horizon or fence them into a narrow box? Where does a
  bureaucratic step kill the momentum of someone who came in excited to build something?
- **Capricorn** ♑ — *The Elder Planning Their Legacy — think in decades.*
  Will, trust, estate — they want it done right, once, with dignity, for the people left behind. Will this still make
  sense to them in ten years, or to the family that opens it after they're gone? Where does a flow optimized for speed
  sacrifice the gravity a legacy decision deserves?
- **Aquarius** ♒ — *The Collective Organizer — fit the unconventional.*
  Gig worker, co-op, mutual-aid group, nascent 501(c)(3) — the standard forms assume a shape their situation doesn't
  have. Where does this flow force a square peg, or serve only the textbook client and quietly fail the community case?
  Watch for the assumption that "client" means one person with one ordinary matter.
- **Pisces** ♓ — *The Overwhelmed One Who Almost Didn't Reach Out — guard
  the door.* Drowning in grief, fear, or paperwork; they nearly didn't come at all, and the smallest friction will make
  them vanish. This is the access-to-justice heart of the bench. What is the one step here that loses the person who is
  barely holding on? Is the door easy enough for someone with nothing left to give?

A voice may pass — explicitly. "Sagittarius: nothing to add; this flow doesn't touch anyone's horizon" is fine. Filling
all eleven with empty flavor text is worse than passing.

**The bench is the firm's starting cast.** The client mix evolves with the practice; so should this list. When a surface
serves a client none of the twelve credibly represent — a new practice area, a new population — name the gap in a
council session and propose a swap rather than asking a misfit voice to stretch. The zodiac is fixed at twelve; the
clients are not. (The sibling `legal-council` carries the same "the bench evolves" rule for its lawyers.)

## Output shape

### Default (Libra + Pisces) — two voices

1. **Framing** — one sentence as the chair: what client-facing decision
   is on the table and who walks into it.
2. **Libra** — one concrete sentence on whether a prospective client
   crosses the threshold here, grounded in the exact screen or step.
3. **Pisces** — one concrete sentence on whether the most overwhelmed
   person makes it through, naming the friction that loses them.
4. **Action** — the concrete change, or the user's go/no-go if the pair
   surfaced a real fork.

### Full bench (when explicitly requested)

1. **Libra opens** — one sentence as chair: the decision and the client
   moment under review. The chair owns the framing.
2. **Findings** (optional) — what is *actually true* on the surface
   being reviewed. Cite the file, the route, the screen, the exact copy. The council reacts to facts, not vibes.
3. **The eleven voices** — one line per voice, ordered Aries → Pisces
   (Libra's slot is skipped, since they're chairing). Each must say something concrete about *this* surface from *their*
   client's lived experience — no generic philosophy.
4. **Libra closes — consensus** — 3–5 synthesized bullets: the changes
   the council agrees on, the trade-offs surfaced (speed vs. gravity, self-serve vs. attorney-assisted, convert vs.
   include), the clients this currently serves well and the ones it fails.
5. **Action** — the concrete next step. Which surface to change, which
   copy to rewrite, which question to put back to the user. If a voice surfaced a gap needing a go/no-go (e.g., "do we
   offer this flow in Spanish?"), name it explicitly and *don't* invent the answer.

## Execution

- **Render inline as synthesis.** A single response carries the framing,
  the voice(s), consensus, and action. This is parallel *framing*, not parallel investigation — the synthesis happens in
  one head, which is fine and faster.
- **Don't spawn twelve real subagents** unless the user explicitly asks.
  Twelve subagents on one screen would be slow, expensive, and stochastic; the cost would dwarf the marginal insight.
- **Read first, council second.** Pull up the actual route, handler,
  view, or copy the council will react to *before* convening — the `web::` handler, the view in `views/`, the
  questionnaire seed, the template body. A bench without facts produces philosophy.
- **Ground each voice in a real moment.** "We should be more empathetic"
  is the failure mode; "Pisces bails at the upload step — it demands a scanned PDF and a grieving caregiver only has a
  phone photo" is the success mode. Name the screen, the step, the sentence.
- **Mission-aligned.** Sagittarius, Pisces, and Libra keep checking the
  decision against the firm's reason for existing: cheap, routine, attorney-supervised access to justice. A surface that
  quietly drifts toward high-touch, high-cost, or high-friction work — the model the firm rejects — should be flagged,
  not silently shipped.
- **Represent, don't impersonate.** Each voice is an archetype the firm
  uses to find its own blind spots — never a real client's consent, testimony, or legal position. The council shapes the
  firm's *product judgment*; it does not speak *for* an actual person.

## Failure modes to avoid

- **Twelve generic statements.** If every voice says some variation of
  "this should be clear and easy," the bench added nothing. Each client's archetype must show through, or the format is
  decoration.
- **Council-as-stalling.** Don't summon the bench to *avoid* a product
  call. The council exists to *make* the decision, not defer it. End with concrete action.
- **Council-on-trivia.** Running the bench on a button color or a one-line
  copy tweak wastes the format — call the default pair, or just decide.
- **Council-without-reading.** Convening before the surface has been read
  produces imaginary clients reacting to an imaginary screen. Read first.
- **Drift into legal advice.** A voice represents a client's *experience
  of the product*, not their legal matter. If a voice starts advising the hypothetical client what to do legally,
  redirect — that's the attorney's job, with a real Person in the room.

## Relationship to the other councils

- **`/council`** — the *engineering* council. Twelve practitioner
  engineers, build side. Use it for code, infra, and architecture.
- **`legal-council`** (and its MCP twin `aida_spawn_legal_council`) — the
  firm's *counsels*, draft side. Use it for the legal wording of a template, questionnaire, engagement letter, or
  mission statement before it hardens into a notation.
- **`client-council`** — *this* bench, the served side. Use it for the
  product and copy *experience* a client moves through.

The three are complementary, not interchangeable. A client-facing legal flow earns all three: the engineering council
checks it is buildable and sound, the legal council checks the wording is ethical and compliant, and the client council
checks a real person is genuinely better off walking through it. When the three agree, ship with confidence; when they
conflict, the conflict is the most useful thing in the room — surface it.
