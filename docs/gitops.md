# GitOps: edit → merge → release → deploy

Navigator's entire lifecycle hangs off one branch — `main`. Every change reaches it the same way (a PR that
auto-merges), `main` is what the production cluster pulls, and the daily release rides off `main`'s history. This doc is
the source of truth for that flow; the workspace `CLAUDE.md` carries only the short rules and links here.

## `main` is sacred and merge-only

- **Never commit directly to `main`.** It advances solely through pull requests — there is no direct push, ever.
- **`main` is what production runs.** The GKE cluster's Config Sync pulls `examples/deploy/k8s/gke` from `main` (see
  [`gke-prod.md`](gke-prod.md)), and the nightly release tag is cut from `main`'s tip. A bad merge to `main` is a
  production concern, not just a code-review one.

## The branch → PR → auto-merge flow

Every task — agent or human — follows the same three steps. No skill invents its own branch ceremony; they all inherit
this.

1. **Branch.** Before the first edit, create a topic branch: `git switch -c <kebab-topic>` (e.g.
   `git switch -c daily-cd-pipeline`). If you find yourself on `main` with uncommitted work, branch first and carry the
   changes over — never commit them to `main`.
2. **Push + open a PR.** `git push -u origin <branch>` then `gh pr create`.
3. **Enable auto-merge.** `gh pr merge --auto --squash`. GitHub squash-merges the moment every required check goes
   green — you do not babysit the merge or merge by hand.

**Auto-merge is a GitHub-native repo setting, not a workflow** — which is why the three workflows below still suffice.

### TDD and the pre-commit gate

- Tests land in the **same commit** as the implementation they cover.
- Always run before committing:

  ```bash
  cargo fmt
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

  Plus the markdown lint (`cargo run -p cli -- validate --markdown-only --no-default-excludes <path>`) if you touched
  any `.md` file.

## CI/CD — three workflows, no more

GitHub Actions carries exactly **three** workflows, one per trigger. Do not add a fourth; fold any new automation into
the matching one.

| Workflow | Trigger | Job |
| --- | --- | --- |
| [`ci.yml`](../.github/workflows/ci.yml) | `pull_request` → `main` | lean fmt + clippy + `cargo test --workspace` |
| [`release-tag.yml`](../.github/workflows/release-tag.yml) | cron 02:00 PST | cut + push the `YY.MM.DD` tag |
| [`deploy.yml`](../.github/workflows/deploy.yml) | `YY.MM.DD` tag push | integration → push images → email report |

### PR flow — `ci.yml`

Runs only on every `pull_request` targeting `main` — **never on `push`**, so `main` itself runs no CI on merge (it
advances merge-only, and the heavy paths ride the release tag). Lean by design: a format check, a clippy pass with
warnings as errors, then the workspace test suite — nothing else. The job keeps target artifacts out of the cache,
disables CI debug info, and runs `cargo clean` between clippy and test so the standard hosted runner has enough disk.
One shared `postgres:17-alpine` container backs the whole job via `TEST_DATABASE_URL` (so `store::test_support` makes a
per-test schema in that single container instead of spawning a testcontainer per binary).
Integration/KIND/docker/browser work does **not** run here.

### Cron flow — `release-tag.yml`

Fires daily at **02:00 PST** (`0 10 * * *` UTC). Its only job is to cut a calendar release tag `YY.MM.DD` (e.g.
`26.06.18` for 2026-06-18) and push it with a PAT (`secrets.RELEASE_PAT`) so the push re-triggers the tag flow below.

### Tag flow — `deploy.yml`

Triggered by the `YY.MM.DD` tag push. Runs the full **KIND integration** suite, then builds and pushes every image — the
two service images (`navigator-web`, `navigator-workflows-service`) and the five CronJob trigger images
(`navigator-*-trigger`) — to **ghcr.io** tagged with that date, then emails a deploy report to `nick@neonlaw.com` via
SendGrid (from `support@neonlaw.com`, the `DEFAULT_FROM_EMAIL` in `workflows/src/email/service.rs`; key in
`secrets.SENDGRID_API_KEY`).

## Publish vs. roll out

The tag flow **publishes** dated images to ghcr.io; it does **not** roll them onto the cluster. Promoting a dated image
to production is a separate, deliberate step — either a Config Sync reconcile or the operator-driven `power-push` (the
[`power-push`](../.claude/skills/power-push/SKILL.md) skill). The cluster's pull-based, credential-free delivery is
documented in [`gke-prod.md`](gke-prod.md#trust-boundary).
