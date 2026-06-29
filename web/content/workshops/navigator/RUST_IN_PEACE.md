# Rust in Peace

*How we use Rust to improve access to justice* — a [Neon Law Foundation](/foundation/mission) talk for [Rust
NYC](https://www.meetup.com/rust-nyc/).

This talk comes from the Foundation itself — the 501(c)(3) that stewards Neon Law Navigator, the open-source harness
behind a law firm that drafts, checks, and files routine legal work. We started as software engineers. We became
lawyers. We kept writing Rust — one Cargo workspace, every executable and library in a single language. The title's joke
is also the thesis: **Rust in Peace** is a eulogy for the programming career I thought I had to keep separate from
practicing law. Going all in on Rust lets that career die peacefully, because the open ecosystem now carries the
production-grade pieces a tiny legal team could not build alone.

The goal of the half hour: show how that compounding works in one real repository. We create **deterministic workflows
from law**, and a language that is *widely available* — free, permissively licensed, governed by a non-profit — lets a
small nonprofit build them with the same first-class tooling the largest companies run on. Every code block below is an
exact copy from the repository, kept honest by a test that fails the build if a slide drifts from the source. The steps
are the talk, beat by beat; the "Copy as Markdown" button hands you the whole thing to take home.

## Agenda

An agenda, not a lecture outline — you are here to argue back. By the end of the half hour you will be able to:

- **Recount** how a two-person team crossed from software to law without dropping the toolchain. **Explain** why a
  foundation-stewarded language is access-to-justice infrastructure. **Trace** our process from the law, to a Cucumber
  feature, to a reusable template, to the signed notation it produces for one client. **Dissect** one workflow — forming
  a Nevada LLC — into attorney-gated steps with the shipped code. **Map** the Rust ecosystem we rely on: Restate,
  OpenTelemetry, Arrow/Parquet/Iceberg, LSPs, and Zed — plus Solana, where we are headed but have not shipped yet.
  **Defend** the claim that a reviewed, repeatable workflow beats a prompt. **Decide** whether to fork, white-label,
  read the code, and star it.

---

We frame this as an agenda rather than a lecture outline — you are here to argue back, not to be tested. By the end of
the half hour you will be able to: **Recount** how a two-person team crossed from shipping software to practicing law
without dropping the toolchain that got them there. **Explain** why a language stewarded by a non-profit foundation —
free to every clinic, student, and solo practitioner — is access-to-justice infrastructure, not just an engineering
preference. **Trace** our process from the law itself, to a Cucumber feature, to a reusable template carrying a
questionnaire and a workflow, to the client-specific notation that gets reviewed and signed. **Dissect** one legal
workflow — forming a Nevada LLC — into small, modular, attorney-gated steps, and read the exact shipped code behind each
one. **Map** the ecosystem choices that let a small legal team act bigger than it is: Restate for durable execution,
OpenTelemetry for privacy-preserving operations, Arrow/Parquet and Iceberg for the lake, Solana for the attestations we
plan but have not shipped, `navigator-lsp` in Zed for legal docs as code. **Defend** the claim that a reviewed,
repeatable workflow beats asking an LLM with a prompt, because steps in a prompt are neither repeatable nor modular.
**Decide** whether to fork, rebrand, read the code, and — if it earns it — star it before you leave.

## A eulogy for my programming career

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

## Go all in, or the interest does not compound

The bet is not "write a service in Rust." It is Rust all the way down: vendors, libraries, command-line tools, workers,
editor integrations, and tests. The payoff is that each good decision reinforces the next one.

---

The core claim for a Rust room is stronger than "Rust is pleasant" or "Rust is safe." The claim is that going all in
changes the economics of a small team. A CLI can call the same validators as the website. A workflow worker can share
the same state-machine types as the template authoring rules. The LSP can surface those rules inside Zed while a lawyer
writes a document. A release can ship one dated set of binaries, images, and editor assets. The ecosystem compounds only
when the seams stay in the same language long enough to reuse them.

That is why the repository is Rust-only by design. Every executable and library is Rust; `navigator` orchestrates the
machine-bound flows; there are no shell scripts and no Makefile. We do not win because we wrote more code than everyone
else. We win when the code someone else wrote in the Rust ecosystem becomes a trustworthy brick in our law practice.

## Widely available — governed in the open

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

## A cautionary tale — Java, Oracle, and the price of a single owner

Java was created at Sun; Oracle acquired Sun in 2010 and sued Google that same year over Android's reuse of 37 Java SE
API packages. *Google v. Oracle* ran **more than a decade**, until the [Supreme Court ruled in
2021](https://en.wikipedia.org/wiki/Google_LLC_v._Oracle_America,_Inc.) it was **fair use**.

---

The contrast that makes the case is a matter of public record. The point for this audience is not which side was right.
The point is the *exposure*. When a language and its APIs are owned by a single company, the terms under which you build
on it are one acquisition or one lawsuit away from changing. A foundation-governed language with a permissive license
(Rust ships under MIT OR Apache-2.0) removes that entire category of risk. With Rust, the eleven-year question simply
never gets asked.

## The goal — deterministic workflows from law

The whole method on one slide: a prompt is a wish; a workflow is a contract. Read the law → a Cucumber feature → a
**template** (the reusable blueprint) → a **notation** (one client's reviewed, signed result).

---

Our process begins by reading the law itself. We translate what the law requires into Cucumber features — executable
behavior, written before any code. Then we express the work as a template: one markdown file whose frontmatter carries a
questionnaire (the questions a client answers) and a workflow (the state machine the matter walks). When a client
engages us, Navigator creates a notation from that template — one client's answers bound to one workflow run — and every
notation passes a staff review owned by the attorney who is the matter's directly responsible individual before anything
leaves the building.

The rest of this talk dissects one real workflow — forming a Nevada LLC, our Neon Law Nest product — into its small,
modular steps, one slide per step, with the exact shipped code behind each. Exact means exact: a test compares every
snippet on these slides against the file it cites and fails the build on drift.

## Step 1 — read the law

A Nevada LLC is a creature of statute: NRS Chapter 86 says what the Articles of Organization must contain. We do not
paraphrase from memory — our `statutes` crate scrapes the NRS weekly into Postgres, rendered publicly at </statutes>.

---

NRS Chapter 86 says what the Articles of Organization must contain, who can be a registered agent, and what the
Secretary of State will accept. We do not paraphrase the law from memory — our `statutes` crate scrapes the Nevada
Revised Statutes weekly and reconciles them into Postgres, and the same text we cite is rendered publicly. The law is
the upstream; everything below is a faithful translation of it.

## Step 2 — write the behavior before the code

The first artifact is not Rust — it is a Cucumber feature describing the whole arc in plain language, runnable as a
test: a founder intakes, an attorney reviews, signatures land, and the state stamps a filing.

From `features/tests/features/nest_formation.feature`:

```gherkin
  Scenario: From intake to a stamped Secretary-of-State filing
    When the firm opens the "nv__llc_formation" matter for the client
    And the founder answers the formation questionnaire:
      | value                  |
      | Libra                  |
      | libra@example.com      |
      | Bright Star Ventures   |
      | Neon Law Registered Agent |
      | members                |
      | Libra; 1 Main St; Las Vegas; NV; 89101; USA |
      | 2026-07-01             |
    Then the formation reaches the signature wait
    And the persisted packet is the official SoS form carrying the founder's answers
    When the attorney files the Articles with the Nevada Secretary of State
    Then the formation workflow reaches END
    And a filing was recorded with the "Nevada Secretary of State"
    And the founder's seven onboarding answers are on file
```

---

This is the law translated into expected behavior. The [`cucumber`](https://docs.rs/cucumber) crate runs this scenario
against a real Postgres on every `cargo test`. The feature is the contract; the code below exists to make it pass.

## Step 3 — the template: a questionnaire and a workflow

The template is one markdown file with two machine-readable graphs: a questionnaire graph (what we ask) and a workflow
graph (what we do). The Nest questionnaire is seven answers, in order.

From `templates/forms/united_states/nevada/state/nv__llc_formation.md`:

```yaml
questionnaire:
  BEGIN:
    _: client_name
  client_name:
    _: client_email
  client_email:
    _: entity_name
  entity_name:
    _: registered_agent
  registered_agent:
    _: management_structure
  management_structure:
    _: managing_members
  managing_members:
    _: formation_date
  formation_date:
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
    articles_rendered: staff_review
  staff_review:
    approved: document_open__articles_pdf
    rejected: END
  document_open__articles_pdf:
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
filing state and the same shape closes an estate plan or sends a debt-collection letter — the steps are modular because
the states are vocabulary, not prose. No branching is needed for a simple formation.

## Step 4 — the attorney gate is a graph invariant

Every workflow must pass through `staff_review` before anything is signed or filed — not as a policy memo, but as a
property checked over the state-machine graph itself with a breadth-first search from `BEGIN`.

From `workflows/src/guardrail.rs`:

```rust
pub fn staff_review_precedes_signature(spec: &WorkflowSpec) -> Result<(), GateViolation> {
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

## Step 5 — signature is a modular step

`sent_for_signature__pending` is one state in the graph, and the thing that fires it is a small trait — not a vendor.
DocuSign is the shipped implementation; dev and tests run a recording stub, so the step stays testable without an
account.

From `web/src/signature.rs`:

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

From `web/src/retainer_walk.rs`:

```rust
    // Idempotency: this notation already has an envelope out. Reuse the
    // persisted id, fire nothing, send nothing — the post-state is
    // whatever the notation already records.
    if let Some(existing) = notation_row.signature_request_id.clone() {
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

## Step 6 — the filing, run durably

The last state, `filing__nv_sos`, records the filing with the Nevada Secretary of State — and like every long-running
step it executes as a journaled, resumable [Restate](https://restate.dev/) workflow through the
[`restate-sdk`](https://docs.rs/restate-sdk) crate.

---

A workflow that survives a pod restart and replays to exactly where it left off used to be big-company infrastructure;
in Rust it is a dependency line. The same durability runs our nightly archive: a law firm carries a ten-year retention
duty, so every night we snapshot Postgres into [Parquet](https://docs.rs/parquet) via [`arrow`](https://docs.rs/arrow) —
the open columnar format the Iceberg lakehouse world builds on. And when a matter one day calls for an on-chain record,
the door is already open: Solana programs are written in Rust, so the same workspace can speak to the chain natively —
not shipped yet, and we will say so plainly until it is.

## Why a workflow beats a prompt

You could ask a frontier model to "form me a Nevada LLC" and get something plausible. We built the harness instead,
because plausible is not the bar — repeatable is. A prompt's steps are neither repeatable nor modular.

---

The same words produce different documents on different days, so a prompt's steps are not repeatable. You cannot swap
its signature vendor, test its review gate, or prove its filing fired exactly once, so a prompt's steps are not modular.
A workflow gives you all of that, plus the thing no model can supply — a licensed attorney, the DRI, reviewing every
notation at a gate the graph cannot route around.

Our goal is to create as many of these reliable, attorney-vetted workflows as possible for our customers. Each one is
concise automation for one real legal outcome — formation, estate plan, debt-collection defense — and each new template
reuses the same states, the same guardrails, and the same review gate. That is how the floor rises: not a bigger model,
but a longer shelf of workflows anyone can read, run, and extend.

## Privacy-preserving operations are part of the product

Legal tech cannot treat observability as a copy of production. Navigator emits OpenTelemetry traces, metrics, and logs
through one Rust crate, and the rule is structural: identifiers and counts, never client content.

---

The repository has one telemetry seam: every binary calls `telemetry::init`, and production exports OTLP to a collector
while dev and CI fall back to human-readable stdout. The interesting part is not the exporter. The interesting part is
the trust boundary. A `notation_id`, a workflow service name, an outcome, a duration, a status code — yes. A client
name, an answer body, an email address, a document body — never. The OpenTelemetry collector runs a fail-closed
redaction processor, and the Rust code keeps request bodies out of spans in the first place.

The analytics story follows the same pattern. Operational telemetry goes to BigQuery through structured logs and OTLP.
Matter data is archived separately by the nightly Restate `Archives` workflow: Postgres snapshots become Parquet through
`arrow` and `parquet`, with Iceberg metadata being added so the lake becomes time-travelable instead of a pile of files.
The ecosystem point is the same as the law point: the boundary is written down, enforced, and testable.

## Rust reaches the places lawyers work

Navigator is not only a web app. The same rule engine runs as a CLI and as an LSP, so legal documents can be written
like code in Zed — an editor written in Rust — with diagnostics and fix-all actions as the lawyer types.

---

This is where "available in many places" stops being abstract. The workspace builds a website, a Restate worker, trigger
jobs, a `navigator` CLI, an MCP server, and `navigator-lsp`. The LSP speaks JSON-RPC over stdio, has no telemetry, and
attaches to Markdown. Ordinary prose gets Markdown rules; templates get the notation rules on top, because a legal
template is both a document and a program. Zed can install the published Navigator LSP extension and pull the matching
release binary automatically.

That matters because the artifact lawyers already care about — the legal document — becomes the artifact the toolchain
understands. Git shows the diff. The LSP shows the rule violation. The CLI validates the same file in CI. The website
turns it into a notation. There is no copy-paste seam where the law stops being source code.

## Built to be forked, rebranded, and improved with agents

Neon Law Navigator powers our practice, but it is designed so another firm can run it under its own name. White-labeling
and AI-assisted development are not side quests; they are how open work compounds.

---

The code is open under MIT OR Apache-2.0. The names and marks are not, because clients should not be confused about
which lawyers stand behind which software. That is why the product has a rebrand seam: a fork can become another firm's
Navigator without pretending to be Neon Law. The Foundation's job is to make that path boring enough that a clinic, a
solo, or another mission-driven firm can adopt the harness instead of starting from a blank repository.

And yes, we use agents. Not as a replacement for review, but as a way to capture compound interest. The repo teaches the
agent the same rules it teaches humans: every task is an issue, PR, or review; every template must pass the workflow
guardrails; every Markdown change runs the Navigator validator; every public UI change gets a walkthrough artifact. The
vibe is useful only when it lands back in durable structure — tests, docs, workflow states, and code a future lawyer can
read.

## The crates we actually run on

The bill of materials for a real legal-tech product — every line a crate you can pull today:

- **HTTP and views** — `axum`, `maud`, `tower` / `tower-http`. **Async runtime** — `tokio`, multi-threaded, with
  graceful shutdown. **Database** — `sea-orm` over Postgres, `uuid` (v7) + `chrono` for keys and timestamps. **Durable
  execution** — `restate-sdk`. **Telemetry** — `opentelemetry`, `tracing`, OTLP. **Archive** — `arrow`, `parquet`,
  `iceberg`. **Content** — `pulldown-cmark`. **Cloud** — `google-cloud-storage` + `reqwest`. **Identity** —
  `jsonwebtoken` + `oauth2`. **Tests** — `testcontainers`, `fantoccini`, `cucumber`.

---

So you can map this onto your own stack: for **HTTP and views**, [`axum`](https://docs.rs/axum) for the router and
handlers, [`maud`](https://docs.rs/maud) for compile-time-checked HTML, [`tower`](https://docs.rs/tower) /
[`tower-http`](https://docs.rs/tower-http) for the middleware stack. The **async runtime** is
[`tokio`](https://docs.rs/tokio), multi-threaded, with signal handling for graceful shutdown. The **database** is
[`sea-orm`](https://docs.rs/sea-orm) over Postgres with [`sea-orm-migration`](https://docs.rs/sea-orm-migration) for
schema, and [`uuid`](https://docs.rs/uuid) (v7) + [`chrono`](https://docs.rs/chrono) for keys and timestamps. **Durable
execution** is [`restate-sdk`](https://docs.rs/restate-sdk) hosting every workflow on one worker endpoint, with the
journal doing the remembering. **Telemetry** is [`opentelemetry`](https://docs.rs/opentelemetry) and
[`tracing`](https://docs.rs/tracing), with OTLP as the export seam. The **archive** uses
[`arrow`](https://docs.rs/arrow) + [`parquet`](https://docs.rs/parquet), with Iceberg metadata coming in through the
[`iceberg`](https://docs.rs/iceberg) crate, to turn the nightly Postgres snapshot into open columnar tables. **Content**
is [`pulldown-cmark`](https://docs.rs/pulldown-cmark), which renders the very markdown you are reading right now.
**Cloud** is [`google-cloud-storage`](https://docs.rs/google-cloud-storage) behind a storage trait, with
[`reqwest`](https://docs.rs/reqwest) for the REST plumbing that provisions a fresh project. **Identity** is
[`jsonwebtoken`](https://docs.rs/jsonwebtoken) and `oauth2` for the OIDC flow. **Tests** use
[`testcontainers`](https://docs.rs/testcontainers) for a real Postgres per test binary,
[`fantoccini`](https://docs.rs/fantoccini) to drive a real browser over WebDriver, and
[`cucumber`](https://docs.rs/cucumber) for the behavior specs you saw in Step 2. One workspace, one `cargo test`, one
language from the HTTP handler down to the migration and back up to the browser assertion. None of these crates asked us
to sign anything.

## Ethics is part of the stack

Lawyers who code still carry the rules of professional conduct, and the engineering answer is the same as it is for
memory safety: make the invariant structural, not aspirational.

- **Scope is a field, not a vibe.** Every engagement is scoped in writing before work starts. **The conflict check runs
  first.** Before any matter opens, we query every current and former matter. **Referral, without a referral fee.** When
  conflicted out, we refer — with no referral fee.

---

**Scope is a field, not a vibe.** Every engagement is scoped in writing before work starts. When staff open a matter,
its scope narrative is seeded as the first clause of the retainer the client signs — for a flat-fee product like
**Northstar** (estate planning) or **Nautilus** (the debt-collection shield), the agreement states exactly what the fee
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

## Review the code, star the repo

Neon Law Navigator is open source under MIT OR Apache-2.0 — the same permissive, foundation-aligned licensing we just
argued for. The ask: open the repository, read the code, push on it, and — if the work earns it — **star it**.

> Read the code and star it:
  **[github.com/neon-law-foundation/navigator](https://github.com/neon-law-foundation/navigator)** — and read the
  [Foundation mission](/foundation/mission) for why any of this matters.

---

The Foundation opened it because building legal tooling in the open is how the floor of competence rises for the next
lawyer, the next clinic, and the next engineer who decides to cross over. So here is the ask, and it is a real one: open
the repository, read the code, and push on it. File an issue when our abstractions leak. Send a pull request when you
can do it better. And if the work earns it, **star the repository** — not because a star makes us "the best," but
because visibility helps the people who need a firm that ships its work in the open actually find one. A language
governed by a non-profit taught us that the commons gets stronger when more people show up. Same principle here. Come
build with us.
