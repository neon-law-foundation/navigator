# Neon Law Navigator license

Copyright (c) 2026 Neon Law Foundation.

SPDX-License-Identifier: MIT OR Apache-2.0

Neon Law Navigator is open source. Two licences apply, because two different kinds of thing live in this repository and
they carry different obligations. A third category is here that the Foundation cannot license at all, and this file says
so rather than leaving a reader to assume.

## Software: MIT OR Apache-2.0

Every line of code — the Rust workspace, the `navigator` CLI, the build and deployment tooling, and the configuration
that ships with them — is dual-licensed under either of

- the [MIT license](LICENSE-MIT) ([SPDX](https://spdx.org/licenses/MIT.html)), or
- the [Apache License, Version 2.0](LICENSE-APACHE) ([SPDX](https://spdx.org/licenses/Apache-2.0.html))

at your option. `SPDX-License-Identifier: MIT OR Apache-2.0`.

This is the ordinary licensing of the Rust ecosystem, and it is deliberate rather than inherited. MIT is the permissive
default a reader recognises on sight; Apache-2.0 adds an express patent grant that MIT does not carry, which matters
more than usual for a law firm publishing legal-automation software — a downstream user would otherwise have to guess at
the patent position of the very entity that wrote it. Offering either alone would give strictly less than the pair does.

## Legal content: CC BY 4.0

The drafted legal prose — the notation bodies under `templates/`, which carry the documents a client signs together with
the questionnaire prompts and workflow definitions in the same files — is licensed under Creative Commons Attribution
International, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). `SPDX-License-Identifier: CC-BY-4.0`.

A software licence is written for software. A retainer, a will template, and an intake questionnaire are drafted
documents, and the obligation that matters for one is attribution: if you publish a document derived from ours, say
where it came from. Otherwise use, adapt, and redistribute them freely, including commercially.

## Government forms: not the Foundation's to license

The blank government PDFs under `templates/forms/`, and the printed matter they reproduce, are works of the issuing
state or federal agency. **The Foundation claims no copyright in them and grants none.** They are committed here so the
binary embeds the same bytes the repository carries, and for no other reason.

What the Foundation does license, under CC BY 4.0 above, is its own material beside each blank form: the catalog card,
the field map, and the workflow that fills the form in.

## Contributions

Unless you state otherwise, any contribution you intentionally submit for inclusion in the work, as defined in the
Apache-2.0 licence, is dual-licensed as above with no additional terms or conditions. Contributions to `templates/`
carry CC BY 4.0 instead, matching what that content ships under. You keep the copyright in what you write.

## Trademarks

The licences above grant rights in copyright, not in trademarks — and here the two sit with different organizations.
Copyright in this work belongs to the **Neon Law Foundation**, which produces it. **NEON LAW** is a registered
trademark, U.S. Reg. No. 6,325,650, owned by **Shook Law PLLC**, the law firm that operates this software under the
mark. That mark, the Neon Law wordmark, and the Neon Law logos are the Firm's and are not licensed here.

You may run, fork, and redistribute this software freely. You may say that your work is built on Neon Law Navigator. You
may not present your deployment as Neon Law, or use the marks in a way that suggests this firm endorses or is
responsible for your service — a law firm's mark is how a client identifies who is accountable for their legal work, and
a fork wearing it would misdirect exactly the person least able to check.

Replace the marks in your own deployment through the brand manifest (see `views::brand_bundle`) rather than by editing
sources; that seam exists for this.

The Neon Law Foundation uses the mark for its charitable, pro bono, and public-education work under separate written
permission from the Firm.

## No legal advice

**Neither licence is legal advice, and neither creates an attorney-client relationship.** A template here is a starting
point drafted for the jurisdictions Neon Law practises in. Running this software does not put a lawyer in your matter,
and adapting a template does not make the result correct for your facts or your state. An attorney-client relationship
with Shook Law PLLC begins only with a signed retainer.
