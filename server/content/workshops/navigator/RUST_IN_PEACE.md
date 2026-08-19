---
kind: workshop
title: Rust in Peace
description: Rust in Peace tours the Rust monorepo behind a litigation-first law practice.
---

# Rust in Peace

A talk by [Nick](mailto:nick@neonlaw.com) for [Rust NYC](https://www.meetup.com/rust-nyc/events/316056830/)

Rust in Peace is a eulogy to my programming career. Over the past year, teaching coding at Apple, I realized nearly all
my students were vibe coding and had stopped needing me. That was the signal to go back to practicing law.

Our firm runs its entire practice on the [Neon Law Navigator](/navigator), Rust-built software that flexes its great
ecosystem. We built our practice to enable all lawyers to vibe-code custom client experiences and tell our clients'
unique stories. In this presentation, you'll see how this all fits together beginning with how we architected the data,
chose Rust-first vendors, and leverage agentic tooling for coding and lawyering.

Navigator is only possible because the ecosystem is this good, because the LLMs are this good, and because communities
like RustNYC are this good. Please come to my retirement party and let us get crabby together.

## Intro

### May my soul rust in peace

![Ferris the Rust crab in goggles holds a glowing vial, Megadeth Rust in Peace parody](img/rust-in-peace/cover.png)

---

Rust in Peace is a Eulogy to my programming career, as I know I'll never see myself as a full-time developer again.

### 5 Years at the Fruit

![A hand holds a kiwi in front of a colorful rainbow sculpture](img/rust-in-peace/kiwi-rainbow.jpg)

---

Twenty days in New York. Thank you to Rob, my friend of twenty years.

### 4 years finance data & platform engineering

![Ten colleagues standing together in an Apple office](img/rust-in-peace/apple-team.jpg)

---

Over 30 petabytes of data, high stress but high performing team.

### 1 year teaching programming

![Colleagues share dinner together at a restaurant](img/rust-in-peace/apple-teaching.jpg)

---

The man in the front is Owen, my beloved boss. Apple is an incredible place.

### What I realized is

![Groovy card: asterisk everyone loves vibing; nearly](img/rust-in-peace/everyone-loves-vibing.png)

---

Nearly everyone loves vibing.

### Decided to be a lawyer in New York

![New York City seen beyond a hillside cemetery](img/rust-in-peace/new-york-lawyer-decision.jpg)

---

I decided to be a lawyer in New York.

### What our firm does

{{firm-product-cards}}

---

Our firm offers Fractional CTO, Litigation, Fractional GC, and one-time services.

### NeonLawNavigator

{{navigator-product}}

---

Neon Law Navigator is Affero licensed software designed to enable all lawyers to be vibe-coding storytellers.

### Agenda

An agenda, not a lecture outline — you are here to argue back. By the end of the half hour you will be able to:

- Why go all-in on Rust
- {Library,Data,DevX,Cloud} tour
- Navigator: Everyone vibes
- Notations: Markdown => PDF with workflows
- Thanks

---

We begin with our experience, we end rusting in peace.

### Las Vegas, 2011 — where I learned to program

![The Las Vegas Ruby Group's red ruby profile mark](img/lvrug/lvrug.png)

---

Very grateful for Paul, David, Ryan, Alex, Jeremy, Russ, Jason, Rachel, Brian, Dylan and so many more.

## Why go all-in on rust

### Compiled, correct, and fast

It takes time to build in Rust, but it's worth it.

---

If we're vibing, why not just vibe in Rust?

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

## The tour: {Library,Data,DevX,Cloud}

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

### The simplest developer environment I can get away with

The local loop is production-shaped on purpose: the Rust CLI provisions an isolated KIND dependency tier and the host
application process shares that exact environment.

---

This is not a claim that every dependency is a process on a laptop. The CLI owns the local lifecycle, gives each
worktree its own ports and database, and uses the same dependency topology that the application expects. Reproducibility
is the developer-experience feature: a change is tested against a real contract, rather than against the accidental
state of one machine.

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

## Navigator: everyone vibes

### Rust reaches the places lawyers work

Navigator is not only a web app. The same rules meet lawyers in the editor they already work in, and the same AIDA
catalog is available to assistants through MCP and A2A.

---

This is where "available in many places" stops being abstract. The workspace builds a website, a Restate worker, trigger
jobs, a `navigator` CLI, an MCP server, and `navigator-lsp`. The LSP speaks JSON-RPC over stdio, has no telemetry, and
attaches to Markdown. Ordinary prose gets Markdown rules; templates get the notation rules on top, because a legal
template is both a document and a program. Most drafting happens in the WYSIWYG editor on the website; the LSP is what
carries the identical rules into the git side of the work, where the pull request and the GitHub Action see exactly what
the author saw.

Reaching those places is an arrival, not a claim that the migration has already finished. The AIDA catalog lives in
mcp/src/tools and exposes the same grounded operations through A2A and MCP, which is the one seam an assistant comes
through — there is no synthesized filesystem behind it, and long-term files stay in the object storage Navigator hosts.
That is the direction: a lawyer keeps working with documents while the system keeps the shape, the workflow, and the
rendered PDF aligned.

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

## Notations: Markdown => PDF with workflows

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

## Wrap Up

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
