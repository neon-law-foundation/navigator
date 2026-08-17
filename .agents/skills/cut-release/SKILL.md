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

Publishing has exactly one trigger: **a person pushes a `YY.M.D` tag**. There is no cron and no `workflow_dispatch`,
because neither carries a tag and both could only *derive* a version — which is how the manifest once sat at `0.1.0`
while tags marched on. Read [`docs/gitops.md`](../../../docs/gitops.md) for the authoritative flow; this file is the
order of operations and the checks that must happen *before* the ref exists.

**The tag is immutable and the day's name is spent the moment it is pushed.** The `release-tags` ruleset restricts
deletion, update, and non-fast-forward with no bypass actor. Every check below exists because discovering the problem
after the push costs the day.

## 1. Preflight — everything that can fail locally

Run these first. Each one maps to a way the pipeline refuses a tag, and each is free here.

```bash
git fetch origin && git status --short
```

- **On `main`, current, clean.** The tag must point at a commit on `main`, and `main` takes no direct commits.
- **The three tag checks `release-version` will run** — shape, UTC date, and manifest equality. Compute the tag the
  same way the workflow does, in UTC:

```bash
TZ=UTC date +'%y.%-m.%-d'
```

- **Unpadded, always.** August is `8`, not `08`. Cargo parses the manifest as strict semver, so a fourth component is
  impossible and a leading zero is invalid.
- **The midnight edge is real.** The date that matters is UTC's, not yours. From 20:00 local on `-04:00`, UTC has
  already rolled over and the only releasable tag is *tomorrow's* local date. Check the UTC date rather than your wall
  clock; re-tagging costs a minute now and a whole day after the push.
- **The name must be free.** `git tag -l` and the remote both — a spent name cannot be reused.
- **Notices must be current.** Every permissive licence in the tree requires its notice to travel with the distributed
  binary, and the CLI archives carry it:

```bash
cargo run -p cli --quiet -- ops notices --check
```

- **The workspace gate.** CI runs the coverage floor inside this same pass:

```bash
cargo nextest run --workspace && cargo test -p features
```

## 2. Write the version

The tag must equal `[workspace.package].version` — the value every crate inherits and `cli/build.rs` bakes into
`navigator --version`. Without it a plain build of the tagged source names a release the source never heard of.

```bash
cargo run -p cli -- ops release-version
```

`--tag <value>` writes an explicit version; `--no-commit` writes the manifest and leaves the commit to you.

## 3. Land it through a PR

`main` is squash-merge-only and takes no direct commits, so the bump lands as an ordinary PR. Wait for it to merge — the
tag goes on the **merged** commit, not on the branch tip that produced it.

## 4. Tag the merged commit and push

Sign the tag: an unsigned commit cannot enter the merge queue, and GitHub Enterprise verifies only the
`nick@neonlaw.com` identity.

```bash
git fetch origin && git checkout main && git pull --ff-only
git tag -s "$TAG" -m "$TAG" && git push origin "$TAG"
```

Pushing the tag is the publish. Watch it:

```bash
gh run watch "$(gh run list --workflow=deploy.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

## Releasing twice in one day

The day's `YY.M.D` is spent, so a second release is a semver prerelease — and **its base is the NEXT day**, which is
correctness rather than taste. Semver ranks a prerelease *below* its own base (spec §11.3), so `26.8.17-hotfix.17` would
sort as **older** than the `26.8.17` it fixes. Hanging it off the next day keeps the order true:

```text
26.8.17  <  26.8.18-hotfix.17  <  26.8.18-hotfix.21  <  26.8.18
```

```bash
cargo run -p cli -- ops release-version --hotfix
```

`H` is the UTC hour, unpadded, `0`–`23` — semver forbids a leading zero in a numeric prerelease identifier, so
`hotfix.08` is not a version at all. A hotfix publishes every image and archive, but it is flagged as a prerelease and
**the Homebrew tap is not notified**: the tap holds one version and every `brew install` resolves to it, so bumping it
to a prerelease would hand an rc to everyone who ran `brew update`.

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
