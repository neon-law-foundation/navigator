# Contributing

**Neon Law Navigator is open source, and it is currently closed to outside contributions.**

The Neon Law Foundation produces this software; Shook Law PLLC, trading as Neon Law, operates it. Issues and pull
requests from outside those two organizations are not being accepted right now. This is a capacity decision rather than
a licensing one: the software runs a live legal practice, every change to it needs review by someone who can weigh the
practice consequences, and there is not review capacity to offer an outside contributor today.

**Write to [contact@neonlaw.org](mailto:contact@neonlaw.org).** Anyone is welcome to — a bug you hit, a security
concern, a fork you are running, a question about the licences, or an interest in contributing when this reopens. The
address is read by people, and a report that never becomes a pull request is still worth sending.

The licence is a separate question, and it is open. The software is dual-licensed under MIT or Apache-2.0 at your
option, and the drafted legal prose under `templates/` is CC BY 4.0. You may run, fork, modify, and redistribute this
software, with no permission to ask for. See [`LICENSE.md`](LICENSE.md) and [`docs/licensing.md`](docs/licensing.md).

## How contributions are licensed

The terms are stated here so they are knowable in advance, and so a fork's own authors know where they stand.

Contributions are **inbound = outbound**: anything submitted for inclusion is licensed `MIT OR Apache-2.0` on the same
terms the project ships under, as described in section 5 of the Apache-2.0 licence. Contributions to `templates/` carry
`CC-BY-4.0` instead, matching what that content ships under. If a change adds a blank government form, note that the
Foundation licenses nothing in the agency's own PDF — only the catalog card, field map, and workflow beside it.

You keep the copyright in what you write. There is no contributor agreement to sign, no copyright assignment, and no bot
standing between an author and a merge.

Work by the Neon Law Foundation's personnel and contractors assigns to the Foundation under the employment or contractor
agreement each of them already holds. That is an arrangement between the Foundation and its own people; it changes
nothing about the terms above.

## What a contribution to a legal-practice repository is not

Two boundaries hold regardless of the licence, and they are why the review bar is what it is. The Foundation produces
the software, but Neon Law runs a live practice on it, and both facts land on every change.

**No client data, ever.** Shipped data contains only firm-owned or synthetic identities; non-firm email addresses use
reserved example domains and phone numbers do not ship. The workspace test suite enforces this on every pull request,
and that gate is the confidentiality boundary.

**Legal template bodies get attorney review.** A change to anything under `templates/` alters a document a real client
may sign, so a licensed attorney reviews it before it merges regardless of how mechanical the diff looks.

Neither the licence nor a merged pull request creates an attorney-client relationship with Shook Law PLLC, and nothing
in this repository is legal advice.

## Working in the tree

For anyone reading the code or running a fork: follow the [workspace layout](docs/workspace-layout.md). Rust owns the
domain and machine-bound flows, and the browser surface through Dioxus. Generated PDFs use Typst and transactional email
uses string templates. Every change is test-driven — the covering test lands with the minimal implementation it proves —
and `cargo fmt`, `cargo clippy` with warnings denied, and `cargo nextest run --workspace` all have to pass before
review.
