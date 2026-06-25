---
title: Going All-In on Rust
description: Why Neon Law Foundation chose one language for fast, safe, local-first access-to-justice software.
---

_Nick is a lawyer for Neon Law and volunteer for the Neon Law Foundation_.

![Ferris painted in Neon Law Foundation colors](img/going-all-in-on-rust/ferris-rust-logo-nlf-handpainted.png)

Friends,

We are going all-in on Rust.

Not because Rust is fashionable. Not because it makes us feel like the toughest engineers in the room. We are going
all-in on Rust because we are trying to build software that can sit with people on the worst days of their lives and
still be fast, boring, correct, and kind.

Cancer changes the way you think about software. So does divorce. So does watching someone you love try to navigate a
system that was not designed for them. You stop wanting cleverness for its own sake. You start wanting tools that open
quickly, run locally, explain themselves, and do not make a vulnerable person wait on a spinner just to find out what
they already wrote down.

That is the feeling underneath this technical decision.

Rust lets us write the Neon Law Navigator once and run it on the machines lawyers actually have: Windows laptops, Linux
servers, cloud containers, little local dev boxes, and whatever comes next. It lets one Cargo workspace hold the CLI,
the web server, the MCP server, the workflow workers, the PDF renderer, the statutes scraper, and the tooling around
notation templates. One language means fewer seams. Fewer seams means fewer places for a tired maintainer to make a
mistake.

That simplicity matters because the thing we are building is bigger than us.

[Neon Law Foundation](/foundation/mission) is a 501(c)(3) non-profit. Neon Law Navigator is open source under Apache-2.0
or MIT, at your option. That choice is not accidental. It is borrowed from the Rust community itself: the official Rust
projects are generally [dual-licensed under Apache-2.0 and MIT](https://www.rust-lang.org/policies/licenses). Dual
licensing is a gesture of trust. It says: take this, build with it, combine it with what you need, and do not let our
paperwork become your wall.

Rust has that same generosity in its governance. The [Rust Foundation describes itself](https://rustfoundation.org/) as
an independent nonprofit committed to Rust's future, and its bylaws say the foundation is organized as a not-for-profit
membership corporation whose purpose includes supporting the Rust Project, cultivating Rust project team members and
user communities, maintaining infrastructure, and stewarding the trademark and other assets. That is not trivia to us.
That is the model.

A language can be infrastructure. A foundation can be infrastructure. A community can be infrastructure.

## Rust in Peace

The talk we are giving at Rust NYC is called [Rust in Peace](/foundation/workshops/navigator/rust-in-peace), and yes,
the title is a little ridiculous. It is also exactly right.

The talk is about using Rust to improve access to justice. It starts from a simple premise: the law is full of
repeatable work, and repeatable work deserves deterministic workflows. We read the law, write an executable behavior
test, encode the intake and workflow in a notation template, run the attorney review gate, and only then let the machine
help with the routine parts.

That is not an AI-first architecture. It is a workflow-first architecture.

We want the City of Light to work without AI and without the Internet. You should be able to sit at a laptop, write a
notation template, validate it locally, and know that the work is grounded in lessons, upon lessons, upon lessons. The
templates should tell you what questions will be asked. The workflow graph should tell you what happens next. The local
validator should tell you whether the shape is sound before anything leaves your machine.

Then, when you connect to the Internet, the lights turn on.

Now the same local foundation can talk to a workflow broker. It can store documents. It can send signatures. It can
schedule reminders. It can expose AIDA over MCP or A2A. It can eventually attest a local record somewhere public when
that is the right tool. But if you never connect, the core promise should still hold: you have a legal workflow you can
read, inspect, validate, and improve.

That is why Rust matters here. It gives us a serious local toolchain without asking us to split our mind across five
languages. It lets the same language power the CLI that checks your template and the server that runs the resulting
workflow. It lets us teach one stack to lawyers, legal-aid technologists, students, and builders who want to help.

## The Community Bet

There is another reason to go all-in.

Rust is becoming a place where serious companies choose to build the hard part. Zed is a Rust-heavy editor built by
people who cared enough about developer experience to start over. Restate is a Rust-heavy durable execution engine
because correctness under failure is the product. Solana programs are developed with Rust tooling too: the official
Solana docs walk developers through creating a Rust project and building a deployable program with `cargo build-sbf`.

We care about those companies succeeding because their success expands our own launch surface. If Zed gets better, our
notation-template authors get a better editor. If Restate gets better, our workflows get a better durability layer. If
Solana's Rust ecosystem gets better, our future attestation seam gets easier to reason about. If OrgStack and other
Rust-first teams prove that small organizations can ship ambitious systems without drowning in glue code, our own choice
gets less lonely.

That is not vendor worship. It is community compounding.

Every Rust company that wins makes Rust documentation better, hiring easier, crates healthier, examples richer, and
debugging less isolating. Every open-source maintainer who chooses the same stack leaves a trail. We get to stand on
those trails, and we owe it to the next team to leave ours visible too.

This is where all three Neon Law Navigator councils meet. The Engineering Council asks whether one language makes the
system clearer to maintain. The Legal Council asks whether the workflow can be reviewed and stood behind. The Client
Council asks whether the person at the door can actually get through it. Rust is not the answer to every question, but
it gives all three councils the same honest starting point: a small, inspectable system that can run here, on this
machine, before it asks the world for help.

## The Cancer Part

The Cancer voice in our councils always asks the question I find hardest to dodge: what does the exhausted person see?

Not the investor. Not the conference room. Not the demo day. The exhausted person.

The person filling out an intake form after chemo. The founder trying to form an LLC after their day job. The parent
trying to understand a deadline after putting a child to bed. The lawyer who wants to help but has forty open loops and
no spare hour to debug a dependency chain.

For them, performance is not vanity. Local validation is not a nerd preference. Memory safety is not an academic virtue.
It is care.

Fast software lowers shame. Safe software lowers fear. Local software lowers dependency. Simple software lowers the
number of people who have to say, "I am sorry, I could not get it to run."

That is the light we are chasing.

## What We Mean by All-In

All-in does not mean Rust is magic. Rust will still make us fight the borrow checker. It will still make us be precise
when we wanted to be poetic. It will still slow down the first draft sometimes.

All-in means we accept that discipline because the product deserves it.

It means new machine-bound tools become Rust binaries or CLI subcommands, not little shell scripts that only one person
understands. It means local workflows, tests, validators, and deployment helpers share one ecosystem. It means we can
run without a network and then become more powerful with one. It means our legal templates stay inspectable, our
automations stay grounded, and our users are surprised by delight because the software is quick, quiet, and safe.

Most of all, all-in means we are choosing a commons.

Rust is great because it is bigger than any one company. Neon Law Navigator has to be bigger than Neon Law too. The firm
can run it in production. The Foundation can steward it in public. Other lawyers can fork it, rebrand it, and make it
their own. That is why the code is Apache-2.0 or MIT. That is why the names and marks are protected but the work is
shared.

We learned that posture from Rust: care enough to build something excellent, then care enough to let people use it.

See you in the light,

Nick
