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

### Las Vegas Ruby Group

![The Las Vegas Ruby Group's red ruby profile mark](img/lvrug/lvrug.png)

---

Began programming here. Very grateful for Paul, David, Ryan, Alex, Jeremy, Russ, Jason, Rachel, Brian, Dylan and so many
more.

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

### Neon Law Navigator

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

## Why go all-in on rust

### Compiled, correct, and fast

It takes time to build in Rust, but it's worth it.

---

If we're vibing, why not just vibe in Rust?

### Amazing Community

What other compiled language has meetups this big? And a discord community of thousands.

---

Also perhaps other communities like JavaScript and Python are too big and subsequently fragmented.

### Mac, Windows, Linux

Rust works on all three.

---

You can download our `navigator` CLI on each cloud.

### snake_case > camelCase

rob_balicki or robBalicki

---

I'll die on this hill.

## The tour: {Library,Data,DevX,Cloud}

### The crates we actually run on

Read our [Cargo.toml](https://github.com/neon-law-foundation/navigator/blob/main/Cargo.toml).

- **HTTP and views** — `axum`, `dioxus`, `tower` / `tower-http`.
- **Async runtime** — `tokio`, multi-threaded, with graceful shutdown.
- **Store** — `surrealdb`, one engine for document, graph, and key-value.
- **Durable execution** — `restate-sdk`.
- **Telemetry** — `opentelemetry`, `tracing`, OTLP.
- **Archive** — `arrow` and `parquet`; a real Iceberg table lane is separately tracked.
- **Content** — `typst`.
- **Cloud** — `google-cloud-storage` + `reqwest`.
- **Identity** — `jsonwebtoken` + `oauth2`.
- **Tests** — `fantoccini` and `cucumber`, exercised through the workspace harness.

---

It's easier thinking in only one language.
