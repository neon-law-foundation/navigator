---
name: cut-release
description: >
  Cut a Neon Law Navigator release from a version YOU name: validate the version you provide, run every check the
  pipeline will run, write it into the workspace, land it through a PR, then tag that merged commit so `deploy.yml`
  publishes. Trigger when the user says "/cut-release", "cut a release", "ship a release", "tag a release", or "publish
  the CLI". The whole point is to fail on this machine, in seconds, instead of on a pushed tag that cannot be moved — a
  rejected tag spends the name for good. Stops at "the tag is pushed and the run is watched"; rolling a cluster is
  `navigator ops ship`, a separate human act.
---

# cut-release

Publishing has exactly one trigger: **a person pushes an immutable release tag**. There is no cron and no
`workflow_dispatch`, because neither carries a tag and both could only *derive* a version — which is how the manifest
once sat at `0.1.0` while tags marched on. Read [`docs/gitops.md`](../../../docs/gitops.md) for the authoritative flow;
this file is the order of operations and the checks that must happen *before* the ref exists.

**The tag is immutable and the name is spent the moment it is pushed.** The `release-tags` ruleset restricts deletion,
update, and non-fast-forward with no bypass actor. Every check below exists because discovering the problem after the
push costs the day.

## 0. The operator names the version

**You provide the version. This skill never chooses one.** Naming a release is an operator decision — whether today's
work is an ordinary cut or a hotfix, and which `N` a hotfix carries — so the skill's job is to *check* the name you give
it, not to pick one and hand it back.

Pass the version to every command below. It is validated before anything else runs:

```bash
.claude/skills/cut-release/scripts/validate-release-tag.sh 26.8.20
```

Offline, read-only, and repeatable. It transcribes the two rules `deploy.yml` applies rather than remembering them, and
rejects a name the workflow would refuse:

| Rule | What it admits |
| --- | --- |
| SHAPE | `YY.M.D` — two-digit year, unpadded month and day — optionally suffixed `-hotfix.N` |
| DATE | a plain tag's base is TODAY's UTC date; a `-hotfix.N` base is TOMORROW's |

Three things that rule encodes, each of which costs a day when guessed instead:

- **A plain tag must equal TODAY's UTC date; a prerelease base must equal TOMORROW's.** The workflow checks exactly
  that, so at 23:30 UTC there is no way to cut a plain tag for tomorrow — wait thirty minutes and today's name becomes
  it. The date that matters is UTC's, never your wall clock.
- **A hotfix hangs off the NEXT day.** Semver ranks a prerelease below its own base (spec §11.3), so
  `26.8.19-hotfix.23` would sort as *older* than the `26.8.19` it follows.
- **Unpadded, always.** August is `8`, not `08`, and semver forbids a leading zero in a numeric prerelease identifier,
  so `hotfix.08` is not a version at all.

`N` is a uniqueness-and-ordering discriminator, not an hour: it is yours to pick and nothing bounds it at 23. Whether
the name is still unspent is a question about the remote, so step 1 asks it — a name already on `origin` is refused
there rather than at the push.

## 1. Preflight — everything that can fail locally

```bash
.claude/skills/cut-release/scripts/preflight.sh 26.8.20
```

Read-only and safely repeatable. It validates the version, fetches, then refuses the cut unless every one of these
holds, because each is a way the pipeline rejects a tag and each is free here:

- **The version is one `deploy.yml` will accept** — shape and base date, per step 0.
- **The version is not already taken on `origin`.** The `release-tags` ruleset admits no bypass actor, so a spent name
  is spent for good; a second release in one UTC day is a `-hotfix.N` prerelease on tomorrow's base.
- **The target is reachable from `origin/main`.** A PR branch is never a release source; wait for the PR to merge.
- **The working tree is clean.** A release names a commit, not a desk.
- **`[workspace.package].version` equals the version you named.** `cli/build.rs` bakes that value into
  `navigator --version`, so a mismatch ships a binary naming a release its source never heard of.
- **`Cargo.lock` agrees with the manifest** (`cargo metadata --locked`). The release builds with `--locked` in four
  places, so a lock still naming the previous version fails *after* the tag is pushed, and a tag cannot be moved.
- **Notices are current** (`ops notices --check`) — every permissive licence in the tree requires its notice to travel
  with the distributed binary, and the CLI archives carry it.
- **The workspace gate passes.** CI runs the coverage floor inside this same pass.

## 2. Write the version

The tag must equal `[workspace.package].version` — the value every crate inherits and `cli/build.rs` bakes into
`navigator --version`. Without it a plain build of the tagged source names a release the source never heard of.

```bash
cargo run -p cli -- ops release-version --tag 26.8.20
```

Passing the same version you validated in step 0 is what keeps the manifest and the tag one decision rather than two
that have to agree. The command refreshes `Cargo.lock` too and commits both files — every workspace crate is pinned in
the lock as well, and the release builds with `--locked`. `--no-commit` writes both files and leaves the commit to you.

## 3. Land it through a PR

`main` is squash-merge-only and takes no direct commits, so the bump lands as an ordinary PR. Wait for it to merge — the
tag goes on the **merged** commit, not on the branch tip that produced it.

## 4. Tag the merged commit and push

Sign the tag: an unsigned commit cannot enter the merge queue, and commit verification recognizes the `nick@neonlaw.com`
identity.

```bash
git fetch origin && git checkout main && git pull --ff-only
.claude/skills/cut-release/scripts/tag-and-push.sh 26.8.20
```

Pass the same version again — the script signs the name it is handed and derives nothing, so the name that passed
preflight is the name that publishes. It re-validates shape and base date (the last place a typo is still free), signs
the tag, pushes it, and watches the run. Idempotent where a rerun can mean the same thing and a hard stop where it
cannot: an already-pushed tag on this commit just watches the existing run, and a tag that exists on a *different*
commit is refused rather than forced, because a tag cannot be moved.

Pushing the tag is the publish.

## Releasing twice in one day

The day's `YY.M.D` is spent, so a second release is a semver prerelease — and **its base is the NEXT day**, which is
correctness rather than taste. Semver ranks a prerelease *below* its own base (spec §11.3), so `26.8.17-hotfix.17` would
sort as **older** than the `26.8.17` it fixes. Hanging it off the next day keeps the order true:

```text
26.8.17  <  26.8.18-hotfix.17  <  26.8.18-hotfix.21  <  26.8.18
```

So a second cut is the same four steps with a `<tomorrow's UTC date>-hotfix.N` version instead — you pick `N`, and step
1 refuses it if that exact name is already on the remote. Step 2 passes your version to `--tag` rather than reaching for
the CLI's `--hotfix`, which [`docs/gitops.md`](../../../docs/gitops.md) documents as deriving `N` from the current UTC
hour: that flag is a fine shortcut by hand, but it makes the name a side effect of when the command ran, which is the
one thing this skill exists to prevent. A hotfix publishes every image and archive and **bumps the Homebrew tap** like
any other release — the tap holds one version and every `brew install` resolves to it, so that version has to be the
newest build that exists. The one surface that treats it differently is the GitHub Release, flagged as a prerelease so
it is not reported as "Latest".

## What the push actually does

`deploy.yml` validates the tag, builds four images, proves them in KIND, publishes to GHCR, attaches three CLI archives
to a GitHub Release, and reports to `#navigator`. Its `release-version` job is the authority every check above
transcribes: shape, base date, equality with `[workspace.package].version`, and provenance from `origin/main`. Two
things it never does:

- **It deploys nothing and holds no cloud credential.** It ends at the registry. Every rollout is `navigator ops ship`,
  run by a person against their own short-lived ADC. Do not add a ship job to that workflow — the seam is what keeps
  "which version is production on?" answered by the operator who rolled it.
- **It cannot move a ref.** The only job holding `contents: write` creates the GitHub Release, and the ref arrived
  before the run did.

The CLI archives are load-bearing beyond the human download: `.github/actions/validate`, the gate every Project
repository runs, fetches them from the Release this run creates. If that lane stops, Project CI breaks everywhere with a
download 404 and nothing in this repository goes red.

## When it fails

- **A flake** — re-run the failed jobs. The tag is unchanged, so a re-run republishes the same name over itself. There
  is nothing to un-publish, because nothing was deployed.
- **The source is wrong** — the name is spent. Fix forward and cut a hotfix, or tag the next UTC day. A moved tag would
  make every artifact already carrying that version a lie.
- **A change to `deploy.yml` itself** — prove it on a `kind-ci/**` branch, which runs the integration job and publishes
  nothing. That is the only way to test the workflow without spending a tag.

Nothing runs this pipeline on a clock, so a defect introduced today is invisible until someone next tags. Pushing a
`kind-ci/**` branch is the periodic check that replaced the retired nightly cron — a habit, not a trigger.
