# Neon Law Navigator workspace — agent rules

**Neon Law Navigator** (Neon Law Navigator) is open source by the **Neon Law Foundation** under dual Apache-2.0 / MIT,
run in production by **Neon Law**, the law firm. The code is licensed; the names and marks are reserved (see the
Trademarks note in [`README.md`](README.md#trademarks)). Forks rebrand via the `navigator rebrand` white-label seam.

This file is the short list of rules. Each rule links to the doc that is its source of truth — read that doc before
acting on anything below, and keep the doc, not this file, authoritative.

## Architecture invariants

- **Rust only.** Every executable and library is Rust; the `navigator` CLI orchestrates every machine-bound flow
  (`start-dev-server`, `deploy`, `e2e`, `grant-staff`, `ship`, …) — no shell scripts, no Makefile. New scripts are Rust
  binaries under `cli`, a new `cli` subcommand, or a new crate. If a task implies another language, push back — find a
  Rust equivalent or carve a clean seam to a separate repo. → [`docs/workspace-layout.md`](docs/workspace-layout.md) -
  **English-first.** English is the only language of every binding or internal artifact: a legal **template body is
  English-only, no exceptions**, as are the portal UI, `/docs`, code, and comments. We localize in exactly two places —
  marketing pages (`/es` Tier-A + the mission letter) and questionnaire intake prompts (`question_translations`, which
  never bypass `staff_review`). Whenever an English marketing or public Foundation page changes, update its Spanish
  counterpart in the same PR; do not leave Spanish as a follow-up. Everything else stays English; push back on
  portal/`/docs`/email localization. → [`docs/i18n.md`](docs/i18n.md)
- **GCP, but provider-agnostic.** GCP-specific code is isolated to two crates: `cloud` (object storage behind the
  `StorageService` trait — `web` depends on `cloud`, never the GCP SDK) and `cli` (`gcp setup` project provisioning). DB
  (Cloud SQL Postgres), OIDC (Google Identity), and the per-Project git archive stay spec-compliant, not SDK-bound. Dev
  uses cloud-agnostic equivalents (`fake-gcs-server`, Keycloak). → [`docs/multi-cloud.md`](docs/multi-cloud.md),
  [`docs/gke-prod.md`](docs/gke-prod.md), [`docs/oss-install.md`](docs/oss-install.md),
  [`docs/cloud-operations.md`](docs/cloud-operations.md), [`cloud/README.md`](cloud/README.md), and
  [`docs/git-project-repos.md`](docs/git-project-repos.md) (per-Project repos; Google Drive ingest is removed).
- **Postgres everywhere.** Cloud SQL in prod, in-cluster in KIND, `testcontainers` in `cargo test`. No SQLite, no
  `APP_ENV`: `store::DbConfig::from_env` reads `DATABASE_URL` and errors when unset. Docker is required everywhere. →
  [`docs/test-database.md`](docs/test-database.md)
- **Secrets in Doppler.** Values for `.env.example` live in Doppler (`dev` / `prd`); prod renders to GCP Secret Manager.
  Doppler sits **above** the env-var interface — the workspace builds and runs with no Doppler account. →
  [`docs/secrets-doppler.md`](docs/secrets-doppler.md)
- **Toolchain.** Pinned in `rust-toolchain.toml`: Rust 1.96.0, edition 2021, clippy pedantic at warn, `unsafe_code =
  "forbid"`.

## How to work

- **Every task is an issue, a PR, or a review.** The agent does exactly three things, and every codebase task is one of
  them: (1) **groom an issue** — read a GitHub issue from its first comment forward, follow how the ask evolved, then
  comment the extra plan detail the work needs, so a future worktree starts from the issue and not from leftover local
  state; (2) **create a PR** — actuate that issue into code in a dedicated worktree through test-driven steps, then open
  the PR; (3) **review a PR** — pull a PR number into a local worktree and resolve every reviewer comment iteratively,
  making the authorized code changes and pushing them back. Everything else — Markdown lint, Restate, legal-workflow
  authoring, Rust conventions, and council review — is supporting context inside one of those three, never a fourth
  action. Production and host operations (prod deploy, prod-db access, disk/Docker maintenance, dependency bumps) sit
  outside this loop and stay propose-only. Start with [`docs/agent-workflows.md`](docs/agent-workflows.md), then follow
  [`docs/index.md`](docs/index.md) to the narrowest source.
- **Use the KIND loop for full-stack local testing, then clean it up.** `cargo run --release -p cli -- start-dev-server`
  brings up Postgres, Keycloak, fake-gcs, OPA, Restate, and Grafana LGTM in KIND and writes `.devx/env`. Run `web`
  **under Doppler with `.devx/env` sourced after** (the KIND wiring must win) — `web` crash-loops on `.devx/env` alone
  because `enforce_prod_invariants` needs Doppler-only secrets:

  ```bash
  doppler run --project navigator --config dev -- \
    bash -c 'set -a; source .devx/env; set +a; cargo run -p web'
  ```

  Never point `web` at ad-hoc local services. Full recipe: [`docs/RUNBOOK.md`](docs/RUNBOOK.md) and
  [`docs/cloud-operations.md`](docs/cloud-operations.md). Local telemetry (Tempo/Loki/Prometheus at `localhost:3000`):
  [`docs/observability.md`](docs/observability.md). The KIND **dependency tier is a persistent fixture** — leave the
  cluster up across sessions and re-run `start-dev-server` to restore port-forwards after a sleep/reboot (it reuses the
  existing cluster). At handoff stop only the host-side `web` and task-owned Docker/build artifacts; full `down` is for
  a deliberate clean rebuild, not routine cleanup. →
  [`docs/RUNBOOK.md`](docs/RUNBOOK.md#keep-the-deps-up-across-sessions-the-persistent-fixture).
- **Machine-bound commands: run them directly when the environment is local and reversible.** Anything driving the KIND
  cluster, a local browser, the Docker daemon, or the workspace toolchain — `docker`, `kind`, `kubectl`, the `navigator`
  CLI subcommands, the browser e2e suite (including starting `chromedriver` + a Postgres port-forward, and
  rebuilding/redeploying the `navigator-web:dev` image) — **the agent may run these itself here.** Asked to "run the
  kind tests", bring up the harness it needs (chromedriver on `:9515`, a `kubectl` port-forward of `deployment/postgres`
  to `15432:5432`, `grant-staff`, and the CI env vars `NAV_BASE_URL` + `DATABASE_URL` + `NAV_REQUIRE_HARNESS=1`) and run
  `cargo test -p web --test browser_e2e`. Only *production* or *irreversible* cloud actions (`gcloud`, `ship`, a real
  `deploy` to prod) stay propose-only — print the exact command and let the user prefix it with `!`.
- **Run `web` and every supporting service on a unique port.** Never bind to a shared default and assume it is free — a
  second worktree or session will collide on it. `navigator worktree-env up` already derives a per-worktree `web` port
  (`WEB_PORT_BASE` 3001 + a hash of the worktree slug, in `cli/src/devx/worktree_env.rs`); prefer it, and pick a unique
  port the same way for anything you start by hand (`web`, chromedriver, a Postgres port-forward, the OIDC / OPA /
  storage deps). Record the ports you chose so the rest of the task — and the next agent — can find them.
- **Scratch output never lands in the working tree.** Screenshots and any throwaway file go under `/tmp` (e.g.
  `/tmp/navigator-screenshots/`, `mkdir -p` first), never the repo. Committed visual artifacts (e.g. `docs/erd.svg`) are
  the exception.
- **Clean up task resources before ending.** After creating or updating a PR, clean worktree build artifacts created by
  Cargo, stop the host-side `web` and task-owned browser processes, and prune task-created Docker build cache/images.
  Leave the KIND dependency cluster running (it is a persistent fixture); never prune Docker volumes without explicit
  user approval. → [`docs/agent-workflows.md`](docs/agent-workflows.md#resource-cleanup)
- **No assumptions; always test what you changed.** "It compiles" / "it looks right" is not evidence — read or run the
  real code path and observe the result. Add or run the covering test (TDD, same commit). Report faithfully: if you
  didn't test it, or a step failed or was skipped, say so with the output.
- **Document the present, not the past.** Code, comments, tests, and docs describe what the system does *today*. Git
  history is the record of how we got here — don't duplicate it in the tree. Delete vestigial references to superseded
  decisions ("we used to…", "no longer…", "legacy", deprecated-but-kept flags/aliases) instead of annotating them, and
  when you change behavior remove the old code rather than leaving a compatibility shim. Keep only history that still
  informs a current reader (the *why* behind a live invariant) and guard tests that assert today's behavior (an old URL
  must 404 or redirect). → [`docs/rust-programming.md`](docs/rust-programming.md).
- **Markdown lint before committing any `.md`.** Dogfood the CLI; never hand-roll a linter. Must exit `0`:

  ```bash
  cargo run -p cli --quiet -- validate --no-default-excludes <path>
  ```

  `validate` classifies each file automatically (prose, notation template, event, blog post, board minutes). Frontmatter
  keys per kind are documented for attorneys in [`docs/frontmatter.md`](docs/frontmatter.md). →
  [`docs/agent-workflows.md`](docs/agent-workflows.md).
- **Use the three decision councils when the decision earns them.** Engineering Council for architecture and doc
  clarity, Legal Council for legal copy before it becomes a Notation/template/prompt/email, and Client Council for
  client-facing product, intake, pricing, onboarding, and portal decisions. Default to the smallest useful bench and
  read the real source first. → [`docs/agent-decision-councils.md`](docs/agent-decision-councils.md).

## Shipping — create PR or review/update PR

**Never commit directly to `main`** — it advances merge-only. Create a dedicated worktree and topic branch before the
first edit, push, then open a PR. GitHub's merge queue lands PRs targeting `main` once the required checks pass (CI
enables auto-merge on open, which enqueues the PR):

```bash
git worktree add -b <kebab-topic> .worktrees/<kebab-topic> origin/main
git push -u origin <kebab-topic>
gh pr create
```

Run the Rust gate before committing when Rust files or build/runtime configuration changed:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For docs-only changes with no Rust files changed, run only the Markdown gate for the touched docs.

CI/CD is exactly **three workflows** (`ci` / `release-tag` / `deploy`) — don't fold new gate logic into a fourth.
Periodic housekeeping is the one carve-out: it lives in a separate **maintenance** workflow (`cleanup.yml`, daily ghcr
retention) on its own cron, outside the CI/CD gate. The full lifecycle — the branch/PR discipline, pre-commit gate, the
workflows, and pull-based deploy — is in [`docs/gitops.md`](docs/gitops.md).

**Reviewing a PR means resolving every comment.** A PR is not "reviewed" until each reviewer comment — Greptile,
CodeRabbit, any bot, any human — has been adjudicated against the real code and answered via the `gh` CLI: fixed and
replied, or acknowledged-with-rationale and replied, with real review threads marked resolved. Never leave a comment
hanging. The full recipe (read → assess → collect every comment → ask → fix → reply + resolve) is in
[`docs/agent-workflows.md`](docs/agent-workflows.md).

## AIDA — the agent

**AIDA** is Neon Law Navigator's domain agent: one tool catalog (`mcp/src/tools/`), two LLM-agnostic protocol surfaces —
**A2A** (`/api/aida.json`, `/api/aida/rpc`; routed by `web::agent_router::AgentRouter`) and **MCP** (`/mcp`; client-side
LLM routes). MCP keeps the `aida_` prefix; A2A strips it, and `web::a2a` bridges both (snapshot-tested in
`web/src/a2a.rs`). Swapping the router means a new `impl AgentRouter` selected from `web::build_router` — never fork the
catalog. → [`docs/aida-a2a-interaction.md`](docs/aida-a2a-interaction.md),
[`docs/gemini-enterprise-mcp.md`](docs/gemini-enterprise-mcp.md).

## Where to find things

- [`docs/`](docs/) — the workspace doc tree, published verbatim at `/docs/:slug`; [`docs/index.md`](docs/index.md) is
  the full index. Start with [workspace-layout](docs/workspace-layout.md), [gitops](docs/gitops.md),
  [glossary](docs/glossary.md), [access-model](docs/access-model.md),
  [agent-decision-councils](docs/agent-decision-councils.md), [agent-workflows](docs/agent-workflows.md),
  [cloud-operations](docs/cloud-operations.md), [rust-programming](docs/rust-programming.md), [oidc](docs/oidc.md), and
  [RUNBOOK](docs/RUNBOOK.md).
- `web/content/marketing/mission.md` — why this project exists (live at `/foundation/mission`). Every product decision
  should be justifiable against it.
- `README.md` — workspace overview, install, demo. `cli/README.md` — per-subcommand reference. `k8s/` — KIND manifests.
  `templates/` — notation templates. `store/seeds/` — canonical reference-data YAML.

## Planning lives in GitHub issues — action 1, groom an issue

Grooming an issue is the agent's first action: read the issue from its opening comment forward, follow how the ask
evolved, and comment the extra plan detail — scope, the test-driven steps, the files in blast radius — that a future
worktree will start from. Planning — design briefs, multi-session kickoffs, the working notes behind a change — lives in
**GitHub issues**, not a local folder. An issue is visible to the whole team and to every future session (a worktree
starts from the issue, not from leftover local state), so reference the issue by number in the branch, the PR, and any
commit that needs the context. When an issue encodes a decision worth keeping past the work, lift it into code, a doc,
or the glossary — the issue is the working space, the repo is the durable record.

## Agent environment notes

### Pull request walkthrough artifacts

When a change affects public UI or portal UI, **always** capture a **live** walkthrough from the running app and put it
in the PR body, not only in the final agent reply. Rendering tests are not a substitute — boot `web` against the
persistent KIND deps (usually already up; if a port-forward died, re-run `start-dev-server`) and capture the real served
page with the agent's local headless browser, Playwright, or a screen recorder. Prefer one short demo video or GIF plus
the clearest screenshots that prove the changed states. Save them under `/tmp/navigator-screenshots/`, then embed each
with the `pr-image-upload` skill (it drives `gh image` → GitHub's `user-attachments` CDN and returns a real URL for the
body) — a raw `<img src="/tmp/…">` renders broken on github.com. **Never** self-host the binary on a remote git branch
or commit it to the tree. If the artifact genuinely cannot be embedded, include the generated path and briefly explain
why, but still produce the live walkthrough when practical.

### Login + authz

Keycloak realm `navigator` ships one user: `staff` / `staff`. First Keycloak login prompts for a last name (the realm
import omits it). The authz tier is DB-sourced (`persons.role`), not from Keycloak — a person is created as `client` on
first login. To reach `/portal/admin/*`, pre-seed or promote the row to `staff`/`admin` (see
[`docs/RUNBOOK.md`](docs/RUNBOOK.md) §3), e.g. `UPDATE persons SET role='staff' WHERE email='staff@neonlaw.com';`
(re-login to refresh the session role).
