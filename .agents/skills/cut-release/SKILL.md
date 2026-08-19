---
name: cut-release
description: >
  Cut a Neon Law Navigator release: run every check the pipeline will run, write the workspace version, land it through
  a PR, then tag that merged commit so `deploy.yml` publishes. Trigger when the user says "/cut-release", "cut a
  release", "ship a release", "tag a release", or "publish the CLI". The whole point is to fail on this machine, in
  seconds, instead of on a pushed tag that cannot be moved — a rejected tag spends the day's only name. Stops at "the
  tag is pushed and the run is watched"; rolling a cluster is `navigator ops ship`, a separate human act.
---

# cut-release

Publishing has exactly one trigger: **a person pushes an immutable release tag**. There is no cron and no
`workflow_dispatch`, because neither carries a tag and both could only *derive* a version — which is how the manifest
once sat at `0.1.0` while tags marched on. Read [`docs/gitops.md`](../../../docs/gitops.md) for the authoritative flow;
this file is the order of operations and the checks that must happen *before* the ref exists.

**The tag is immutable and the day's name is spent the moment it is pushed.** The `release-tags` ruleset restricts
deletion, update, and non-fast-forward with no bypass actor. Every check below exists because discovering the problem
after the push costs the day.

## 0. Ask what the name is — never choose it

**The version is a lookup, not a judgement call.** `scripts/next-release-tag.sh` prints the one name that is cuttable
right now and exits; it reads the remote and the UTC clock and writes nothing, so run it as often as you like.

```bash
.claude/skills/cut-release/scripts/next-release-tag.sh
```

It transcribes `deploy.yml`'s rule rather than remembering it:

| Is today's UTC `YY.M.D` already a tag? | The cuttable name |
| --- | --- |
| No | `YY.M.D` — today's UTC date, plain |
| Yes | `<tomorrow's UTC date>-hotfix.<UTC hour>`, walking to the first free `N` |

Two things that rule encodes, both of which cost a day when guessed instead:

- **A plain tag must equal TODAY's UTC date; a prerelease base must equal TOMORROW's.** The workflow checks exactly
  that, so at 23:30 UTC there is no way to cut a plain tag for tomorrow — wait thirty minutes and today's name becomes
  it. The date that matters is UTC's, never your wall clock.
- **A hotfix hangs off the NEXT day.** Semver ranks a prerelease below its own base (spec §11.3), so
  `26.8.19-hotfix.23` would sort as *older* than the `26.8.19` it follows.

Unpadded, always: August is `8`, not `08`, and semver forbids a leading zero in a numeric prerelease identifier, so
`hotfix.08` is not a version at all.

## 1. Preflight — everything that can fail locally

```bash
.claude/skills/cut-release/scripts/preflight.sh
```

Read-only and safely repeatable. It fetches, then refuses the cut unless every one of these holds, because each is a way
the pipeline rejects a tag and each is free here:

- **The target is reachable from `origin/main`.** A PR branch is never a release source; wait for the PR to merge.
- **The working tree is clean.** A release names a commit, not a desk.
- **`[workspace.package].version` equals the name from step 0.** `cli/build.rs` bakes that value into
  `navigator --version`, so a mismatch ships a binary naming a release its source never heard of.
- **Notices are current** (`ops notices --check`) — every permissive licence in the tree requires its notice to travel
  with the distributed binary, and the CLI archives carry it.
- **The workspace gate passes.** CI runs the coverage floor inside this same pass.

## 2. Write the version

The tag must equal `[workspace.package].version` — the value every crate inherits and `cli/build.rs` bakes into
`navigator --version`. Without it a plain build of the tagged source names a release the source never heard of.

```bash
cargo run -p cli -- ops release-version --tag "$(.claude/skills/cut-release/scripts/next-release-tag.sh)"
```

Passing the name from step 0 explicitly is what keeps the manifest and the tag one decision rather than two that have to
agree. `--no-commit` writes the manifest and leaves the commit to you.

## 3. Land it through a PR

`main` is squash-merge-only and takes no direct commits, so the bump lands as an ordinary PR. Wait for it to merge — the
tag goes on the **merged** commit, not on the branch tip that produced it.

## 4. Tag the merged commit and push

Sign the tag: an unsigned commit cannot enter the merge queue, and commit verification recognizes the `nick@neonlaw.com`
identity.

```bash
git fetch origin && git checkout main && git pull --ff-only
.claude/skills/cut-release/scripts/tag-and-push.sh
```

It re-derives the name, signs the tag, pushes it, and watches the run. Idempotent where a rerun can mean the same thing
and a hard stop where it cannot: an already-pushed tag on this commit just watches the existing run, and a tag that
exists on a *different* commit is refused rather than forced, because a tag cannot be moved.

Pushing the tag is the publish.

## Releasing twice in one day

The day's `YY.M.D` is spent, so a second release is a semver prerelease — and **its base is the NEXT day**, which is
correctness rather than taste. Semver ranks a prerelease *below* its own base (spec §11.3), so `26.8.17-hotfix.17` would
sort as **older** than the `26.8.17` it fixes. Hanging it off the next day keeps the order true:

```text
26.8.17  <  26.8.18-hotfix.17  <  26.8.18-hotfix.21  <  26.8.18
```

`scripts/next-release-tag.sh` already returns this shape once today's name is spent, so a second release needs no
different command — step 0 answers it. A hotfix publishes every image and archive and **bumps the Homebrew tap** like
any other release — the tap holds one version and every `brew install` resolves to it, so that version has to be the
newest build that exists. The one surface that treats it differently is the GitHub Release, flagged as a prerelease so
it is not reported as "Latest".

## What the push actually does

`deploy.yml` validates the tag, builds four images, proves them in KIND, publishes to GHCR, attaches three CLI archives
to a GitHub Release, and reports to `#navigator`. Two things it never does:

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
- **The source is wrong** — the day's tag is spent. Fix forward and cut a hotfix, or tag the next UTC day. A moved tag
  would make every artifact already carrying that version a lie.
- **A change to `deploy.yml` itself** — prove it on a `kind-ci/**` branch, which runs the integration job and publishes
  nothing. That is the only way to test the workflow without spending a tag.

Nothing runs this pipeline on a clock, so a defect introduced today is invisible until someone next tags. Pushing a
`kind-ci/**` branch is the periodic check that replaced the retired nightly cron — a habit, not a trigger.
