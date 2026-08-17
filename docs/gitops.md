# GitOps: edit → merge → release → deploy

Every change reaches production through an auto-merging PR to `main`, followed by a release tag, image publication, and
a deliberate rollout. This flow supports the actions in [`agent-workflows.md`](agent-workflows.md).

## `main` is sacred and squash-merge-only

- **Never commit directly to `main`.** PRs squash to one commit; merge and rebase-merge are disabled.
- **Production follows `main`.** GKE reconciles `examples/deploy/k8s/gke`, and release tags point at its tip. See
  [`gke-prod.md`](gke-prod.md).

## The branch → PR → auto-merge flow

1. **Task worktree + branch.** Start with **New Worktree** in Codex or Claude and verify the current path is a
   non-primary `git worktree` entry. Then run `cargo run -p cli -- dev worktree-env up --branch <kebab-topic>` once. The
   CLI names that worktree's PR branch; it does not create another checkout. See
   [`agent-workflows.md`](agent-workflows.md#create-a-pr) for the stop condition when a task did not start in New
   Worktree.
2. **Push + open a PR.** `git push -u origin <branch>` then `gh pr create`.
3. **Let auto-merge land it.** `ci.yml` enables auto-merge on open and each push. A green required gate plus resolved
   review threads triggers squash; approval is not required. Use draft status to hold a PR. The PR title becomes the
   Conventional Commit subject on `main`.

### Codified merge gate

Navigator is public at [`neon-law-foundation/navigator`](https://github.com/neon-law-foundation/navigator) on
github.com. Nothing here names a host, sets `GH_HOST`, or repoints `NAVIGATOR_GITHUB_API_BASE`: `gh` and every Action
default to the right place.

A public repository gets GitHub-hosted runners at no cost, so CI runs on `ubuntu-latest` rather than on
organization-hosted capacity, and the Marketplace is available.

**Confirm an App id against a live check run before trusting it.** A required status check registered under the wrong id
still *reads* as present in the API while matching an App that never posts a check, so the gate silently enforces
nothing:

```bash
gh api repos/neon-law-foundation/navigator/commits/<sha>/check-runs \
    --jq '.check_runs[] | "\(.name) \(.app.id) \(.app.slug)"'
```

`navigator ops github setup [repository]` reconciles one repository at a time, and it governs **every** repository the
Firm administers rather than a checked-in pair. The target resolves in precedence order — the explicit `owner/name`
argument, then `GITHUB_REPOSITORY`, then the checkout's `origin` remote.

The authorization boundary is the **host**. A remote pointing anywhere other than `github.com` is refused before a token
is read, so an incidental checkout of someone else's fork cannot become a write target by being the current directory.
That check mattered more when the host itself was the boundary; on a public host it is the last thing standing between a
reconcile and a repository nobody meant to govern, so it stays.

Policy stays explicit; it is simply no longer an allowlist. Every repository gets `COMMON_POLICY`: the `production`
branch protections, the `production-review` gate, the CODEOWNERS assertion, and the merge policy — pull requests only,
squash only, auto-merge, automatic head-branch deletion, and squash commits titled and described from the pull request.
`neon-law-foundation/navigator` alone adds `NAVIGATOR_POLICY`'s three extras — the release-tag ruleset, the DevX labels,
and the App-installation assertion — because it is the only repository that cuts a release or runs that automation.

There is no lighter tier — a repository the Firm administers on someone else's behalf would receive the same gate.
`assert_codeowners` sits in the common policy rather than beside the review gate for a reason the tests enforce:
`require_code_owner_review` against an absent or unresolvable CODEOWNERS silently accepts anyone's approval, so the two
ship together or neither means anything.

Run a dry run before applying drift, then rerun without it:

```bash
navigator ops github setup neon-law-foundation/navigator --dry-run
navigator ops github setup neon-law/ui --dry-run
```

A second dry run after applying must report *no drift*. That is the only proof the reconcile actually converged, and it
is worth running: GitHub returns a ruleset's rules in whatever order it first stored them, which differs between a
ruleset this command created and one built by hand through REST. Comparing the rule vectors positionally made every
hand-made ruleset read as permanently drifted — each run wrote a PUT, each following run still saw drift, and "already
matches" was unreachable. `ruleset_matches` normalizes the order away, because order carries no meaning in the API. A
reconcile that never converges is a reconcile whose drift report means nothing.

#### One required check, named `ci` everywhere

Every administered repository terminates its `ci` workflow in a single aggregating job spelled exactly `ci`, and that
one context is what the ruleset requires. The job runs nothing itself: it `needs:` the real jobs and fails unless they
all succeeded. The jobs behind it stay free to differ per repository — this workspace runs `cargo test (workspace)` on a
large runner, `neon-law/ui` runs a `lint`/`verify` pair — and free to be renamed, because the required context never
moves.

The indirection exists because the alternative fails silently. A required status check is matched by string, so renaming
the job renames its check run while the ruleset goes on waiting for the old spelling. Nothing turns red; pull requests
simply sit forever on a check that will never arrive, and the usual fix — dropping the stale rule — leaves the branch
enforcing nothing at all. `ops github setup` therefore reads the repository's CI workflow and refuses to bind the gate
unless a job in it actually reports as `ci`.

It accepts either `.github/workflows/ci.yml` or `.github/workflows/gate.yml`, and looks for them in that order. Two
spellings are live at once and both are correct: a repository the Firm has always administered carries `ci.yml`, while a
Project repository written by `navigator projects repository scaffold` carries `gate.yml`. What they share is the
invariant the gate is actually matched by — a job whose check run is named `ci` — so the filename is free to differ. A
repository carrying neither file is refused, and so is one whose workflow exists but ends in some other job name; those
are different problems with different fixes, so they are different errors.

#### Adopting a repository that is not yet governed

Host-based resolution removed the allowlist, not the convention. A repository still has to *earn* the gate, and the two
fail-closed assertions above are what it earns it with.

Every other repository the Firm administers grew up terminating its `ci.yml` in a job named `verify`, with its
`production` ruleset requiring that context. `assert_required_check_job` refuses to bind the gate until a job reports as
`ci`, and `assert_codeowners` refuses until the file names an owner the API resolves. So adopting one is two edits *in
that repository* — add the aggregating `ci` job, add `.github/CODEOWNERS` — before `ops github setup` will write
anything.

Order matters and is not a deadlock. Land the `ci` job while the ruleset still requires `verify`; both jobs run, the
pull request merges on `verify`, and the reconcile afterwards moves the required context from `verify` to `ci`. A
repository holding template content with no workflow at all has nothing for a required status check to bind to, so it
takes the CODEOWNERS half and waits for a real `ci.yml` before it can take the rest.

#### Review gate: two rulesets, not one

Contributors cannot land code without a code owner's approval; the code owner can still merge their own work. That
asymmetry is not a preference — GitHub forbids approving your own pull request, so a code-owner requirement applied
uniformly would mean the sole owner could never merge again. What the owner keeps is the ability to merge, not
auto-merge; the note below has the distinction and why it cannot be closed.

Bypass is how GitHub expresses the exemption, and it is scoped to a **whole ruleset, never to a single rule**. So the
policy is split in two:

- **`production`** — no bypass actor, so every rule in it binds the administrator too: the required `ci` check, signed
  commits, linear history, no deletion and no force-push, and a squash-only pull request with every review thread
  resolved.
- **`production-review`** — bypassed by `OrganizationAdmin`: one approving review, `require_code_owner_review`, stale
  reviews dismissed on push, and the last push itself required to carry an approval.

Rules of the same type in two rulesets do not replace one another — GitHub applies the union and the most restrictive
value wins. A contributor is therefore held to one code-owner approval, while the organization owner falls back to
`production`'s zero. Both are still held to signing, linear history, squash, resolved threads, and a green `ci`, because
those rules live in the ruleset nobody bypasses. That is the whole point of the split: the owner's exemption buys
exactly one thing, and it is not the test gate.

The bypass names the `OrganizationAdmin` **role**, not a person, so the policy survives the administrator changing
without a code edit and cannot silently widen the way a hardcoded username or a `write`-role bypass would.

> **Auto-merge on the owner's own pull requests.** A bypass belongs to the actor who performs the merge, and auto-merge
> merges as whoever *armed* it. `ci.yml`'s `enable-automerge` job arms it as the App (or as `GITHUB_TOKEN`), and that
> identity is not an `OrganizationAdmin` — so on the owner's own pull requests auto-merge can sit waiting for an
> approval that, by GitHub's own rule against self-approval, can never arrive. `.github/CODEOWNERS` names one owner and
> no one else, so there is no second code owner who could supply that approval either. Do not "fix" this by adding the
> App as a bypass actor: the App arms auto-merge on *everyone's* pull requests, so that would hand every contributor
> the exemption and delete the gate.

The owner therefore merges their own work by hand, once `ci` is green. The bypass permits it from the CLI:

```bash
gh pr merge --squash --admin
```

`--admin` waives the approval requirement in `production-review` and nothing else. `production` carries no bypass actor,
so signed commits, linear history, squash-only, resolved review threads, and a green `ci` all still hold — the same gate
a contributor faces, minus the approval that cannot exist.

Making the owner's merges genuinely unattended costs one of exactly two things, because a ruleset cannot condition on
pull-request author and so cannot exempt one person's pull requests:

- **Drop `production-review`.** Every contributor loses the code-owner gate too, and `production`'s zero-approval
  pull-request rule becomes the whole policy. That is an edit to `desired_review_ruleset` in `cli::devx::github_setup`,
  reconciled by rerunning the setup command — not a click in the settings UI, which would drift back on the next run.
- **Add a second code owner who resolves and actually reviews.** That is an edit to `.github/CODEOWNERS`, and the policy
  stays exactly as it is. It costs a real reviewer, which is the point of the gate.

Repository permissions are the outer boundary and are not managed by this command: a collaborator with `read` cannot
push a branch at all, and with forking disabled has no fork path either. The review gate governs everyone who *can* push
— today the `write` collaborators.

#### CODEOWNERS owners must resolve

`require_code_owner_review` is only worth anything if the file names an owner GitHub can find. GitHub does not reject an
unresolvable CODEOWNERS entry: it drops the rule and leaves those paths unowned, which is indistinguishable from having
no CODEOWNERS at all. A repository can sit for months with the review gate on, the file committed, and no owner on any
path.

This was not hypothetical while the repository sat on an EMU-provisioned enterprise that shared no account namespace
with github.com: a handle carried over from a github.com checkout resolved to nothing. The public host removed that
particular trap, and left the general one — a misspelled handle, or a person who has left the org — which fails exactly
the same way and just as silently. `ops github setup` resolves every owner named in `.github/CODEOWNERS` against the API
(`@user`, `@org/team`; email owners are matched against the commit author and are accepted as-is) and fails closed
before writing anything.

All assertions run before the first write, so a repository that cannot satisfy the policy is left exactly as it was
rather than half-reconciled.

> **Auto-merge identity.** `enable-automerge` prefers a GitHub App token with `contents: write` and
> `pull_requests: write`, falling back to `GITHUB_TOKEN`. It needs `AUTOMERGE_APP_ID` and
> `AUTOMERGE_APP_PRIVATE_KEY` as Actions secrets. The only Actions *variables* are the two registry coordinates in
> [Keyless pushes to the image hub](#keyless-pushes-to-the-image-hub). >
> Neither secret exists on this repository today, so auto-merge arms under `GITHUB_TOKEN` as
> `app/github-actions`. Arming also requires at least one *required* status check: with nothing required, the
> mutation is refused with `Pull request is in unstable status`, because auto-merge has nothing to wait on. >
> Dependabot has a separate secret store, so mirror both App secrets there. Forks do not use this path.

### TDD and the pre-commit gate

- Tests share a commit with the implementation. Rust or runtime changes require:

  ```bash
  cargo fmt
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

- Prose-only changes require:

  ```bash
  cargo run -p cli -- validate <path>
  ```

- After each PR update, clean task-owned builds, KIND, browser, images, and build cache. Never prune volumes without
  approval.

## CI/CD workflows

Add jobs to the workflow that owns their trigger; do not create a redundant workflow.

| Workflow | Trigger | Job |
| --- | --- | --- |
| `.github/workflows/ci.yml` | `pull_request` → `main` | Rust quality gate |
| `.github/workflows/deploy.yml` | a dated `YY.M.D` tag, or a `kind-ci/**` branch | publish + ship staging |
| `.github/workflows/codeql.yml` | `pull_request` → `main` | CodeQL scan — enable it, see below |

### CodeQL can be turned back on

`codeql.yml` was `disabled_manually` while the repository was private, because uploading results needed Code Security
and the enterprise did not have it. The scan itself always ran fine and then failed at the last step:

```text
Code Security must be enabled for this repository to use code scanning.
```

**Open-sourcing the repository removes the blocker.** Code Security was a paid GitHub Advanced Security feature on the
private enterprise repository; code scanning is free on public repositories, so the upload step that always failed now
succeeds. Turn it on:

```bash
gh workflow enable CodeQL
```

Nothing in the workflow file needs to change.

Leaving it enabled while it could only fail was not free, which is why it was disabled rather than tolerated. A
permanently red check is worse than an absent one in two specific ways: it trains reviewers to read red as normal, and
it held auto-merge. The CodeQL checks are not *required* — the `production` ruleset requires only `ci` — but a failing
check of any kind makes the pull request's overall status roll up to `FAILURE`, and auto-merge will not fire against a
failing rollup. PR #3 sat with a green required gate and had to be merged by hand for exactly this reason. If the scan
starts failing for a real finding, that same rollup rule applies.

### One protection system, not two

`main` is governed by **rulesets** alone — `production` and `production-review`. A legacy classic branch-protection rule
was configured as well, and the pair did not compose: the classic rule carried `requiresApprovingReviews: true` with a
required count of `0`, which leaves `reviewDecision` at `null` forever and holds every pull request at `BLOCKED` no
matter how green its checks are. It was deleted.

Nothing was given up in the trade. The rulesets are the stricter of the two — they require signed commits and a passing
`ci`, neither of which the classic rule asked for. Keep protections in rulesets, where `ops github setup` can reconcile
them; a classic rule added by hand is invisible to that command and will drift.

Two rulesets, not one, because bypass is granted per ruleset — see [Review gate: two rulesets, not
one](#review-gate-two-rulesets-not-one). `production` keeps its empty `bypass_actors`, so the classic rule's admin
enforcement is preserved for everything that must hold universally; only the approval requirement in `production-review`
is exempt, and only for the organization owner.

One caveat learned the hard way: GitHub caches a pull request's merge state. Changing branch protection does **not**
recompute it for pull requests whose checks have already finished — they stay `BLOCKED` until some later event on the
pull request forces a fresh evaluation. Push a commit, or close and reopen, after changing protection.

### When `gh pr merge` refuses but the merge is legal

`gh pr merge --squash` can refuse with *"the base branch policy prohibits the merge"* on a pull request that GitHub will
merge without complaint. The CLI runs its own pre-flight against `mergeStateStatus`, and that field reads `BLOCKED`
whenever the status rollup is failing — including when every failing check is optional. The API applies the real policy
instead:

```bash
gh api -X PUT repos/neon-law-foundation/navigator/pulls/<n>/merge -f merge_method=squash
```

Confirm the required check is genuinely green first, because this bypasses the CLI's guess and nothing else:

```bash
gh pr view <n> --json statusCheckRollup \
    --jq '.statusCheckRollup[] | select(.isRequired) | "\(.name) \(.conclusion)"'
```

### PR flow — `ci.yml`

`ci.yml` runs for PRs to `main`, never pushes. The `rust` job carries the quality gate: formatting, the repository-wide
`navigator` content validation pass, `cargo clippy` with warnings denied, and `cargo test --workspace`. The Rust tests
need no database service — each opens its own embedded engine. `ci.yml` is the only workflow that runs on
`pull_request`, and it carries no KIND, Docker, or browser coverage — that proof happens on the release train (and
locally, see below), never on a PR. The workflow is the source of truth for commands, caches, and pinned tool versions.

The `ci` job is the required status check — see [One required check, named `ci`
everywhere](#one-required-check-named-ci-everywhere). It runs nothing, `needs:` the `rust` job, and fails unless it
succeeded. It tests the dependency's result explicitly rather than relying on a bare `needs:`, because a skipped
required check is not a red one: GitHub reports no result at all, so the gate would quietly stop blocking exactly when
the job it guards had failed.

`deploy.yml` no longer has a `pull_request` trigger. It previously ran its KIND integration job against UI-scoped PRs so
Dioxus/browser changes got production-shaped proof before merge; that coupled every PR to the release workflow's script
and image builds. UI and browser changes are instead verified locally before opening a PR — see [Local KIND
development](../CLAUDE.md#local-kind-development) and the `web-preview` / `kind-local-dev` skills — and a tagged release
(or a `kind-ci/**` branch push, below) remains the CI-side KIND proof.

### One workflow owns publishing — `deploy.yml`

Publishing is a calendar event. The cron fires at 01:11 UTC, and that run proves the workspace in KIND, builds every
image, pushes them to GHCR, cuts the `YY.M.D` tag and its GitHub Release, and reports what it published. A
`workflow_dispatch` runs the same pipeline on demand when waiting until tomorrow is not an answer. Versions omit leading
zeros, remain valid semver, and align with image tags and `navigator --version`.

**No tag triggers this workflow, because it CUTS the tag.** `release-windows-cli-publish` creates the `YY.M.D` tag at
the SHA the archives were built from, so a tag trigger would be this workflow reacting to itself. The version derives
from the runner clock and threads into every image build as the `RELEASE_TAG` build-arg, which each Containerfile turns
into the runtime environment variable `NAVIGATOR_RELEASE_TAG`.

**This workflow deploys nothing, and holds no cloud credential.** It ends at the registry. Putting a version in front of
real clients' matters is a separate act a person takes from their own machine — see [The deploy is a human
act](#the-deploy-is-a-human-act).

**Run the browser gate locally before you tag.** A green `ci` proves the Rust workspace and says nothing at all about
the browser and accessibility suites: they self-skip when no harness is present, so the only thing that runs them on CI
is `deploy.yml`'s `integration` job, and the only thing that runs `integration` on a tag is the tag itself. That means a
UI regression is discovered by the release, forty-odd minutes in, on an immutable tag whose name is spent for the day.
So prove it first:

```bash
cargo run -p cli -- dev browser-e2e
```

Green locally is the precondition for pushing the tag. A `kind-ci/<topic>` branch push is the CI-side alternative when
the change is to the workflow itself rather than to a page — it runs `integration` alone and publishes nothing.

**One tag per calendar day, in UTC.** `YY.M.D` admits only one tag per day by construction, and `release-version`
enforces it rather than trusting it: a tag that is not the current UTC date fails the run at its first job, before any
image is built. UTC is the zone this convention has always been derived in, it carries no DST discontinuity, and the
runner clock is already UTC.

**The tag must carry its own version.** The same first job also fails a tag that does not equal `Cargo.toml`'s
`[workspace.package].version` — the value every crate inherits through `version.workspace = true` and `cli/build.rs`
bakes into `navigator --version`. Without this the manifest sat at `0.1.0` while tags marched on, so a plain build of
the tagged source misreported the release it was cut from. The bump is one line — `navigator ops release-version` writes
today's UTC `YY.M.D` (or `--tag` for an explicit value) and commits it. Because the tag points at `main`, that commit
lands through an ordinary PR — `main` takes no direct commits.

**The midnight edge is real.** The date that matters is UTC's, not yours. On `-04:00`, from 20:00 local onward UTC has
already rolled over, so the only releasable tag is *tomorrow's* local date and pushing the one that matches your wall
clock fails. The error names the tag to push instead, and because nothing has been built yet, re-tagging costs a minute.

**Nothing in the pipeline can move a ref.** `release-version` existed to cut the nightly tag and held `contents: write`
for exactly that; it now validates the ref it was handed, and the only job still holding `contents: write` is the one
attaching CLI archives to the tag's GitHub Release. No App identity is involved either: a separate `release-tag.yml`
once cut the tag as the `navigator-release` App purely to defeat GitHub's recursion guard — a tag created with the
built-in `GITHUB_TOKEN` does not trigger another workflow's `on: push: tags` — and a tag pushed by a person is subject
to no such guard.

### What each stage does — `deploy.yml`

The nightly run proves the workspace in KIND and publishes all service and trigger images, plus three `navigator` CLI
archives attached to the `YY.M.D` GitHub Release it cuts: `navigator-<tag>-windows.zip`, `navigator-<tag>-linux.tar.gz`,
and `navigator-<tag>-macos.tar.gz`. Each carries the executable beside `LICENSE.md`, `LICENSE-MIT`, and
`LICENSE-APACHE`. Container images are **linux/amd64 only**; GKE Autopilot consumes amd64. The macOS archive is arm64 —
`macos-latest` is Apple silicon — so an Intel Mac still builds the immutable release tag locally with Cargo, and the
`#navigator` report carries that exact command beside the three downloads. Failure at any stage pages `#navigator`.

**Every publishing run builds all three CLI archives, and Project CI depends on them.** `release-windows-cli-build`,
`release-cli-build-linux`, and `release-cli-build-macos` carry no `if:` of their own — they need the two publish jobs,
so they run whenever a run publishes images and skip whenever one does not (a `kind-ci/**` branch iteration). This is
not only for human downloads: the `.github/actions/validate` composite action, the gate **every** Project repository
runs, downloads `navigator-<version>-<platform>` from the Release these jobs cut. If they stop running, Project CI
breaks everywhere with a download 404 and nothing in this repository goes red — which is exactly the kind of failure
worth stating in prose, because no test here will catch it. The macOS archive existed nowhere until it was added:
`validate` had always mapped a macOS runner to `platform=macos`, so that download 404'd for every Project repository
whose gate ran on one.

The run narrates itself while it goes. Every forward-path step opens with a `.github/actions/slack-progress` post to
`#navigator` naming the tag, the stage, and the step, so the channel watches the release advance rather than waiting ~45
minutes for a verdict — and the last line posted names the step a failure died in, before anyone opens the run. Those
posts are advisory: the action reports a failed webhook as a warning and never fails a job, because a release must not
be lost to its own narration. They self-gate on the trigger ref the same way the two reports do, so a `kind-ci/**`
branch iteration stays silent. Steps gated on `failure()` or `always()` are post-mortem diagnostics rather than progress
and are deliberately not narrated; the failure page already covers that moment. `cli/tests/deploy_slack_progress.rs`
holds the narration complete — a new step added without a post fails that gate, because nobody notices a *missing* Slack
line.

### What detects a broken pipeline

**Nothing on a clock does, and that is the deliberate cost of releasing on a tag.** The nightly train doubled as a daily
liveness check on the whole release path: images still build, KIND still stands up, `ops ship` still authenticates. A
defect introduced today is now invisible until someone next tags. And the cron was never a reliable signal even while it
ran — a silent nightly failure went unnoticed for four consecutive nights — so the honest statement is that this
pipeline has no automatic breakage detection, not that it has a weaker one.

What remains, and what each does not cover:

- `notify-failure` pages `#navigator` when a release fails, reading the trigger ref rather than a job output so that a
  failure anywhere — including the tag validation — still pages. It can only fire on a run that happened.
- `kind-ci/**` proves a release-workflow change on demand: push a `kind-ci/<topic>` branch to run the KIND integration
  job alone, publishing nothing and shipping nothing. On demand, not on a schedule.
- `ci.yml` proves the Rust workspace on every PR and says nothing about images, KIND, or shipping.

Two consequences to plan around rather than discover:

- A `kind-ci/**` push is the cheapest way to exercise the pipeline without releasing. It is the periodic check the cron
  used to be, and it now has to be a habit rather than a trigger.
- **Image retention no longer depends on release cadence**, because it counts versions rather than days — see [Image
  retention](#image-retention). An age-based rule would have made a quiet fortnight delete the versions production was
  running; keeping the last ten releases removes that failure mode rather than documenting it.

### Recovering a failed release

Three lanes, cheapest first.

1. **Re-run the failed jobs** from the run's page, or dispatch `deploy.yml` again. The version derives from the same
   UTC day either way, so a re-run republishes that same name over itself. This is the move for a flake: a runner disk,
   a registry timeout, a wedged port-forward. Nothing was deployed, so there is nothing to un-deploy.
2. **`ops ship` the already-published tag.** If the images published green and only a roll failed, rebuild nothing:

   ```bash
   navigator ops ship --deployment <row> --deployments-dir . --tag YY.M.D
   ```

   This is [The manual deploy](#the-manual-deploy). `ship` builds nothing, refuses a tag absent from the registry, and
   `--dry-run` rehearses it first.
3. **Fix forward on the next UTC day.** If the source is wrong, land the fix on `main` and tag the next day.

**A same-day re-release is not possible, by construction.** One tag per calendar day plus immutable tags means the day's
release name is spent the moment it is pushed: the `release-tags` ruleset restricts deletion and update, so the tag
cannot be moved onto the fix, and no second `YY.M.D` exists for that day. If a same-day fix is genuinely required, the
options are re-running failed jobs when the source is fine, or rolling back with `ops ship --tag <previous>` while the
fix waits for the next UTC day. Deleting and re-pushing a tag is never the answer — a moved tag makes every artifact
already carrying that version a lie.

### Keyless pushes to the image hub

The publish jobs hold no registry key. They mint a short-lived token through Workload Identity Federation and
impersonate `navigator-ci-pusher@ghcr.iam.gserviceaccount.com`, which holds `roles/artifactregistry.writer` on the
`navigator` repository and nothing else — it cannot read a runtime secret and cannot reach a GKE cluster. The
deploy-side identity is a separate service account per runtime project, so a pusher cannot also roll a cluster.

Three values wire this up, and only the provider resource name is a secret:

| Name | Kind | Value |
| --- | --- | --- |

**The issuer is the standard one.** Actions OIDC tokens are minted by `https://token.actions.githubusercontent.com`,
which is what every public WIF tutorial assumes and what the pool must trust. This was the opposite while Navigator sat
on a data-residency enterprise, which minted its own tokens under the tenant host — a provider pinned to the wrong
issuer is accepted at create time, reports `ACTIVE`, and then fails every token exchange, so confirm it rather than
assuming:

```bash
curl https://token.actions.githubusercontent.com/.well-known/openid-configuration
```

The attribute condition is pinned to the full `owner/repo` and to the refs that may publish, so a fork — which carries
its own `repository` claim — and an arbitrary branch both fail at `google-github-actions/auth` rather than at the push:

```text
assertion.repository == 'neon-law-foundation/navigator'
    && (assertion.ref == 'refs/heads/main' || assertion.ref.startsWith('refs/tags/'))
```

`refs/tags/*` is the arm that matters: every publishing run now starts from a tag ref, so that is the claim the token
carries. `refs/heads/main` is retained and unused — it admitted the `workflow_dispatch` publish, which ran from `main` —
and narrowing the condition to tags alone is a provider change to make deliberately rather than a side effect of
retiring the trigger. `navigator ops gcp hub setup --project-id ghcr` reconciles all of it, converging an existing
provider onto the current issuer and condition instead of reporting success over a stale one. The impersonation binding
is reconciled *exclusively*: exactly one repository may impersonate the pusher, because an `ensure` that only ever adds
can never revoke, and an org rename would otherwise leave the old principal still able to publish images.

### Image retention

Published images are garbage-collected by the registry itself, not by a workflow. `navigator ops gcp hub setup` PATCHes
the hub repository's `cleanupPolicies` to a pair of rules (`cli::devx::gcp::artifact_registry`): a `KEEP` policy
retaining the **last 10 versions of every image**, and a `DELETE` policy matching everything else. Change retention by
changing `RETAINED_VERSIONS` and re-running the hub setup — there is no scheduled GitHub workflow in this repository to
add it to.

**Retention is a count, not an age, and that is load-bearing.** An age-based rule is only safe while releases outrun it.
Under the nightly train every running tag was a day old, so the old `delete-older-than-7d` rule could never reach one;
with releases driven by tags, a quiet fortnight would have let the registry delete the exact versions production was
running. Serving pods survive that — they already pulled — but a restart, a reschedule, or a node replacement cannot
pull its image, and `ops ship` refuses a tag the registry no longer holds. A count cannot expire: the last ten releases
stay pullable however long the gap between them, so retention no longer depends on release cadence.

**The two policies are one unit.** Keep policies take precedence over delete policies in Artifact Registry, and that
precedence is the only thing making the pair mean "keep ten, delete the rest". The `DELETE` half matches every version
(`tagState: ANY`), so applying it without its `KEEP` partner would empty the repository on the next sweep. They are
written in a single PATCH for that reason, and a test asserts both halves.

`keepCount` is per package, so each image keeps its own last ten rather than ten versions across the repository. One
release pushes one version per image under two tags (`YY.M.D` and `latest`) — one digest, one version — so ten versions
is ten releases, and with one tag per calendar day it is also at least the last ten release days.

## Pin every consumed image, binary, and action

**Every consumed image, binary, and action is immutable.** Publishing `latest` is allowed; consuming it is not.

Embedded Rego policy is load-bearing: `cli/tests/regorus_policy.rs` compiles the production source and runs every
checked-in policy rule. The Regorus version and policy source ship together in the web binary; upgrading either is a
deliberate, tested change.

- **Images** (`image:`, `FROM`): pin an explicit version tag, never `latest` or another rolling tag, and confirm the tag
  still exists on the registry we pull from before pinning.
- **Installer binaries** (a workflow step's `version:`): pin the version, never `latest` — `latest` also round-trips a
  release API that has 500'd and killed a job.
- **Third-party GitHub Actions** (`uses:`): pin the full commit SHA with a trailing `# vX.Y.Z` comment, per GitHub's
  guidance — a bare `@v2` resolves to a branch tip upstream can force-push.

`navigator validate` rejects mutable consumption under `k8s/`, `examples/`, `images/`, and `.github/workflows/`;
`deploy.yml` publication sites are exempt.

## Publish vs. roll out

The publish jobs cannot mutate Google Cloud or a cluster, and neither can anything else in `deploy.yml`. Rolling is a
separate act under a separate identity — a person's, not the pipeline's.

Every roll target is a directory in the repository's `deployments/` tree: one `ops ship` run per directory, staging
first, then production, every deployment on the same tag.

Staging is the only gate on the way to production, and it is the only one that earns its place. It runs the same
`neon-server` image over a simulated data plane, so a failure there is evidence about the version rather than about real
people's matters — which is exactly what a canary has to be. Nothing rolls it on your behalf any more, so it is a step
the operator takes before the row clients are on rather than one a run reports.

**Publishing and rolling are separated because they answer to different things.** Publishing is mechanical: the
workspace either builds and passes its gates or it does not, which is a question a cron can settle. Rolling puts a
version in front of people whose legal matters are in it, which is a judgement — and the moment a green pipeline made
it, the record of what production runs stopped being a decision anyone took and became a side effect of whoever merged
last.

### The deploy is a human act

**No workflow in this repository can roll a cluster.** `deploy.yml` ends at the registry, holds no Google Cloud
credential, and requests no `id-token: write`. That is a security boundary rather than a preference: a pipeline that can
roll production is a pipeline whose compromise rolls production, and CI's remaining reach is a registry push.
`cli/tests/deploy_workflow.rs` asserts both halves — no job named `ship*`, and no credential exchange step — so
restoring that reach means deleting an assertion that says why it was removed.

The handoff is a Slack message. When a publish run goes green, `notify` posts two messages to `#navigator`: what was
published, then the `ops ship` command with the version already substituted, so it can be copied without editing.

That second message enumerates nothing. It once derived a line per row from the tree at run time (`ls deployments`) and
read each row's public host out of its `config.toml`; the tree moved, so both would now fail inside a Slack step whose
failure nobody reads as "the tree moved". It names no deployment either — a row is rollable because its directory
exists, and this repository cannot see whether one does, so every public instruction takes a placeholder.
`no_public_source_instructs_a_deployment_by_name` in `cli/src/devx/deployments.rs` fails the build if a name drifts back
in.

Nor does it name the repository that holds the tree. The `DEPLOY_REPO` Actions variable carries that, so renaming or
replacing the deploy repository is a variable change rather than a pull request here; unset, the message says "your
deployments checkout" instead of interpolating a blank. The coupling runs one way only — the deploy repository's own
workflow hardcodes this one and derives the release tag from its own clock, and nothing here reaches back.

Then a person runs it, from their own machine, against their own short-lived credentials:

```bash
gcloud auth application-default login
navigator ops ship --deployment <row> --deployments-dir . --tag YY.M.D
```

**Rolling back is the same command with an older version.** `ops ship` neither knows nor cares which direction a version
moves; it reconciles the deployment onto the tag it is given. The only requirement is that the images still exist — see
[Image retention](#image-retention), which keeps the last ten versions for exactly this.

### The manual deploy

To roll outside a release or promotion run — a re-roll, a rehearsal, a rollback, or a deployment neither workflow
reached — run one `ops ship` per directory:

```bash
navigator ops ship --deployment <row> --deployments-dir . --tag YY.M.D
```

`--deployment` is required and reads every coordinate from `deployments/<name>/config.toml` — never from the shell, so a
stale environment cannot select the wrong deployment. `ship` builds nothing. It validates the deployment's secrets,
reconciles manifests, rolls every service and trigger to one tag, and re-registers Restate. After a secret rotation, a
`--restart-only` ship restarts the pods without changing the version; that lane is manual only. See
[`cloud-operations.md`](cloud-operations.md) and [`gke-prod.md`](gke-prod.md#trust-boundary).

Forks that run a GitOps controller (Config Sync, Argo CD, Flux) can let the controller reconcile the manifests instead
of running `ship`. This repository has no controller and no deploying workflow: `ops ship`, run by a person, is the
whole rollout path.
