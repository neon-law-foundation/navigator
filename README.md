# Neon Law Navigator

Neon Law Navigator is the open-source monorepo behind [neonlaw.com](https://www.neonlaw.com) — Neon Law's website, the
Neon Law Foundation's, and the software that delivers legal services. It is produced by the **Neon Law Foundation**, a
501(c)(3) nonprofit, and operated by Neon Law, the law firm. It combines versioned legal Notations, durable workflows,
attorney-reviewed automation, client and lawyer portals, the `navigator` CLI, and AIDA's agent tools.

It serves the lawyers, operators, and engineers who turn repeatable legal work into accountable client service. The
system keeps the lawyer as the actor while giving each matter a consistent intake, review, document, filing, and audit
path.

Navigator exists to make high-quality legal services easier to operate and more accessible without separating the public
mission from the delivery system that supports it. Neon Law practises consumer law on published flat fees; this is the
software that makes that economics work, and it is public so that anyone else can run it too.

Start with the [glossary](docs/glossary.md), use the [documentation index](docs/index.md) to find the narrow source of
truth, and follow <AGENTS.md> for local development and contribution workflows.

## License

The software is dual-licensed under either

- the [MIT license](LICENSE-MIT), or
- the [Apache License, Version 2.0](LICENSE-APACHE)

at your option: `MIT OR Apache-2.0`. Read it, build it, fork it, and redistribute it.

**The legal prose carries a different licence.** The notation bodies under `templates/` — retainers, wills,
questionnaire prompts — are [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/): adapt and redistribute them
freely, commercially included, on attribution alone. A software licence's obligations describe nothing a will template
does. The blank government PDFs under `templates/forms/` are the issuing agency's work, and the Foundation licenses
nothing in them.

See <LICENSE.md> for why the three differ, [`docs/licensing.md`](docs/licensing.md) for why this software is published
at all, and <CONTRIBUTING.md> for how contributions are licensed.

Copyright (c) 2026 **Neon Law Foundation**.

## Trademarks

Copyright and trademark sit with different organizations here, and the distinction is the one a fork needs. The **Neon
Law Foundation** produces this software and holds the copyright, publishing it under the licences above. **NEON LAW** is
a registered trademark, U.S. Reg. No. 6,325,650, owned by **Shook Law PLLC**, the law firm that operates the software
under the mark. The licence grants rights in copyright, not in trademarks.

Run it, fork it, redistribute it, and say your work is built on Neon Law Navigator. Do not present your deployment as
Neon Law: a law firm's mark is how a client identifies who is accountable for their legal work, so a fork trading as
Neon Law would misdirect exactly the person least able to check. Rename your deployment through the brand manifest
(`views::brand_bundle`) rather than by editing sources.

The Neon Law Foundation uses the mark for its charitable, pro bono, and public-education work under separate written
permission from the Firm.

## No legal advice

This repository is not legal advice, and using it does not create an attorney-client relationship.
