# Licensing

Navigator is free software. Root [`LICENSE`](../LICENSE) is the licence of record, it is the only licence file in the
tree, and it covers everything the Foundation is able to license:

| What | Licence | Why |
| --- | --- | --- |
| Workspace, CLI, build and deploy tooling | `AGPL-3.0-only` | Copyleft with a network clause, which is how it is run |
| Notation bodies under `templates/` | `AGPL-3.0-only` | Same grant; one tree, one answer |
| Blank government PDFs in `templates/forms/` | None — not the Foundation's | A Nevada state form belongs to Nevada |

One grant and one file is the whole design. A reader never has to work out which instrument governs the file in front of
them, and a fork never has to reconcile two sets of obligations across a directory boundary.

## Who holds what

Two organizations, and the split is the thing a fork actually needs to get right.

| Held | By | Which is why |
| --- | --- | --- |
| Copyright in this repository | **Neon Law Foundation**, a 501(c)(3) | The outbound grant is the Foundation's to make |
| **NEON LAW**, U.S. Reg. No. 6,325,650 | **Shook Law PLLC**, the law firm | The mark is not licensed here at all |

Produce and operate are different verbs and this is the whole point. The Foundation *produces* Navigator — writes it and
publishes it as public infrastructure — which is why the copyright and the grant are its to give. The Firm *operates*
it, running a legal practice on it under the NEON LAW mark, which is why the mark is the Firm's: a mark on legal
services is how a client identifies who is accountable for their legal work, and that accountability belongs to the
entity holding the bar licence. The Foundation itself uses the mark under written permission from the Firm.

The practical consequence: a fork inherits everything the copyright holder can give and nothing the registrant kept.
Copy it, change it, sell it, run it for other people — none of that needs anyone's permission, though § 13 attaches an
obligation to the last one. Calling the result "Neon Law" needs the Firm's permission, and the Firm does not give it.

## The grant: AGPL-3.0-only

Every line of code and every line of drafted legal prose is licensed under version 3 of the GNU Affero General Public
License ([`LICENSE`](../LICENSE)). Cargo and npm manifests declare `AGPL-3.0-only`.

**`-only`, never `-or-later`.** The terms this repository publishes under are the terms in its own licence file. A later
FSF revision may be an improvement, but a law practice does not hand a third party the ability to change the obligations
attached to the software it runs its matters on.

`deny.toml`'s allowlist is a different question and stays permissive. It governs what this workspace is willing to
*consume*, which has nothing to do with how the workspace is licensed out — and the direction matters: every licence on
that allowlist may be distributed inside an AGPL work, whereas the reverse would not hold.

### Section 13 is the reason

§ 13 is what makes this the Affero licence rather than the ordinary GPL, and it is the clause to read before deploying
rather than before forking. Modify Navigator, let users interact with your version remotely over a network, and you must
offer those users the corresponding source of what they are actually using. The obligation attaches to **operating** the
software, not only to shipping a copy of it.

That is not an incidental fit. Nobody downloads a legal-services platform to run it on their own desk; they run it as a
portal for clients. Under a permissive licence, a firm could take this software, improve it substantially, and operate a
practice on those improvements while the public tree stayed where it was. § 13 is what makes the exchange symmetric:
anyone may run a practice on Navigator, and a client of that practice can see the software their matter is being handled
by.

Two things it does **not** do:

- **It does not reach an unmodified deployment.** Running the software as published carries no § 13 source obligation,
  because the corresponding source is already here.
- **It does not reach client data.** § 13 obliges you to publish *your modified software*. A matter, a document, and a
  client's facts are not the software, and nothing in the licence asks for them.

### What a fork owes, in order

1. **Keep the notices.** § 4 conditions the permission to convey on handing every recipient this License along with the
   work, and on keeping the copyright notices intact.
2. **Publish your changes when you convey the work.** § 5 covers conveying modified source; § 6 covers conveying a
   built binary, which must be accompanied by the corresponding source.
3. **Publish your changes when you operate it for others.** § 13, above.
4. **Rename it.** Not a copyright obligation at all — see [Trademarks](#trademarks). The brand manifest
   (`views::brand_bundle`) is the seam.

## Government forms: nobody's to license

The blank government PDFs under `templates/forms/` are works of the issuing state or federal agency. The Foundation
claims no copyright in them and grants none; they are committed so the binary embeds the same bytes the repository
carries, and for no other reason.

This is not a technicality. Claiming a licence over a state's own form would be over-claiming a copyright the Foundation
does not hold, and an over-claim in a licence file published beside a law practice is the kind of error that gets quoted
back. What the Foundation does license beside each blank PDF is its own material: the catalog card, the field map, and
the workflow that fills the form in.

## Why the legal prose is under the software licence

The notation bodies under `templates/` carry the documents a client signs, together with the questionnaire prompts and
workflow definitions in the same files. They are licensed `AGPL-3.0-only` with everything else.

The alternative — a separate attribution licence over the prose — treats a template as a document that happens to sit
near a program. In this tree it is not. A notation body is an input the workflow engine parses, validates against the
`N`-family rules, and renders; the prose, the prompts, and the state machine are the same file, and a rule change and a
clause change arrive through the same review. Splitting the licence at that boundary asks a contributor to work out
which half of a line they are editing, and asks a fork to track two obligations through one file.

The obligation the prose actually needs survives the change: attribution is a subset of what § 4 and § 5 already
require, since a conveyed copy keeps its notices and a modified one says what changed.

## Why open

Neon Law charges published flat fees for consumer legal work. That economics only holds if routine matters cost very
little to run, and this software is what makes them cost little. Publishing it is the same argument as publishing the
prices: a legal system where only the well-resourced can afford counsel is not fixed by one firm being efficient in
private.

Three consequences follow, and they are the trade being made:

- **No trade-secret protection.** Anything published cannot be un-published, so no mechanism in this tree is a secret.
- **The confidentiality boundary is procedural.** A publication path exists, so the no-client-data rule is enforced by
  a load-bearing test on every pull request — see [`agent-workflows.md`](agent-workflows.md#no-client-data-in-the-repo).
- **Forks are expected, and they come back.** Another firm running this software is the point, and under § 13 a firm
  that improves it while operating it for clients publishes those improvements. The brand manifest
  (`views::brand_bundle`) exists so a fork renames itself without patching sources.

The trademark reservation below protects the thing that actually distinguishes the practice, which is why the software
itself does not need protecting.

## Trademarks

**NEON LAW** is a registered trademark, U.S. Reg. No. 6,325,650, owned by Shook Law PLLC. The licence grants rights in
copyright, not in trademarks, and `LICENSE` says so explicitly — a reader deciding whether they may ship a fork called
"Neon Law" reads the licence file, so the answer has to be there rather than only in a doc.

Note that the registrant is not the copyright holder — see [Who holds what](#who-holds-what). The Foundation cannot
sublicense a mark it does not own, so no amount of copyright permission reaches the name.

This is the one reservation this project genuinely needs. A client identifies who is accountable for their legal work by
the name on the door, so a fork trading as Neon Law would misdirect the person least able to check. Anyone may run,
fork, and redistribute the software, and may say their work is built on Neon Law Navigator; nobody may present their
deployment as Neon Law.

The Neon Law Foundation uses the mark for its charitable, pro bono, and public-education work under separate written
permission from the Firm.

## Contributions

**Outside contributions are closed right now**, and anyone is welcome to write to
[contact@neonlaw.org](mailto:contact@neonlaw.org) instead. That is a capacity decision about pull requests and nothing
more: it revokes nothing, because a licence already given cannot be taken back, and every copy already cloned keeps its
rights whatever the contribution policy says.

The terms are stated anyway, so they are knowable in advance and a fork's own authors know where they stand.
Contributions are **inbound = outbound**: anything intentionally submitted for inclusion is licensed `AGPL-3.0-only`,
the same terms the project ships under, wherever in the tree it lands. No copyright assignment, no contributor
agreement, no acceptance ledger, and no bot in the merge path. Contributors keep the copyright in what they write. See
[`CONTRIBUTING.md`](../CONTRIBUTING.md).

Two boundaries survive the opening, because this repository runs a live practice: **no client data ever enters the
tree**, and **a change to `templates/` gets attorney review** before it merges, however mechanical the diff looks.

## What the binary carries

An installed `navigator` is one executable that may sit far from anything it shipped beside, so its terms are compiled
into it as well as staged in the archive. The AGPL requires this rather than merely inviting it: § 4 conditions the
permission to convey on handing every recipient a copy of this License along with the work. A bare executable someone
was given is a copy — and under § 13 its holder may owe the source onward in turn, which nobody honours from terms they
were never shown.

- `navigator --license` prints [`LICENSE`](../LICENSE), embedded with `include_str!`.
- `navigator --third-party-notices` prints `THIRD-PARTY-NOTICES.txt`, likewise embedded.

Each release archive carries `LICENSE` beside the executable, so an unpacked archive states its own terms before anyone
runs anything.

There is no separate end-user licence agreement. Anyone can build the same binary from the same code, so a second
instrument over the executable would only claim restrictions the licence has already given away.

`THIRD-PARTY-NOTICES.txt` is generated by `navigator ops notices` from `Cargo.lock`. A statically linked Rust binary
carries the compiled form of every crate in its dependency tree, and the permissive licences those crates use — the set
`deny.toml` allows — each require their notice to travel with the distributed work. Apache-2.0 section 4 says so
explicitly; MIT, ISC, and the BSD family require the copyright notice to be retained. Each distinct licence text appears
once, listing the crates that carry it; crates that publish no licence file are listed with the SPDX expression their
manifest declares. Regenerate and commit it whenever the dependency tree moves:

```bash
cargo run -p cli -- ops notices
```

`navigator ops notices --check` fails when the committed file is stale, which is the gate a release should run.

## What the images carry

A container image someone pulled is a copy too, and its holder has neither the repository nor a release archive. Every
published image therefore does both of the things a registry makes possible:

- `LABEL org.opencontainers.image.licenses="AGPL-3.0-only"`, which Artifact Registry and GHCR read for the package
  page — what a reader sees *before* pulling.
- `LICENSE` staged at `/app` beside the binary — what a running container can be made to show.

`Containerfile.runner` is exempt: it is the CI runner image rather than a published artifact of the software.

### The GHCR mirror

`publish-service` can mirror the product images to `ghcr.io` alongside Artifact Registry, from the same build. It is
**off by default** and turns on with one repository variable:

| Setting | Kind | Value |
| --- | --- | --- |
| `GHCR_PUBLISH` | variable | `true` |

No credential to create. The repository and its Actions live on github.com, whose own registry is `ghcr.io`, so the
mirror authenticates with `GITHUB_TOKEN` and the `packages: write` scope. That scope is granted on `publish-service`
alone rather than at the top of the workflow — every other job in the file checks out and builds release code, and none
of them has any business writing packages.

Two things to know before switching it on:

- **A GHCR package inherits its linked repository's visibility.** `neon-law-foundation/navigator` is private, so the
  mirror publishes private packages. Harmless, and pointless — which is why the default is off.
- **Publishing the licence is not publishing the repository.** A licence says what a reader may do with the source; it
  does not put the source anywhere. Making the tree public is a separate, deliberate act.

## Releases

Release archives carry `LICENSE` so an unpacked archive states its own terms without the repository tree, and the binary
prints the licence and the third-party notices itself. Someone who ends up with nothing but the executable can still
read the terms they are running under and the attributions it is obliged to carry.
