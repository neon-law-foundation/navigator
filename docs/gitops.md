# GitOps: edit → merge → release → deploy

Neon Law Navigator's entire lifecycle hangs off one branch — `main`. Every change reaches it the same way (a PR that
auto-merges), `main` is what the production cluster pulls, and the daily release rides off `main`'s history. This doc is
the source of truth for that flow; the workspace `CLAUDE.md` carries only the short rules and links here.

For agents, this collapses to two codebase actions: create a PR, or review/update an existing PR. The branch ceremony,
test gate, release tag, and deploy hand-off are all supporting steps inside those actions.

## `main` is sacred and squash-merge-only

- **Never commit directly to `main`.** It advances solely through pull requests — there is no direct push, ever. **Every
  PR lands by squash.** Squash is the *only* merge strategy: each PR collapses to exactly one commit on `main`,
  regardless of how many commits (or `Merge branch 'main'` commits) the branch carried. Merge commits and rebase-merge
  are disabled on the repo — there is no other way to land. So `main`'s history is one linear commit per PR, and a
  branch's internal history never reaches it.
- **`main` is what production runs.** The GKE cluster's Config Sync pulls `examples/deploy/k8s/gke` from `main` (see
  [`gke-prod.md`](gke-prod.md)), and the nightly release tag is cut from `main`'s tip. A bad merge to `main` is a
  production concern, not just a code-review one.

## The branch → PR → merge queue flow

Every task — agent or human — follows the same three steps. No workflow invents its own branch ceremony; they all
inherit this.

1. **Branch.** Before the first edit, create a topic branch: `git switch -c <kebab-topic>` (e.g.
   `git switch -c daily-cd-pipeline`). If you find yourself on `main` with uncommitted work, branch first and carry the
   changes over — never commit them to `main`.
2. **Push + open a PR.** `git push -u origin <branch>` then `gh pr create`.
3. **Let the merge queue land it.** Merging is hands-off through **GitHub's native merge queue** (configured on the
   `main` branch ruleset, squash method, batch size 1). `ci.yml`'s `enable-automerge` job turns on GitHub auto-merge
   when the PR opens, which enqueues it the moment the gate is green and every review thread is resolved. The queue then
   builds a temporary `gh-readonly-queue/main/...` ref — `main` rebased + your PR alone — fires a `merge_group` event
   that re-runs the full `cargo test (workspace)` suite against that **rebased** commit (the exact tree that will land),
   and squash-merges only if it passes. So the suite runs against the real merge result on every rebase, never just the
   stale PR branch. You do not babysit the merge, rebase by hand, or click "Merge when ready". The whole PR becomes one
   commit on `main`; write the PR title as the Conventional Commit you want in `main`'s history, since that title (not
   the branch's individual commits) is the squashed commit's subject.

> **Auto-merge runs as a GitHub App, not `GITHUB_TOKEN`.** The `enable-automerge` job mints an installation token via
> [`actions/create-github-app-token`](https://github.com/actions/create-github-app-token) and enables auto-merge with
> that. This is deliberate: GitHub's docs state that "events triggered by the `GITHUB_TOKEN` will not create a new
> workflow run"
> ([Triggering a workflow](https://docs.github.com/en/actions/using-workflows/triggering-a-workflow)),
> so a PR the default token enqueues never fires `merge_group`, the required `cargo test (workspace)` check never
> reports, and the entry times out and is silently dropped — the PR sits "auto-merge enabled, CLEAN" forever (see
> [community discussion #70310](https://github.com/orgs/community/discussions/70310)). A GitHub App token (or a PAT) is
> the documented fix. The job is guarded on the App being configured: set `AUTOMERGE_APP_ID` and
> `AUTOMERGE_APP_PRIVATE_KEY` in the `navigator-gitops` Doppler config, which auto-syncs to GitHub Actions **secrets**
> (this repo keeps no Actions *variables* — Doppler is the single source of truth, so the App ID is stored as a secret
> too and hoisted to job-level `env` so the step `if` can gate on it). The App is installed on the repo with
> `contents: write` + `pull_requests: write`. Until both exist the step is skipped and the job falls back to
> `GITHUB_TOKEN` — which enables auto-merge but does not reliably enqueue, so the first PR after each fresh setup may
> need a one-time manual nudge (`gh pr merge <n>` as a real user).
>
> One wrinkle: a Dependabot-opened PR runs with the **Dependabot** secret store, not the Actions one, so the same two
> App secrets must also be set there or the bump PR enables auto-merge but never enqueues. NeonLaw mirrors them once;
> forks consume the published GHCR image and never hit this.

### TDD and the pre-commit gate

- Tests land in the **same commit** as the implementation they cover. When a PR changes Rust files or build/runtime
  configuration, run before committing:

  ```bash
  cargo fmt
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

- When a PR changes only Markdown or other prose files and no Rust files changed, the full Rust suite is not required.
  Run the Markdown gate for the touched docs instead:

  ```bash
  cargo run -p cli -- validate --markdown-only --no-default-excludes <path>
  ```

- After the PR is created or updated, clean task-owned build and e2e resources. `cargo clean` the task worktree when
  Rust commands created local build artifacts, stop the KIND/dev stack you started, and prune task-created Docker build
  cache or images. Do not prune Docker volumes without explicit approval.

## CI/CD — three workflows, plus maintenance

GitHub Actions carries exactly **three CI/CD** workflows, one per trigger — do not fold new gate logic into a fourth.
Periodic housekeeping is the one carve-out: it lives in a separate **maintenance** workflow on its own cron, outside the
CI/CD path, so a retention change never lands in a release diff and a cleanup run never shares state with a deploy.

| Workflow | Trigger | Job |
| --- | --- | --- |
| [`ci.yml`](../.github/workflows/ci.yml) | `pull_request` → `main` | fmt + Markdown CLI + clippy + tests |
| [`release-tag.yml`](../.github/workflows/release-tag.yml) | cron 02:00 PST | cut + push the `YY.MM.DD` tag |
| [`deploy.yml`](../.github/workflows/deploy.yml) | tag push or dispatch | integration → push images → Slack |
| [`cleanup.yml`](../.github/workflows/cleanup.yml) | cron 07:00 PST | prune ghcr versions > 14 days (maintenance) |

### PR flow — `ci.yml`

Runs only on every `pull_request` targeting `main` — **never on `push`**, so `main` itself runs no CI on merge (it
advances merge-only, and the heavy paths ride the release tag). Lean by design: a format check, a repository-wide
Markdown validation pass through the `navigator` CLI, a clippy pass with warnings as errors, then the workspace test
suite — nothing else. The Markdown pass builds `navigator` once and runs the local debug binary with `validate
--no-default-excludes .`, so ordinary docs get prose Markdown rules and notation templates get the stricter
questionnaire/workflow/template rule set. The job keeps target artifacts out of the cache, disables CI debug info, and
runs `cargo clean` between clippy and test so the standard hosted runner has enough disk. It still uses two
Rust-specific caches: `Swatinem/rust-cache` restores Cargo's registry, git, and tool caches, while `sccache` stores
reusable rustc outputs in GitHub Actions cache. That gives successive PRs a compiler cache without restoring the full
`target/` tree that previously exhausted runner disk. One shared `postgres:17-alpine` container backs the whole job via
`TEST_DATABASE_URL` (so `store::test_support` makes a per-test schema in that single container instead of spawning a
testcontainer per binary). Integration/KIND/docker/browser work does **not** run here.

### Cron flow — `release-tag.yml`

Fires daily at **02:00 PST** (`0 10 * * *` UTC). Its only job is to cut a calendar release tag `YY.MM.DD` (e.g.
`26.06.18` for 2026-06-18) and push it with a PAT (`secrets.RELEASE_PAT`) so the push re-triggers the tag flow below.

### Tag flow — `deploy.yml`

Triggered by the `YY.MM.DD` tag push, or manually with `workflow_dispatch` when an operator needs another publish during
the same day. The nightly path keeps the plain calendar tag. A manual dispatch derives a Pacific-time `YY.MM.DD.HH` tag,
so a run on June 25, 2026 at 2 p.m. publishes `26.06.25.14` instead of overwriting `26.06.25`. Either path runs the full
**KIND integration** suite, then builds and pushes every image — the two service images (`navigator-web`,
`navigator-workflows-service`) and the five CronJob trigger images (`navigator-*-trigger`) — to **ghcr.io** tagged with
that release version plus `latest`. The service images publish as linux/amd64 + linux/arm64 manifest lists from native
runners, so Apple-Silicon KIND pulls do not rely on emulation; the trigger images stay amd64-only because their CronJobs
are rare local paths and their shared Dockerfile is x86_64-pinned. In parallel with image publishing, it builds the
public `navigator` CLI and `navigator-lsp` binaries on native Linux, macOS, and Windows runners, records GitHub artifact
attestations for the downloadable archives, and attaches those six archives to the GitHub Release for that version. Once
the Release is up, a `homebrew-tap` job cross-pollinates the public
[neon-law-foundation/homebrew-tap](https://github.com/neon-law-foundation/homebrew-tap) repo: it recomputes the macOS
checksum and rewrites only the `navigator` CLI formula to the new release version, then pushes it with the gitops PAT
`secrets.GHCR_CLEANUP_PAT` (which must carry `contents:write` on the homebrew-tap repo) so that invoking `brew install
neon-law-foundation/tap/navigator` always tracks the latest build. The published formula is Apple-Silicon only, matching
the single macOS binary the release builds. Separately, the Zed extension flow pulls the matching `navigator-lsp`
release binary for editor installs. On success it posts a **"ready to deploy"** message to the engineering Slack channel
(the prod ops incoming webhook, `secrets.SLACK_WEBHOOK_URL`, synced from Doppler), tagging Nick with the exact `ship`
command to roll the new images to prod; a failure on any stage posts a separate alert to the same channel, also tagging
Nick. The images are published, **not** rolled out — see [Publish vs. roll out](#publish-vs-roll-out) below.

### Maintenance flow — `cleanup.yml`

Separate from the CI/CD three, on its own cron and knowing nothing about tags. Fires daily at **07:00 PST** (15:00 UTC)
— five hours after the tag cut, so the day's fresh images already exist — and prunes ghcr: it discovers every
`navigator-*` container package through GitHub's package API, then deletes versions older than 14 days through `gh api`
authenticated with `secrets.GHCR_CLEANUP_PAT`. That secret must be a classic PAT from an org/package admin with
`read:packages` and `delete:packages`; fine-grained PATs cannot list org packages through this endpoint. `latest` and
the recent dated tags are re-pushed daily by `deploy.yml`, so their versions stay under the cutoff and only stale images
are swept. It then posts a Slack summary, tagging Nick on failure. New scheduled maintenance belongs here, not in a
CI/CD workflow.

## Publish vs. roll out

The tag flow **publishes** dated images to ghcr.io; it does **not** roll them onto the cluster. **There is no automatic
production rollout, by design** — promoting a dated image to prod is a separate, deliberate, operator-driven step. This
keeps every cluster mutation in the hands of a human at a trusted, authenticated workstation: GitHub Actions holds no
GCP credential, no cluster access, and no path to write to prod.

### The manual deploy

When the **"ready to deploy"** Slack message lands (the green-deploy hand-off from `deploy.yml`), an operator rolls the
published image onto the GKE cluster with `ship` — this is the exact command the Slack message hands you, with the date
filled in:

```bash
doppler run --project navigator --config prd -- \
  cargo run --release -p cli -- ship --tag YY.MM.DD
```

`ship` builds **nothing** — the images already exist from the tag flow. It takes the `--tag` you pass (it never guesses
the latest), confirms the prod Secret satisfies the new binary's boot invariants, pins **both** deployments
(`navigator-web` and `workflows-service`) plus the trigger CronJobs to that tag, rolls them out together, and
re-registers the worker with Restate. The full recipe — the pre-roll Secret check, the manifest-drift guard, and the
no-rebuild restart path for a bare secret rotation — lives in [`cloud-operations.md`](cloud-operations.md). The
cluster's pull-based, credential-free image delivery is documented in [`gke-prod.md`](gke-prod.md#trust-boundary).

Forks that run a GitOps controller (Config Sync, Argo CD, Flux) can let the controller reconcile the overlay instead of
running `ship` by hand; this repo's production roll is the manual `ship` above.
