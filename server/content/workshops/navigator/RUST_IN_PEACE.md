---
kind: workshop
title: Rust in Peace
description: Rust in Peace tours the Rust monorepo behind a litigation-first law practice.
---

# Rust in Peace

*A Neon Law talk for [Rust NYC](https://www.meetup.com/rust-nyc/events/316056830/).*

Rust in Peace is a eulogy to my programming career. Over the past year, teaching coding at Apple, I realized nearly all
my students were vibe coding and had stopped needing me. That was the signal to go back to practicing law.

Our firm runs its entire practice on one Rust monorepo: the store, the durable workflow engine, identity, telemetry, PDF
generation, and the browser surface. We are currently litigating nine-figure matters, and the software is what gives us
the context to win them. This talk walks through what that actually looks like in production, including our migration
from Postgres to SurrealDB. That move came from discovering that the hardest question in legal work is a graph one,
because we deal with people, their relationships, and their problems.

You will see how it fits together, and how we choose Rust vendors all the way down: Restate, Rauthy, OpenObserve,
Pingora, Stalwart, and Regorus. Writing software is not my job anymore. That is only possible because the ecosystem is
this good, because the LLMs are this good, and because communities like RustNYC are this good. Please come to my
retirement party and let us get crabby together.

## Intro

### A eulogy in Rust

![Ferris the Rust crab in goggles holds a glowing vial, Megadeth Rust in Peace parody](img/rust-in-peace/cover.png)

---

*Rust in Peace* is a play on the Megadeth album, and a eulogy to my programming career — a Neon Law talk given for [Rust
NYC](https://www.meetup.com/rust-nyc/events/316056830/).

Open here. The title plays on Megadeth's *Rust in Peace*, and I mean it: this talk is a eulogy to my programming career.
Over the past year, teaching coding at Apple, I watched nearly all of my students turn to vibe coding and stop needing
me — that was the signal to go back to practicing law. What follows is how a two-person firm runs an entire litigation
practice on one Rust monorepo: the store, the durable workflow engine, identity, telemetry, PDF generation, and the
browser surface. Going all in on Rust is what lets me lay the old career down and Rust in peace.

### Agenda

An agenda, not a lecture outline — you are here to argue back. By the end of the half hour you will be able to:

- **Recount** how a two-person team crossed from software to law without dropping the toolchain. **Trace** our process
  from the law, to a Cucumber feature, to a reusable template, to the signed notation it produces for one client.
  **Dissect** one workflow, forming a Nevada LLC, into attorney-gated steps with the shipped code. **Map** the Rust
  ecosystem we rely on and the seams that let us change it. **Defend** the claim that a reviewed, repeatable workflow
  beats a prompt. **See** how a grounded issue becomes a reviewable change, with automation carrying evidence forward
  and a person retaining the decision.

---

We frame this as an agenda rather than a lecture outline. You are here to argue back, not to be tested. We will trace a
law firm that chose one language, the ecosystem and seams that make that choice survivable, and a legal workflow whose
steps are small enough to be tested and reviewed. The close is our development loop: start from an issue grounded in the
codebase, use automation to carry the evidence through triage, tests, and advisory reviews, then hand the merge decision
to the responsible human.

### Las Vegas, 2011 — where I learned to program

![The Las Vegas Ruby Group logo: a ruby beside the wordmark, lvrug.org, @LVRUG](img/lvrug/lvrug.png)

---

I did not learn to code alone at a terminal. I learned it in the Las Vegas Ruby group, in 2011, watching people who had
never met agree on how a web application should be shaped. Rails was young, the community was younger, and the
astonishing part was not the framework — it was the *agreement*. A room full of independent developers converging on
shared conventions, in the open, with no one forcing them to. That experience is the whole reason this talk exists:
everything I now believe about a foundation-governed language, a commons, and shared conventions started in that room.

## The Rails Lesson

### Convention over configuration

Rails' core idea was radical to a beginner: **stop configuring, start agreeing.** If everyone follows the same
convention, the framework fills in the rest — and a stranger can read your code.

---

"Convention over configuration" was the first big idea I ever internalized about software. Instead of every project
inventing its own layout, Rails said: here is where models go, here is where controllers go, here is how you name things
— and if you follow it, everything wires itself together. For a new programmer that was a revelation. The convention was
not a cage; it was a shared language that let me read someone else's app and immediately know where everything lived.
That is the same instinct that later became Navigator's templates and its CLI orchestration rule: agree on the shape,
and reuse compounds.

### The database was a shared design

The first convention I learned was the schema. **Migrations** turned the database into something a team designed
together — versioned in the repository, applied the same way on every machine.

---

Database design was where the conventions got concrete. Migrations meant the schema was not a thing one person
configured on one server — it was code, in the repository, applied identically everywhere. I learned what a foreign key
was, what a join was, what an index bought you, all inside a convention that made the database a *shared* artifact
instead of a private one. Navigator runs one engine everywhere for exactly this reason, and the lineage is direct: the
schema is a design the whole team reads, not a secret on one box.

### Controllers — where a request becomes a decision

The next thing we agreed on was the **controller**: the one place a request turns into an action. Same file, same shape,
in every app — so anyone could find the logic.

---

Controllers taught me that a web request is not magic — it is a function with a shape everyone agreed on. The request
comes in, the controller decides what to do, a response goes out. Because Rails standardized where that happened, I
could open any Rails app in the world and know exactly where to look for what it did. Navigator's Axum handlers are the
same idea in Rust: a request arrives, a handler makes a decision, and the shape is consistent enough that the next
reader — human or agent — knows where to look.

### Parameter handling — the shape of what comes in

Then we learned to distrust the input. **Parameter handling** was the convention for taking what a user sent and making
it safe before it touched anything that mattered.

---

Parameter handling was my first lesson in the trust boundary. Everything a user sends is suspect until you have named
it, permitted it, and shaped it. Rails made that a convention instead of a habit you hoped everyone remembered. That
lesson runs straight through Navigator: the questionnaire is a typed, ordered set of answers, not a free-form blob, and
the privacy rule — identifiers and counts in telemetry, never client content — is the same discipline, grown up. You
decide the shape of what comes in.

### Middleware — the layer I didn't know I needed

The last convention floored me: **middleware.** A stack of small, composable layers every request passes through —
logging, auth, sessions — each one modular, each one reusable.

---

Middleware was the idea that made me feel like I finally understood software. A request does not go straight to your
code; it passes through a stack of small layers, each doing one job — logging here, authentication there, sessions after
that — and you compose them. Modular, ordered, reusable. I did not know I needed that concept until I saw it, and then I
saw it everywhere. Navigator's `tower` / `tower-http` middleware and its in-process Regorus policy layer are the same
pattern: small, composable steps a request walks through, each testable on its own. The workflow state machine is
middleware grown into the practice of law — a stack of modular steps every matter passes through.

### "It works on my machine"

And then there was the phrase every one of us said, half-joking, in that Rails room: **"It works on my machine."** It
was a promise and a curse — the first bug we could never reproduce.

---

This is a thread we will pull on for the rest of the talk, so hold onto it. "It works on my machine" was the punchline
and the original sin of that early community. It worked for you and broke for me, and we could not say why, because the
machine was special — it carried state nobody had written down. Every good thing that happened to software since is, in
some sense, a campaign against that sentence: version control, migrations, containers, CI, durable execution. Remember
the phrase. We are going to earn the right to say it again — and mean it — by the end.

### How new all of this still was

What stunned me most was how **young** it all was. These were not settled laws handed down — they were conventions a
small community was inventing, out loud, in real time. And I got to be in the room.

---

The thing I did not appreciate at the time was how nascent it all was. These conventions felt like natural law to a
beginner, but they were only a few years old, argued into existence by a small, generous community that decided to agree
in public. Programming was new enough that an ordinary person at a Las Vegas meetup could watch the norms of an entire
ecosystem get set. That is a rare thing to witness, and it left me with a conviction I never lost: the conventions that
matter are built by communities in the open, not handed down by owners. Which is exactly the argument this talk makes
about Rust.

### From Rails conventions to Navigator

Fifteen years later I am a lawyer, and I kept every one of those lessons. **Schema, controllers, parameters, middleware,
convention over configuration** — Navigator is those Rails ideas, applied to the practice of law in Rust.

---

Here is the segue. I stopped writing Ruby and I became a litigator, but I never stopped believing the things that room
taught me. Navigator is what happens when you take convention over configuration, a shared schema, consistent
controllers, disciplined input handling, and composable middleware — and apply them to legal work, in a language
governed by a foundation the way that community was governed by its own good faith. The rest of this talk is those
conventions, grown up: a template is convention over configuration for a legal matter, a workflow is middleware for the
practice of law, and the attorney review gate is the strongest parameter check we have. Let me show you.

### A eulogy for my programming career

I thought becoming a full-time lawyer meant leaving production software behind. Rust changed the obituary: I can let the
old career rest because its best habits now run the law practice.

---

The honest version of the origin story: we were engineers who got tired of watching routine legal work priced out of
reach of the people who needed it most. So we went and got licensed. I thought that meant choosing a side: spend the day
as a lawyer, then steal nights to keep production software alive. The surprise was that software development had changed
enough to make the split unnecessary. With Rust, Cargo, and the ecosystem around them, the serious parts I would have
had to invent alone — web servers, durable execution, observability, data formats, editor tooling — were already open,
reviewed, and composable.

So yes, this is a eulogy. Not for programming, but for the version of my programming career that had to be a separate
identity. A pull request and a contract are closer than either profession likes to admit: both are reviewed line by
line, both fail in the edge cases, both are worse when a single person is the only one who understands them.

Neon Law Navigator is what fell out of that conviction. It is a harness — a deterministic checklist applied every time —
that grounds an LLM's output in a shared, database-backed vocabulary so the routine parts of legal drafting come out
correct and cheap. The lawyer still signs. The machine just makes it faster and more correct to *be* the lawyer who
signs.

## Shared craft compounds

### Keep domain seams in Rust, where interest compounds

The bet is not "write a service in Rust." Rust owns reusable domain systems: libraries, command-line tools, workers,
editor integrations, tests, and — through Dioxus — the browser pages. The payoff is that each good decision reinforces
the next one.

---

The core claim for a Rust room is stronger than "Rust is pleasant" or "Rust is safe." The claim is that keeping domain
seams in Rust changes the economics of a small team. A CLI can call the same validators as the website. A workflow
worker can share the same state-machine types as the template authoring rules. The LSP can surface those rules in
whatever editor a lawyer writes in. A release can ship one dated set of binaries, images, and editor assets. The
ecosystem compounds when the domain seams stay in the same language long enough to reuse them.

That is why Rust owns Navigator's rules, workflows, storage, authorization, `store`, CLI, and — through Dioxus — the
browser surface. `navigator` orchestrates machine-bound flows; there are no shell scripts and no Makefile. We do not win
because we wrote more code than everyone else. We win when the code someone else wrote in the Rust ecosystem becomes a
trustworthy brick in our law practice.

### Widely available — governed in the open

Rust is stewarded by the [Rust Foundation](https://foundation.rust-lang.org/), an independent non-profit whose members
include AWS, Google, Microsoft, and Meta — none of whom *own* the language. Wide availability is the access-to-justice
argument: the toolchain costs a clinic exactly what it costs us — nothing.

---

The trademark, the infrastructure, and the long-term stewardship live in a neutral body whose mission is to support the
maintainers and the open ecosystem, not to monetize a single vendor's roadmap. That governance structure is exactly why
the language is *widely available* — and wide availability is the access-to-justice argument. The toolchain that runs
our production system costs the same for a legal-aid clinic, a law student, or a solo practitioner in a one-stoplight
town as it does for us: nothing. We are a foundation-stewarded practice building on a foundation-governed language, and
the rhyme is not an accident: a commons, run in the open, is the only infrastructure model that scales *down* to the
people the mission serves as well as it scales up.

### A cautionary tale — Java, Oracle, and the price of a single owner

Java was created at Sun; Oracle acquired Sun in 2010 and sued Google that same year over Android's reuse of 37 Java SE
API packages. *Google v. Oracle* ran **more than a decade**, until the [Supreme Court ruled in
2021](https://en.wikipedia.org/wiki/Google_LLC_v._Oracle_America,_Inc.) it was **fair use**.

---

The contrast that makes the case is a matter of public record. The point for this audience is not which side was right.
The point is the *exposure*. When a language and its APIs are owned by a single company, the terms under which you build
on it are one acquisition or one lawsuit away from changing. A foundation-governed language removes that entire category
of risk. With Rust, the eleven-year question simply never gets asked.

### Say it with your whole chest

And when someone asks why it is written in Rust: because we are doing it in motherfucking **Rust**.

---

The language says we intend to be right. There is a version of engineering humility that shades into apologizing for
existing, and it does not serve clients, it does not serve the mission, and it does not close deals. Fuck yeah, we are
the best at this. Fuck yeah, it is in motherfucking Rust — memory-safe, fearlessly concurrent, and compiled to a single
binary a clinic can run on a laptop. Pick the tools you can defend all the way down, then say the claim out loud.

## Six ways to know a change holds

### One giant change deserves more than one glance

When two numbers are too large to hold side by side, you can compare several well-chosen positions instead of trusting
one glance. AI-assisted code is similar: no one review sees the whole change, so we ask the same question six different
ways.

---

This is an analogy, not a claim of mathematical proof. A few sampled positions can miss a difference; six checks can
share the same blind spot. But a giant change — a new feature, a refactor, or a generated patch — is too large for one
person to understand all at once. The practical response is to make several **deliberately different measurements** of
it. Each view asks whether the change still agrees with a different contract: the language, the conventions, the
behavior, the learner, the reader, and the person who must act on it.

The point is not to slow down vibe coding into bureaucracy. It is to make rapid iteration safe enough to keep: generate
quickly, then make the result survive several kinds of contact with reality.

### The compiler asks whether the program can be true

**Rust compiler**: do the types, ownership, lifetimes, and exhaustive cases still compose into a program the machine can
run?

---

The compiler is the first explanation of a change, expressed in the language's own terms. It cannot tell us whether a
rule is legally correct or whether a screen is useful. It can tell us whether the program's stated relationships are
internally coherent: a value is owned somewhere, a borrowed value outlives its use, errors are handled, every variant is
accounted for. That is an unusually strong first filter for AI-generated code because it turns a vague instruction into
a precise question the toolchain can answer.

For a refactor, compile early and often. Let the type system show where the old story and the new story disagree before
you spend time polishing either one.

### Clippy and rustfmt ask whether the code speaks our dialect

**Clippy** finds suspicious choices. **rustfmt** removes accidental visual difference. Together they make the code
easier for the next reader — and the next agent — to recognize.

---

Passing compilation is necessary, not sufficient. A patch can type-check while hiding an avoidable allocation, an
unhelpful conditional, a lossy conversion, or code that only looks novel because its formatting is noisy. `cargo clippy`
and `cargo fmt` compare the result against a shared craft vocabulary.

That shared dialect matters more when work moves quickly. Formatting makes a mechanical rewrite visually quiet; Clippy
is a second pair of eyes for patterns that compile but deserve a question. Neither replaces judgment, and neither should
be waived just because an AI wrote the first draft.

### Tests ask whether the promise still holds at the boundary

Tests are the behavioral measurement: given this input, state, or failure, does the system still keep the promise it
made before the refactor?

---

A test is stronger than “the code looks right.” It fixes an observable promise at a boundary: a command rejects an
invalid request, a workflow cannot skip attorney review, a rendered document carries the right answer, an API returns
the right response. When a generated patch changes behavior, write or update the test first; when it claims to be a
refactor, let the existing test prove the claim.

Good coverage is not a percentage that blesses a change. It is a set of examples that would fail for the mistakes we are
actually afraid of — especially the edge cases an eager generator is least likely to notice.

### Bloom turns verification into a learning loop

The [Bloom taxonomy](https://en.wikipedia.org/wiki/Bloom%27s_taxonomy) gives the same work six human verbs: **remember,
understand, apply, analyze, evaluate, create.**

---

Bloom is a useful counterweight to tool output. The compiler can say a program is well-formed; a learner still needs to
name the concept, explain it, use it, pull it apart, judge it, and make something new with it. The three Navigator
workshops already use that progression because it makes the participant, not the software, the actor.

Use the same ladder on a proposed change. Can we **remember** the rule it touches? **Understand** why the rule exists?
**Apply** it to one real path? **Analyze** a failure? **Evaluate** the trade-off? **Create** a smaller, clearer version?
If we cannot explain a patch at those levels, we do not yet own it.

### Marketing copy must explain the way, not only the claim

“AI helps us move faster” is not an explanation. The useful promise is: **we move faster because every change meets
several explicit checks before it earns trust.**

---

This is the public-language version of the method. We should not market a magic machine or promise perfection. We can
say what actually happens: Rust makes invalid states difficult to express; Clippy and formatting keep the code legible;
tests protect the behavior; a lawyer reviews the legal work; the workflow records what happened. The speed comes from
reusing those checks, not from skipping them.

That copy matters because it keeps the product story falsifiable. A prospective user should be able to read the claim,
open the code or workshop, and see the safeguards named there. “Explain the way” is a better promise than “trust us.”

### Three workshops, three people, one method

The same explanation changes with the person doing the work:

- **Using the Navigator** — the licensed lawyer checks a matter and signs the result.
- **Operating Navigator** — the admin operator checks the environment before it goes live.
- **Contributing to Navigator** — the contributor checks that an improvement protects the shared corpus.

---

One method should not become one generic pitch. The lawyer needs to hear that the harness preserves professional
judgment and records the review. The operator needs to hear that dry runs, repeatable provisioning, and readiness checks
make a deployment inspectable before it affects a firm. The contributor needs to hear that a small change becomes
durable only when it has a focused test and works in the same local topology as production.

Those are three different personas, three different stakes, and three versions of the same discipline: make the claim,
then look at it from enough angles that its weak spots have somewhere to surface.

### Fast generation, slow enough verification

Vibe code quickly. Then keep it **DRY, refactorable, covered, formatted, and explainable** — six perspectives make the
feedback loop faster, not heavier.

---

The practical loop is short: state the behavior; ask the model for a small change; compile it; format and lint it; run
the focused test; read the diff; explain the change to the persona who will rely on it. When the change is larger, add
the wider test suite and the human review that match its risk. When one of those views disagrees, do not paper over the
signal — improve the abstraction, remove duplicated logic, or split the change until the explanation becomes clear.

This is how rapid AI-assisted work compounds rather than decays. We keep the speed of generation while preserving the
craft that lets the next person safely refactor, reuse, test, and trust what we just made.

## From Law to Workflow

### The goal — deterministic workflows from law

The whole method on one slide: a prompt is a wish; a workflow is a contract. Read the law → a Cucumber feature → a
**template** (the reusable blueprint) → a **notation** (one client's reviewed, signed result).

---

Our process begins by reading the law itself. We translate what the law requires into Cucumber features — executable
behavior, written before any code. Then we express the work as a template: one markdown file whose frontmatter carries a
questionnaire (the questions a client answers) and a workflow (the state machine the matter walks). When a client
engages us, Navigator creates a notation from that template — one client's answers bound to one workflow run — and every
notation passes a lawyer review owned by the attorney who is the matter's directly responsible individual before
anything leaves the building.

The rest of this talk dissects one real workflow — forming a Nevada LLC, our Neon Law Nest product — into its small,
modular steps, one slide per step, with the exact shipped code behind each. Exact means exact: a test compares every
snippet on these slides against the file it cites and fails the build on drift.

### Step 1 — read the law

A Nevada LLC is a creature of statute: NRS Chapter 86 says what the Articles of Organization must contain. We do not
paraphrase from memory — we read the chapter at its official source and cite it.

---

NRS Chapter 86 says what the Articles of Organization must contain, who can be a registered agent, and what the
Secretary of State will accept. We do not paraphrase the law from memory — we read the chapter as the Legislature
publishes it and cite that text. The law is the upstream; everything below is a faithful translation of it.

### Step 2 — write the behavior before the code

The first artifact is not Rust — it is a Cucumber feature describing the whole arc in plain language, runnable as a
test: a founder intakes, an attorney reviews, signatures land, and the state stamps a filing.

From `features/tests/features/nest_formation.feature`:

```gherkin
  Scenario: From intake to a stamped Secretary-of-State filing
    When the firm opens the "nv__llc_formation" matter for the client
    And the founder answers the formation questionnaire:
      | value                  |
      | Libra                  |
      | Bright Star Ventures   |
      | Neon Law Registered Agent |
      | members                |
      | Libra; 1 Main St; Las Vegas; NV; 89101; USA |
      | 2026-07-01             |
    And the attorney approves and sends the document
    Then the formation reaches the signature wait
    And the persisted packet is the official SoS form carrying the founder's answers
    And the generated packet is filed as a document in the matter
    When the attorney files the Articles with the Nevada Secretary of State
    Then the formation workflow reaches END
    And a filing was recorded with the "Nevada Secretary of State"
    And the founder's six onboarding answers are on file
```

---

This is the law translated into expected behavior. The [`cucumber`](https://docs.rs/cucumber) crate runs this scenario
against a real store on every `cargo test`. The feature is the contract; the code below exists to make it pass.

### "It works on my machine" — so we stopped trusting my machine

Callback. The fix for that old curse is not a better machine — it is to **stop trusting any machine.** The scenario
above runs against a real, throwaway store, so the environment is pinned in code, not remembered on a box.

---

Here is the phrase again, one step closer to earned. The Cucumber scenario on the last slide does not run against "my
database". The workspace test harness gives every test its own embedded engine, with the schema applied — no server, no
port, nothing to configure — so the same contract runs identically on a laptop and in CI. That is the first real answer
to "it works on my machine": make the machine stop being special.

### Step 3 — the template: a questionnaire and a workflow

The template is one markdown file with two machine-readable graphs: a questionnaire graph (what we ask) and a workflow
graph (what we do). The Nest questionnaire is six answers, in order.

From `templates/forms/united_states/nevada/state/nv__llc_formation.md`:

```yaml
questionnaire:
  BEGIN:
    _: person__client
  person__client:
    _: entity__company
  entity__company:
    _: person__registered_agent
  person__registered_agent:
    _: custom_single_choice__management_structure
  custom_single_choice__management_structure:
    _: people__managing_members
  people__managing_members:
    _: custom_datetime__formation_date
  custom_datetime__formation_date:
    _: END
  END: {}
```

And here is the workflow — the LLC formation dissected into small, named, modular steps. Each state is a noun in our
glossary; each transition is a signal some handler fires. This graph *is* the product.

From `templates/forms/united_states/nevada/state/nv__llc_formation.md`:

```yaml
workflow:
  BEGIN:
    intake_submitted: intake_persisted__organizer
  intake_persisted__organizer:
    articles_rendered: lawyer_review
  lawyer_review:
    approved: generate_pdf__articles_pdf
    rejected: END
  generate_pdf__articles_pdf:
    pdf_persisted: sent_for_signature__pending
  sent_for_signature__pending:
    signature_received: filing__nv_sos
    signature_declined: END
  filing__nv_sos:
    filed: END
  END: {}
```

---

Read the workflow aloud and it is the practice of law: intake, render, attorney review, signature, filing. Swap the
filing state and the same shape closes an estate plan or sends a consumer-report dispute letter — the steps are modular
because the states are vocabulary, not prose. No branching is needed for a simple formation.

### Step 4 — the attorney gate is a graph invariant

Every workflow must pass through `lawyer_review` before anything is signed or filed — not as a policy memo, but as a
property checked over the state-machine graph itself with a breadth-first search from `BEGIN`.

From `workflows/src/guardrail.rs`:

```rust
pub fn lawyer_review_precedes_signature(spec: &WorkflowSpec) -> Result<(), GateViolation> {
    let begin = StateName::begin();
    if let Some(signature) = reaches_target_without_review(spec, &begin, is_signature_state) {
        return Err(GateViolation {
            fill_state: begin.as_str().to_string(),
            submission_state: signature,
        });
    }
    Ok(())
}
```

---

The guardrail fails any template where a signature state is reachable without an attorney review in between. This is
what "attorney-vetted" means in this codebase: the review gate cannot be skipped, because a template that skips it does
not load. The reviewing attorney is the matter's directly responsible individual — the DRI — and the bytes that go out
for signature are the bytes that attorney approved.

### Step 5 — signature is a modular step

`sent_for_signature__pending` is one state in the graph, and the thing that fires it is a small trait — not a vendor.
DocuSign is the shipped implementation; dev and tests run a recording stub, so the step stays testable without an
account.

From `portal/src/signature.rs`:

```rust
pub trait SignatureProvider: Send + Sync {
    /// Submit the rendered retainer PDF for the given notation, placing
    /// the fields described by `manifest`. Returns a provider-issued id
    /// correlating future events.
    async fn send_for_signature(
        &self,
        notation_id: Uuid,
        pdf: &[u8],
        manifest: &SignatureManifest,
    ) -> Result<SignatureRequestId, SignatureError>;
```

Because the step is modular it can also be *careful*. Dispatch is idempotent — a notation that already has an envelope
out reuses it, fires nothing, and sends nothing — so a retry can never double-send a client's contract.

From `portal/src/retainer_walk.rs`:

```rust
    // Idempotency: this notation already has an envelope out. Reuse the
    // recorded id, fire nothing, send nothing — the post-state is
    // whatever the notation already records.
    if let Some(existing) =
        store::signatures::request_id_for_notation(deps.surreal, notation_id).await?
    {
        return Ok((
            StateName::from(notation_row.state.as_str()),
            crate::signature::SignatureRequestId(existing),
        ));
    }
```

---

The trait is the seam: DocuSign is the shipped implementation, and dev and tests run a stub that records every call so
the step itself stays testable without a vendor account. The idempotency check shown above is what makes a retry safe —
the post-state is whatever the notation already records, so no second envelope ever goes out.

### Step 6 — the filing, run durably

The last state, `filing__nv_sos`, records the filing with the Nevada Secretary of State — and like every long-running
step it executes as a journaled, resumable [Restate](https://restate.dev/) workflow through the
[`restate-sdk`](https://docs.rs/restate-sdk) crate.

---

A workflow that survives a pod restart and replays to exactly where it left off used to be big-company infrastructure;
in Rust it is a dependency line. The same durability runs our nightly archive: a law firm carries a ten-year retention
duty, so every night we snapshot the store into [Parquet](https://docs.rs/parquet) via [`arrow`](https://docs.rs/arrow)
— the open columnar format the Iceberg lakehouse world builds on. And when a matter one day calls for an on-chain
record, the door is already open: Solana programs are written in Rust, so the same workspace can speak to the chain
natively — not shipped yet, and we will say so plainly until it is.

### "It works on my machine" — even when my machine dies

Restate takes the phrase somewhere new. The durable filing does not live on the machine at all — it is **journaled**, so
it survives the pod that ran it and replays exactly where it left off.

---

Callback again, and the strongest technical one so far. A workflow that runs durably has cut the last cord to any
particular machine. The filing step's progress is journaled, so if the process dies mid-run, another process picks it up
and replays to the exact point it left off — same inputs, same outputs, no double-send. "It works on my machine" used to
mean the state was trapped on that machine. Durable execution inverts it: the state is written down, portable, and
machine-independent. The run belongs to the journal now, not to the box.

### Why a workflow beats a prompt

You could ask a frontier model to "form me a Nevada LLC" and get something plausible. We built the harness instead,
because plausible is not the bar — repeatable is. A prompt's steps are neither repeatable nor modular.

---

The same words produce different documents on different days, so a prompt's steps are not repeatable. You cannot swap
its signature vendor, test its review gate, or prove its filing fired exactly once, so a prompt's steps are not modular.
A workflow gives you all of that, plus the thing no model can supply — a licensed attorney, the DRI, reviewing every
notation at a gate the graph cannot route around.

Our goal is to create as many of these reliable, attorney-vetted workflows as possible for our customers. Each one is
concise automation for one real legal outcome — formation, estate plan, screening-report dispute — and each new template
reuses the same states, the same guardrails, and the same review gate. That is how the floor rises: not a bigger model,
but a longer shelf of workflows anyone can read, run, and extend.

### Privacy-preserving operations are part of the product

Legal tech cannot treat observability as a copy of production. Navigator emits OpenTelemetry traces, metrics, and logs
through one Rust crate, and the rule is structural: identifiers and counts, never client content.

---

The repository has one telemetry seam: every binary calls `telemetry::init`. With the complete OpenObserve environment
contract it exports OTLP directly to the selected organization and stream; dev and CI fall back to human-readable
stdout. The interesting part is not the exporter. The interesting part is the trust boundary. A `notation_id`, a
workflow service name, an outcome, a duration, a status code — yes. A client name, an answer body, an email address, a
document body — never. The Rust code keeps request bodies out of spans in the first place, and every production stream
must enforce the same allow-list at its boundary.

The analytics story follows the same pattern. Operational telemetry goes directly to OpenObserve. Matter data is
archived separately by the nightly Restate `Archives` workflow: store snapshots become Parquet through `arrow` and
`parquet`. The separately tracked Iceberg work must land with a restore drill before we call this an open table format.
The ecosystem point is simple: the boundary is written down, enforced, and testable.

## Rust All the Way Down

### We did not start all Rust

Navigator did not begin as a pure-Rust stack. We have moved the boundary a seam at a time: server-rendered Maud with
HTMX/Alpine/Bootstrap gave way to Dioxus; React and TipTap were evaluated and declined; the browser now shares the Cargo
workspace with the rules and the server.

---

The important part of this story is the verb: **moved**, not replaced everything. A browser surface was the first proof
that Rust could hold a domain seam end to end. We began with server-rendered pages and a small JavaScript layer because
that was the smallest working thing. Then the boundary became expensive: the same legal rules were being explained to
two toolchains. Dioxus lets the browser surface join the workspace that already owns rules, templates, workflows, and
tests. We looked at React and TipTap; we did not pretend evaluation was adoption. The decision was to make one Rust
toolchain the place a change is expressed, reviewed, and shipped.

### From Postgres to Rust — over a bridge, since dismantled

SurrealDB holds the record: document, graph, and key-value work meet in one Rust engine. Every port slice proved its
reads and writes before the old path went away, and the last of them took Postgres with it.

---

The bridge was load-bearing while it stood. Postgres stayed the source of truth, entities and migrations real, while
SurrealDB ran beside it — so a table, query, or workflow moved as a small falsifiable claim rather than as a weekend
rewrite of a law practice. The move also retired purpose-built graph machinery where SurrealQL is the better fit. The
principle held all the way across: **keep the old truth until the new truth has evidence** — and then, once it does,
actually take the old one down.

### The runtime story: keep the seam, change the engine

The same move repeats at the runtime boundary. Durable workflows landed on Restate, local identity on Rauthy, the
private edge on Pingora, local S3 on Garage, and observability on OpenObserve — each behind an application-owned
protocol, trait, or environment contract.

---

None of those names is the architecture. The architecture is the seam. The Template declares; Restate runs. An OIDC
provider establishes identity, while Navigator's database decides authorization. `StorageService` keeps object bytes
behind one contract. OTLP is the telemetry export boundary. The current local loop uses a production-shaped KIND
dependency tier because it gives the code a real Restate journal, identity provider, object store, database, and
telemetry collector to meet. The next developer-experience step may replace the cluster with native Rust process groups,
but it must preserve the same environment contract and real-path tests. We do not call a mock a migration.

### One deployment bucket; many matter repositories

Storage and Git deliberately have different cardinality. The target is **one private object-storage bucket per
deployment**; documents, assets, exports, logs, and every Project's files use application-owned logical keys and
metadata inside it. Git is the inverse: one deployment-specific GitHub organization holds many private repositories,
such as `neon-law-stg-projects/PROJECT_CODE_1` and `neon-law-stg-projects/PROJECT_CODE_2`.

---

This distinction prevents two expensive mistakes. A Project is not a cloud-resource factory: creating a matter must not
create a bucket, a public IAM policy, or a lifecycle policy that a future operator has to clean up. The bucket remains
private, and marketing assets travel through Navigator's same-origin route rather than an `allUsers` binding. A Project
*does* receive its own private Git repository, named exactly by its code, because a repository is the readable,
reviewable history of that matter's working artifacts. One bucket is a storage boundary; many repositories are a review
boundary. Do not collapse the two.

### Rust where we can own the path; partners at the edge

The destination is not an ideological ban on vendors. Surreal Cloud, Restate Cloud, OpenObserve Cloud, Stalwart Managed
Email, and Miuda PBX are the Rust-operated lanes under recorded cutover gates. Xero, DocuSign, and the SIP carrier stay
at the business edge, where they provide the service we do not claim to reimplement.

---

That is the full setup: use Rust for the integration surface we need to understand and evolve, then have a commercial
relationship with the people operating the infrastructure. The mail decision is deliberately narrow: a Stalwart
automation subdomain owns `{project-code}@mail.<brand-domain>` and `no-reply@` at cutover; Google Workspace continues to
hold human mail at the apex. The voice decision is equally narrow: Miuda/RustPBX owns the application side and a SIP
carrier is the edge. Each production tenancy, DNS record, deliverability soak, event stream, archive proof, and vendor
retirement remains a named acceptance gate — an owner decision is not evidence that a cutover happened.

### Rust reaches the places lawyers work

Navigator is not only a web app. The same rules meet lawyers in the document folder they already understand, and the
same AIDA catalog is available to assistants through MCP and A2A.

---

This is where "available in many places" stops being abstract. The workspace builds a website, a Restate worker, trigger
jobs, a `navigator` CLI, an MCP server, and `navigator-lsp`. The LSP speaks JSON-RPC over stdio, has no telemetry, and
attaches to Markdown. Ordinary prose gets Markdown rules; templates get the notation rules on top, because a legal
template is both a document and a program. Most drafting happens in the WYSIWYG editor on the website; the LSP is what
carries the identical rules into the git side of the work, where the pull request and the GitHub Action see exactly what
the author saw.

The folder surface is an arrival, not a claim that the migration has already finished. #1102 owns the matter-folder
experience; #1122 provides its VFS seam. The AIDA catalog lives in mcp/src/tools and exposes the same grounded
operations through A2A and MCP. That is the direction: a lawyer keeps working with documents while the system keeps the
shape, the workflow, and the rendered PDF aligned.

### I want a commercial relationship with everything I depend on

A small legal team cannot maintain the world's infrastructure — so I want to **pay for it, in the open.** A vendor like
Restate, or **GitHub Sponsors** for the libraries. A healthy dependency is one someone is paid to keep healthy.

---

Here is a value that shapes every dependency choice we make. I do not want to depend on software that no one is
accountable for. Sometimes that means a commercial vendor — Restate is a company, and I want a real relationship with
the people who keep durable execution durable. Sometimes it means sponsoring a maintainer through GitHub Sponsors, so
the crate we lean on has someone paid to answer the hard bug. Wide availability and a paid relationship are not in
tension; they are the same bet. The commons stays healthy when the people who show up to maintain it can afford to keep
showing up. A foundation-governed language taught me that, and I want our money flowing the same direction as our trust.

### Always the latest, always Rust

Two rules keep the interest compounding: **take the newest version** of every dependency, and **use exactly one
language.** No pinned-and-forgotten crates, no second runtime, no polyglot seams to babysit.

---

I update dependencies to the latest, deliberately and often, because a dependency you never update is a liability you
are pretending is an asset. Staying current is how you keep receiving the security fixes, the performance, and the new
capabilities maintainers ship — and it is only affordable because Cargo and a strong type system make an upgrade loud
when it breaks. And it is all Rust, on purpose. One language from the HTTP handler to the migration to the browser test
means one toolchain to keep current, one set of conventions, one place a change reverberates. Every time I have been
tempted to add a second language for one clever thing, the cost of the seam has outweighed the cleverness. Go all in,
stay current — that is how a two-person team acts bigger than it is.

### Swap the seam — it still works

Every heavy dependency sits behind a **trait**, so changing one does not make the application a rewrite. Signatures,
storage, identity, durable execution, policy, telemetry, and the project repository surface have all had an explicit
seam. Sometimes the right change is a new provider; sometimes it is retiring a surface entirely.

---

Callback, and the one that sets up the finale. Because every heavy piece is behind a Rust trait, the environment is a
set of plugs, not a monolith. Signature is a `SignatureProvider` — DocuSign in production, a recording stub in dev and
tests. Object storage is a `StorageService` — Google Cloud Storage in cloud deployments; locally, the S3-compatible
Garage. Identity is OIDC, with Google in production and Rauthy in the local loop. The store is SurrealDB everywhere. The
project repository surface was deliberately retired in favor of a GitHub link-out, which is stronger evidence than
pretending every seam must keep the same shape. "It works on my machine" becomes a property of the contracts and tests,
not a promise that every dependency has already been replaced.

### The crates we actually run on

The bill of materials for a real legal-tech product — every line a crate you can pull today:

- **HTTP and views** — `axum`, `dioxus`, `tower` / `tower-http`.
- **Async runtime** — `tokio`, multi-threaded, with graceful shutdown.
- **Store** — `surrealdb`, one engine for document, graph, and key-value.
- **Durable execution** — `restate-sdk`.
- **Telemetry** — `opentelemetry`, `tracing`, OTLP.
- **Archive** — `arrow` and `parquet`; a real Iceberg table lane is separately tracked.
- **Content** — `pulldown-cmark`.
- **Cloud** — `google-cloud-storage` + `reqwest`.
- **Identity** — `jsonwebtoken` + `oauth2`.
- **Tests** — `fantoccini` and `cucumber`, exercised through the workspace harness.

---

So you can map this onto your own stack: for **HTTP and views**, [`axum`](https://docs.rs/axum) for the router and
handlers, [`dioxus`](https://docs.rs/dioxus) for the browser surface, [`tower`](https://docs.rs/tower) /
[`tower-http`](https://docs.rs/tower-http) for the middleware stack. The **async runtime** is
[`tokio`](https://docs.rs/tokio), multi-threaded, with signal handling for graceful shutdown. The **store** is
[`surrealdb`](https://docs.rs/surrealdb) — one engine for document, graph, and key-value. **Durable execution** is
[`restate-sdk`](https://docs.rs/restate-sdk) hosting every workflow on one worker endpoint, with the journal doing the
remembering. **Telemetry** is [`opentelemetry`](https://docs.rs/opentelemetry) and [`tracing`](https://docs.rs/tracing),
with OTLP as the export seam. The **archive** uses Arrow and Parquet. The real Iceberg-table and restore drill work is
tracked separately, so the slide does not turn a plan into a fact. **Content** is rendered from the Markdown you read.
**Cloud** is [`google-cloud-storage`](https://docs.rs/google-cloud-storage) behind a storage trait, with
[`reqwest`](https://docs.rs/reqwest) for the REST plumbing that provisions a fresh project. **Identity** is
[`jsonwebtoken`](https://docs.rs/jsonwebtoken) and `oauth2` for the OIDC flow. **Tests** use
[`fantoccini`](https://docs.rs/fantoccini) to drive a real browser over WebDriver, and
[`cucumber`](https://docs.rs/cucumber) for the behavior specs you saw in Step 2. One workspace, one `cargo test`, one
language from the HTTP handler down to the migration and back up to the browser assertion. None of these crates asked us
to sign anything.

### Ethics is part of the stack

Lawyers who code still carry the rules of professional conduct, and the engineering answer is the same as it is for
memory safety: make the invariant structural, not aspirational.

- **Scope is a field, not a vibe.** Every engagement is scoped in writing before work starts. **The conflict check runs
  first.** Before any matter opens, we query every current and former matter. **Referral, without a referral fee.** When
  conflicted out, we refer — with no referral fee.

---

**Scope is a field, not a vibe.** Every engagement is scoped in writing before work starts. When lawyers open a matter,
its scope narrative is seeded as the first clause of the retainer the client signs — for a flat-fee product like
**Northstar** (estate planning) or **Nautilus** (the screening-report shield), the agreement states exactly what the fee
buys, and work outside that scope takes a new or amended engagement.

**The conflict check runs first.** We offer every current product — Northstar, Nautilus, Nest, Nexus — and we will take
your matter if we can. Before any matter opens, we check it against every current and former matter across the whole
firm — a query, not a memory. Ethics rules may conflict us out: we cannot represent a business and an individual whose
interests are adverse to each other.

**Referral, without a referral fee.** When we are conflicted out, we refer you to counsel who also use the platform and
are committed to improving access to justice with our software. There are no referral fees between Neon Law and any firm
we refer cases to — the referral is the mission working, not a revenue line.

The deterministic harness and the ethics rules turn out to be the same idea: a checklist applied every time, encoded
where it cannot be skipped.

### The simplest developer environment I can get away with

The local loop is production-shaped on purpose: the Rust CLI provisions an isolated KIND dependency tier and the host
application process shares that exact environment.

---

This is not a claim that every dependency is a process on a laptop. The CLI owns the local lifecycle, gives each
worktree its own ports and database, and uses the same dependency topology that the application expects. Reproducibility
is the developer-experience feature: a change is tested against a real contract, rather than against the accidental
state of one machine.

### The online developer environment

So we built the environment in the cloud too. The same Rust processes, the same swappable seams, spun up **online** — so
the loop follows me anywhere, and I barely write the code by hand at all. **Agents** do the typing; I review.

---

Here is the final chapter. We took that same environment — the same binaries, the same trait-swapped seams, the same
one-command setup — and made it run in the cloud, so a developer or an agent can spin up a full Navigator loop without a
laptop that has anything special on it. GKE Autopilot runs the production shape so code integration stays seamless: the
cloud environment and the local environment are the same shapes with the plugs swapped. And because the repo teaches an
agent the same conventions it teaches a human — every task an issue, PR, or review; every template past the guardrails;
every change linted and tested — I increasingly do not write the code by hand. I describe the work, the agent drafts it
in an environment that already knows the rules, and I review the pull request the way I review a contract: line by line.

### "It works on my machine in the cloud — and on yours, and on my client's"

The phrase, finally earned. **It works on my machine in the cloud. It works on your machine. It works on my client's
machine** — because the environment is swappable seams, pinned services, and durable runs, not a special box.

---

The callback lands, and the curse becomes the product. "It works on my machine" is no longer a shrug — it is a
guarantee, because there is nothing special about the machine anymore. It works on my machine in the cloud, because the
cloud environment is the same processes and the same seams. It works on your machine, because you plug the same traits
into your own vendors — your DocuSign, your storage, your identity, your Restate. And it works for my client, because
GKE Autopilot runs the production shape while the store and object storage are swapped for their production-grade
equivalents, seamlessly. A fork can rebrand it and it still works. The Rails room said "it works on my machine" as an
apology; fifteen years, one foundation-governed language, and a law practice later, we get to say it as a promise. That
is what going all in on Rust bought a two-person firm: an environment that is not special anywhere, so it works
everywhere. Read the code, and hold us to it.

### Where we are today — live by August 20

Every seam now has its Rust answer, decided and recorded: **SurrealDB** (Surreal Cloud) for the store, **Restate Cloud**
for durable execution, **OpenObserve Cloud** for telemetry, **Stalwart** Managed Email for every matter address, **Miuda
PBX** on RustPBX for voice, **Pingora** at the private edge, **Garage** and **Rauthy** in the local loop, **Regorus**
for policy, **Dioxus** for every view, **Typst** for every document. Running by **August 20, 2026**.

---

This is where we are today — not a wish list, a decision list. Each of these is a recorded owner decision in the
repository's issues, grounded in a spike or a source read, with its covering tests and cutover gates named before any
code lands. The store is SurrealDB with Surreal Cloud in production and an in-memory engine in every test. Durable
execution stays Restate, now as Restate Cloud per deployment. Telemetry lands in OpenObserve — one Rust binary locally,
their cloud in production. Every matter gets an email address on a dedicated subdomain served by Stalwart, inbound and
outbound, with every message archived to the Iceberg lake. Voice is Miuda's commercial build of RustPBX — the IVR is an
Axum webhook in this workspace, not a dialplan file. Pingora guards the private edge behind a per-org tailnet — and
because the tailnet is the edge, a Project developer's browser reaches the API only from a machine already on it, so
even the cross-origin grant rides the VPN, while production, which has no such edge, hands out none. Regorus evaluates
our policies in-process, and Dioxus and Typst render every screen and every document. And we say the edges plainly,
because the commercial-relationship slide taught us to: Xero invoices, DocuSign signs, a SIP trunk carries the calls,
Google Workspace holds the humans' mail. Those are relationships at the boundary — all software in the path is Rust. The
date is real and we are saying it out loud: this stack runs by August 20, 2026. Hold us to that too.

### Betting on ourselves — Rust in peace

Choosing these vendors is **betting on ourselves**. If we write successful Rust, our vendors write successful Rust — a
**healthy ecosystem** that stays usable for many years. That lets the firm focus on **winning cases**, and lets me
**Rust in peace**: no longer a full-time developer, standing on the shoulders of giants.

---

Here is the closing thought, and it is the whole talk in one move. Every vendor on that slide is a company that made the
same bet we did: build the serious thing in Rust, stay current, publish the crate. So choosing them is not outsourcing —
it is betting on ourselves twice. The same language, the same toolchain, the same crates.io; when we write successful
Rust, the ecosystem that makes our vendors successful gets stronger, and when they succeed, the foundation under us gets
firmer. That is what a healthy ecosystem is — not a moment of hype, but an environment that compounds and can be used
for many years to come. And the payoff is the point of the whole firm: that stack runs so we can focus on winning cases.
Early in this talk I gave a eulogy for my programming career. Here is where it rests: I no longer have to consider
myself a full-time developer, because I am building on the shoulders of these giants — the foundation that governs the
language, the maintainers who ship the crates, the vendors who run the clouds. The title was the plan all along. Rust in
peace.

## Wrap Up

### Take the method home

The ask is not a star on a repository — it is the method, and every piece of it is a crate you can pull tonight. **Read
the law. Write the behavior first. Keep the domain seam in Rust. Put a human at the gate.**

> Read [what we built and why](/navigator) for the longer version — then come find me afterward and let us get crabby
  together.

---

Here is the real ask, and it is smaller than a star and bigger than a link: take the method. Read the law at its source
before you write the test. Write the behavior before you write the code. Keep the domain seam in one language long
enough that the reuse starts compounding. Put a licensed human at a gate the state machine cannot route around. None of
that needs our repository. It needs `axum`, `cucumber`, `restate-sdk`, and the discipline to run the checks you already
have on every change you ship.

And here is the part I actually want from a Rust room. Tell me where this leaks. File the bug against the crate we both
depend on. Sponsor the maintainer whose library is holding your production up. A two-person firm can run a litigation
practice on this stack only because thousands of people kept the ecosystem healthy, so the way to pay that back is
upstream, not at us. A language governed by a non-profit taught us that the commons gets stronger when more people show
up. Come find me afterward. Let us get crabby together.
