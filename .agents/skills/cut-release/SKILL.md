---
name: cut-release
description: >
  Cut a Neon Law Navigator release by writing the version you name into `[workspace.package].version` and landing it on
  `main` through a PR — the merge is the publish. Trigger when the user says "/cut-release", "cut a release", "ship a
  release", "tag a release", or "publish the CLI". There is no tag to push: `deploy.yml` reads the merged manifest,
  proves the tree, creates the immutable tag itself, and publishes. Stops at "the bump is merged and the run is
  watched"; rolling a cluster is `navigator ops ship`, a separate human act.
---

# cut-release

Publishing has exactly one trigger: **a version bump lands on `main`**. There is no tag push, no cron, and no
`workflow_dispatch`. Read [`docs/gitops.md`](../../../docs/gitops.md) for the authoritative flow; this file is the order
of operations.

**The version is `[workspace.package].version`.** `deploy.yml` runs `ops release-check` on every push to `main`, and a
version newer than every release tag is what makes that push build, prove, tag, and publish. The tag is derived from the
manifest rather than compared against it, so the tag and the source cannot disagree — they are one decision.

## 0. The operator names the version

**You provide the version. This skill never chooses one.** Naming a release is an operator decision, so the skill's job
is to *check* the name you give it.

There are exactly two rules, and only the second is enforced:

| Rule | Enforced by | What it admits |
| --- | --- | --- |
| It is a version | `semver::Version::parse` | three components, no leading zeros, no build metadata, any prerelease |
| It is **newer than every release tag** | `ops release-check` | semver ordering: a prerelease is below its base |

**`YY.M.D` is the convention and nothing checks it.** Name a release after the UTC day you cut it — `26.8.23` — because
a date is a useful thing for a version to mean. But no date check exists any more: a bump is authored days before it
merges, so a clock check could only ever fail a release for having been reviewed slowly. If the convention does not fit,
depart from it; the pipeline only cares that the number went up.

Three things that follow, each of which used to be a rule someone had to know:

- **A prerelease of a released version is refused, and its own base is fine.** `26.8.23-hotfix.1` is admissible after
  `26.8.22` and refused after `26.8.23`, because semver ranks a prerelease below its base (spec §11.3). The old rule
  that a hotfix hangs off tomorrow's date was a description of exactly this; the comparator does the job now.
- **Unpadded, always.** August is `8`. Not a preference — `26.08.23` is not a version, and semver forbids a leading zero
  in a numeric prerelease identifier, so `hotfix.08` is not one either.
- **A second release the same day needs no special spelling.** `26.8.23` after `26.8.22`, or `26.8.23-hotfix.1`. Both
  are just bigger numbers.

## 1. Write the version

```bash
cargo run -p cli -- ops release-version --tag 26.8.23
```

`--tag` is required: the command derives nothing, so passing the version you named is what keeps the manifest and the
release one decision. It parses the version on the way in, so a name the pipeline would refuse fails here. It refreshes
`Cargo.lock` too and commits both files — every workspace crate is pinned in the lock as well, and the release builds
with `--locked`. `--no-commit` writes both files and leaves the commit to you.

## 2. Preflight — everything that can fail locally

```bash
.agents/skills/cut-release/scripts/preflight.sh
```

Read-only and safely repeatable, and it takes no version: it reads the one you just wrote. It runs the release decision
exactly as CI will, checks the notices and the lock, and runs the workspace gate.

**`ci.yml` runs the first three of those on the pull request**, so this script is about not wasting a CI cycle rather
than about being the only line of defence. That is the change worth knowing: the release preflight used to live only
here, on your machine, skippable by forgetting.

**The browser suite is the exception, and it matters.** A green `ci` proves the Rust workspace and says nothing about
the browser and accessibility suites — they self-skip when no harness is present, so the only thing that runs them is
`deploy.yml`'s `integration` job, on the merge that publishes. A UI regression is otherwise discovered by the release:

```bash
cargo run -p cli -- dev browser-e2e
```

## 3. Land it through a PR

`main` is squash-merge-only and takes no direct commits, so the bump lands as an ordinary PR. Its `ci` check is the last
place the release preflight is still free to fix, and it runs all of it: `ops release-check`, `ops notices --check`, and
the `--locked` lock check.

**Merging is the publish.** Nothing else is required of you.

## 4. Watch the run

The merge starts `deploy.yml`. Watch it:

```bash
gh run watch "$(gh run list --workflow deploy.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```

`--branch main` is the filter that matters: every merge starts a run, and almost all of them end in seconds having
decided there is nothing to publish.

## What the merge actually does

`deploy.yml` reads the version, builds four images, proves them in KIND, **creates the release tag**, publishes to GHCR,
attaches three CLI archives to a GitHub Release, bumps the Homebrew tap, and reports to `#navigator`.

**The tag is created after integration passes and before anything publishes**, and that ordering is the point. While a
person pushed the tag, the ref existed before a single image was built, so a release that went red had already spent its
name. Now a failure above that line costs nothing but a re-run.

Two things it never does:

- **It deploys nothing and holds no cloud credential.** It ends at the registry. Every rollout is `navigator ops ship`,
  run by a person against their own short-lived ADC. Do not add a ship job — the seam is what keeps "which version is
  production on?" answered by the operator who rolled it.
- **It cannot move a ref.** `release-tag` creates one and refuses a tag that already exists at a different commit;
  `release-windows-cli-publish` creates a Release against it. The `release-tags` ruleset restricts deletion, update, and
  non-fast-forward with no bypass actor.

The CLI archives are load-bearing beyond the human download: `.github/actions/validate`, the gate every Project
repository runs, fetches them from the Release this run creates. If that lane stops, Project CI breaks everywhere with a
download 404 and nothing in this repository goes red.

## When it fails

- **Before the tag** — a red build or a red KIND suite. Nothing was created and nothing published; re-run it, and the
  version keeps its name. This is most failures, and it is the whole reason the tag moved into the pipeline.
- **After the tag** — a registry flake, a tap rejection. Re-run the failed jobs; the tag is unchanged, so a re-run
  republishes the same name over itself. There is nothing to un-publish, because nothing was deployed.
- **"Re-run all jobs" is safe.** `release-check` reports a version whose tag already names *this* commit as publishable,
  so a full re-run republishes instead of deciding there is nothing to do.
- **The source is wrong** — that version is spent. Bump past it and merge again.
- **A change to `deploy.yml` itself** — prove it on a `kind-ci/**` branch, which runs the integration job, creates no
  tag, and publishes nothing. That is the only way to test the workflow without publishing.
- **`release-check` says the version is OLDER than a released one** — a bad bump, or a rebase that resurrected an old
  manifest. Bump past the version it names.

Nothing runs this pipeline on a clock, so a defect in the publishing stages is invisible until someone next bumps the
version. Pushing a `kind-ci/**` branch is the periodic check that replaced the retired nightly cron — a habit, not a
trigger. One thing did get cheaper: the decision job runs on *every* merge, so a break in the trigger itself surfaces at
the next merge rather than at the next release.
