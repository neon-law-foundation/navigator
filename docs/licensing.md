# Licensing

Navigator is open source. Root [`LICENSE.md`](../LICENSE.md) is the licence of record, and it covers three kinds of
thing rather than one:

| What | Licence | Why |
| --- | --- | --- |
| Workspace, CLI, build and deploy tooling | `MIT OR Apache-2.0` | Software, and the reader picks |
| Notation bodies under `templates/` | `CC-BY-4.0` | A drafted document; attribution is what fits |
| Blank government PDFs in `templates/forms/` | None — not the Foundation's | A Nevada state form belongs to Nevada |

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
Copy it, change it, sell it, run it for other people — none of that needs anyone's permission. Calling the result "Neon
Law" needs the Firm's, and the Firm does not give it.

## Software: MIT OR Apache-2.0

Every line of code is dual-licensed under [`LICENSE-MIT`](../LICENSE-MIT) or [`LICENSE-APACHE`](../LICENSE-APACHE) at
the recipient's option. Cargo and npm manifests declare `MIT OR Apache-2.0`.

The pair is the Rust ecosystem's convention and it is deliberate rather than inherited. MIT is the permissive default a
reader recognises on sight; Apache-2.0 adds an express patent grant that MIT does not carry. Offering only one would
give a downstream user strictly less than the pair does, and legal-automation software published from inside a legal
practice is exactly the kind of upstream whose patent position a downstream user would otherwise have to guess at.

`deny.toml`'s allowlist is a different question and stays permissive. It governs what this workspace is willing to
*consume*, which has nothing to do with how the workspace is licensed out.

## Legal content: CC BY 4.0

The drafted legal prose — the notation bodies under `templates/`, carrying the documents a client signs together with
the questionnaire prompts and workflow definitions in the same files — is Creative Commons Attribution 4.0
International, `CC-BY-4.0`.

A software licence is written for software. Preserving a copyright header in "the Software", marking modified files, a
patent grant — none of it describes anything a will template does. The obligation that matters for a drafted document is
attribution: publish something derived from ours and say where it came from. Otherwise adapt and redistribute freely,
commercially included.

## Government forms: nobody's to license

The blank government PDFs under `templates/forms/` are works of the issuing state or federal agency. The Foundation
claims no copyright in them and grants none; they are committed so the binary embeds the same bytes the repository
carries, and for no other reason.

This is not a technicality. Claiming CC BY over a state's own form would be over-claiming a copyright the Foundation
does not hold, and an over-claim in a licence file published beside a law practice is the kind of error that gets quoted
back. What the Foundation does license beside each blank PDF is its own material: the catalog card, the field map, and
the workflow that fills the form in.

## Why open

Neon Law charges published flat fees for consumer legal work. That economics only holds if routine matters cost very
little to run, and this software is what makes them cost little. Publishing it is the same argument as publishing the
prices: a legal system where only the well-resourced can afford counsel is not fixed by one firm being efficient in
private.

Three consequences follow, and they are the trade being made:

- **No trade-secret protection.** Anything published cannot be un-published, so no mechanism in this tree is a secret.
- **The confidentiality boundary is procedural.** A publication path exists, so the no-client-data rule is enforced by
  a load-bearing test on every pull request — see [`agent-workflows.md`](agent-workflows.md#no-client-data-in-the-repo).
- **Forks are expected.** Another firm running this software is the point. The brand manifest (`views::brand_bundle`)
  exists so a fork renames itself without patching sources.

The trademark reservation below protects the thing that actually distinguishes the practice, which is why the software
itself does not need protecting.

## Trademarks

**NEON LAW** is a registered trademark, U.S. Reg. No. 6,325,650, owned by Shook Law PLLC. The licences grant rights in
copyright, not in trademarks, and `LICENSE.md` says so explicitly. Apache-2.0 § 6 withholds trademark rights on its own,
but a reader deciding whether they may ship a fork called "Neon Law" reads `LICENSE.md`, not § 6.

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
Contributions are **inbound = outbound**: anything intentionally submitted for inclusion is licensed under the same
terms the project ships under, per Apache-2.0 section 5. No copyright assignment, no contributor agreement, no
acceptance ledger, and no bot in the merge path. Contributors keep the copyright in what they write. See
[`CONTRIBUTING.md`](../CONTRIBUTING.md).

Two boundaries survive the opening, because this repository runs a live practice: **no client data ever enters the
tree**, and **a change to `templates/` gets attorney review** before it merges, however mechanical the diff looks.

## What the binary carries

An installed `navigator` is one executable that may sit far from anything it shipped beside, so its terms are compiled
into it as well as staged in the archive. Both grants require this rather than merely inviting it: MIT conditions its
permission on the notice travelling with every copy, and Apache-2.0 § 4(a) obliges a redistributor to hand recipients a
copy of the License. A bare executable someone was given is a copy.

- `navigator --license` prints [`LICENSE.md`](../LICENSE.md), embedded with `include_str!`.
- `navigator --third-party-notices` prints `THIRD-PARTY-NOTICES.txt`, likewise embedded.

Each release archive carries `LICENSE.md`, `LICENSE-MIT`, and `LICENSE-APACHE` beside the executable, so an unpacked
archive states its own terms — and both texts it offers — before anyone runs anything.

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

- `LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"`, which Artifact Registry and GHCR read for the package
  page — what a reader sees *before* pulling.
- `LICENSE.md`, `LICENSE-MIT`, and `LICENSE-APACHE` staged at `/app` beside the binary — what a running container can be
  made to show.

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

Release archives carry `LICENSE.md`, `LICENSE-MIT`, and `LICENSE-APACHE` so an unpacked archive states its own terms
without the repository tree, and the binary prints the licence and the third-party notices itself. Someone who ends up
with nothing but the executable can still read the terms they are running under and the attributions it is obliged to
carry.
