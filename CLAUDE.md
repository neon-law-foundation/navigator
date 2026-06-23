# Navigator workspace — agent rules

**Navigator** (Neon Law Navigator) is open source by the **Neon Law Foundation** under dual Apache-2.0 / MIT, run in
production by **Neon Law**, the law firm. The code is licensed; the names and marks are reserved (see the Trademarks
note in [`README.md`](README.md#trademarks)). Forks rebrand via the `navigator rebrand` white-label seam.

This file is the short list of rules. Each rule links to the doc that is its source of truth — read that doc before
acting on anything below, and keep the doc, not this file, authoritative.

## Architecture invariants

- **Rust only.** Every executable and library is Rust; the `navigator` CLI orchestrates every machine-bound flow
  (`start-dev-server`, `deploy`, `e2e`, `grant-staff`, `power-push`, …) — no shell scripts, no Makefile. New scripts are
  Rust binaries under `cli`, a new `cli` subcommand, or a new crate. If a task implies another language, push back —
  find a Rust equivalent or carve a clean seam to a separate repo. →
  [`docs/workspace-layout.md`](docs/workspace-layout.md)
- **English-first.** English is the only language of every binding or internal artifact: a legal **template body is
  English-only, no exceptions**, as are the portal UI, `/docs`, code, and comments. We localize in exactly two places —
  marketing pages (`/es` Tier-A + the mission letter) and questionnaire intake prompts (`question_translations`, which
  never bypass `staff_review`). Everything else stays English; push back on portal/`/docs`/email localization. →
  [`docs/i18n.md`](docs/i18n.md)
- **GCP, but provider-agnostic.** GCP-specific code is isolated to two crates: `cloud` (object storage behind the
  `StorageService` trait — `web` depends on `cloud`, never the GCP SDK) and `cli` (`gcp setup` project provisioning). DB
  (Cloud SQL Postgres), OIDC (Google Identity), and the per-Project git archive stay spec-compliant, not SDK-bound. Dev
  uses cloud-agnostic equivalents (`fake-gcs-server`, Keycloak). → [`docs/multi-cloud.md`](docs/multi-cloud.md),
  [`docs/gke-prod.md`](docs/gke-prod.md), [`docs/oss-install.md`](docs/oss-install.md),
  [`cloud/README.md`](cloud/README.md); per-Project repos in [`docs/git-project-repos.md`](docs/git-project-repos.md)
  (Google Drive ingest is removed).
- **Postgres everywhere.** Cloud SQL in prod, in-cluster in KIND, `testcontainers` in `cargo test`. No SQLite, no
  `APP_ENV`: `store::DbConfig::from_env` reads `DATABASE_URL` and errors when unset. Docker is required everywhere. →
  [`docs/test-database.md`](docs/test-database.md)
- **Secrets in Doppler.** Values for `.env.example` live in Doppler (`dev` / `prd`); prod renders to GCP Secret Manager.
  Doppler sits **above** the env-var interface — the workspace builds and runs with no Doppler account. →
  [`docs/secrets-doppler.md`](docs/secrets-doppler.md)
- **Toolchain.** Pinned in `rust-toolchain.toml`: Rust 1.96.0, edition 2021, clippy pedantic at warn, `unsafe_code =
  "forbid"`.

## How to work

- **Begin every session with the KIND loop and leave it up.** `cargo run --release -p cli -- start-dev-server` brings up
  Postgres, Keycloak, fake-gcs, OPA, Restate, and Grafana LGTM in KIND and writes `.devx/env`. Run `web` **under Doppler
  with `.devx/env` sourced after** (the KIND wiring must win) — `web` crash-loops on `.devx/env` alone because
  `enforce_prod_invariants` needs Doppler-only secrets:

  ```bash
  doppler run --project navigator --config dev -- \
    bash -c 'set -a; source .devx/env; set +a; cargo run -p web'
  ```

  Never point `web` at ad-hoc local services. Full recipe: the `web-preview` and `kind-local-dev` skills,
  [`docs/RUNBOOK.md`](docs/RUNBOOK.md). Local telemetry (Tempo/Loki/Prometheus at `localhost:3000`): the `grafana-lgtm`
  skill; emit-side seam: the `observability` skill and [`docs/observability.md`](docs/observability.md).
- **Machine-bound commands: run them directly when the environment is local and reversible.** Anything driving the KIND
  cluster, a local browser, the Docker daemon, or the workspace toolchain — `docker`, `kind`, `kubectl`, the `navigator`
  CLI subcommands, the browser e2e suite (including starting `chromedriver` + a Postgres port-forward, and
  rebuilding/redeploying the `navigator-web:dev` image) — **the agent may run these itself here.** Asked to "run the
  kind tests", bring up the harness it needs (chromedriver on `:9515`, a `kubectl` port-forward of `deployment/postgres`
  to `15432:5432`, `grant-staff`, and the CI env vars `NAV_BASE_URL` + `DATABASE_URL` + `NAV_REQUIRE_HARNESS=1`) and run
  `cargo test -p web --test browser_e2e`. Only *production* or *irreversible* cloud actions (`gcloud`, `power-push`, a
  real `deploy` to prod) stay propose-only — print the exact command and let the user prefix it with `!`.
- **Scratch output never lands in the working tree.** Screenshots and any throwaway file go under `/tmp` (e.g.
  `/tmp/navigator-screenshots/`, `mkdir -p` first), never the repo. Committed visual artifacts (e.g. `docs/erd.svg`) are
  the exception.
- **No assumptions; always test what you changed.** "It compiles" / "it looks right" is not evidence — read or run the
  real code path and observe the result. Add or run the covering test (TDD, same commit). Report faithfully: if you
  didn't test it, or a step failed or was skipped, say so with the output. → the `verify` and `run` skills.
- **Markdown lint before committing any `.md`.** Dogfood the CLI; never hand-roll a linter. Must exit `0`:

  ```bash
  cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>
  ```

  → the `markdown-lint` skill.

## Shipping — branch → PR → auto-merge

**Never commit directly to `main`** — it advances merge-only. Branch before the first edit, push, then open a PR and
enable auto-merge so GitHub lands it once CI is green:

```bash
git switch -c <kebab-topic>
git push -u origin <branch>
gh pr create
gh pr merge --auto --squash
```

Run the gate before committing (plus the markdown lint above if you touched any `.md`):

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI is exactly **three workflows, no fourth**. The full lifecycle — the flow every committing skill inherits, the
pre-commit gate, the three workflows, and pull-based deploy — is in [`docs/gitops.md`](docs/gitops.md).

**Reviewing a PR means resolving every comment.** A PR is not "reviewed" until each reviewer comment — Greptile,
CodeRabbit, any bot, any human — has been adjudicated against the real code and answered via the `gh` CLI: fixed and
replied, or acknowledged-with-rationale and replied, with real review threads marked resolved. Never leave a comment
hanging. The full recipe (read → assess → collect every comment → ask → fix → reply + resolve) is the `review-pr` skill.

## AIDA — the agent

**AIDA** is Navigator's domain agent: one tool catalog (`mcp/src/tools/`), two LLM-agnostic protocol surfaces — **A2A**
(`/api/aida.json`, `/api/aida/rpc`; routed by `web::agent_router::AgentRouter`) and **MCP** (`/mcp`; client-side LLM
routes). MCP keeps the `aida_` prefix; A2A strips it, and `web::a2a` bridges both (snapshot-tested in `web/src/a2a.rs`).
Swapping the router means a new `impl AgentRouter` selected from `web::build_router` — never fork the catalog. →
[`docs/aida-a2a-interaction.md`](docs/aida-a2a-interaction.md),
[`docs/gemini-enterprise-mcp.md`](docs/gemini-enterprise-mcp.md).

## Where to find things

- [`docs/`](docs/) — the workspace doc tree, published verbatim at `/docs/:slug`. Start here:
  [workspace-layout](docs/workspace-layout.md) (crate map), [gitops](docs/gitops.md) (ship + deploy),
  [glossary](docs/glossary.md) (vocabulary), [oidc](docs/oidc.md) (authz), [RUNBOOK](docs/RUNBOOK.md) (KIND).
- `web/content/marketing/mission.md` — why this project exists (live at `/foundation/mission`). Every product decision
  should be justifiable against it.
- `README.md` — workspace overview, install, demo. `cli/README.md` — per-subcommand reference.
- `.claude/skills/council/` — the Council of Twelve architecture-review pattern (`/council`).
- `k8s/` — KIND manifests. `templates/` — notation templates. `store/seeds/` — canonical reference-data YAML.

## Local-only convention: `prompts/`

The gitignored `prompts/` directory holds draft briefs and multi-session kickoff texts, named by topic. **Future
sessions won't see these** unless the user pastes them back — the hand-off is the prompt text, not repo state. **Do not
commit prompts.** If a prompt encodes a decision worth keeping, lift it into code, a doc, a skill, or the glossary.
