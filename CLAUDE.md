# Navigator workspace — agent rules

**Navigator** is short for **Neon Law Navigator**: published as open source by the **Neon Law Foundation** under a dual
Apache-2.0 / MIT license (at your option) and run in production by **Neon Law**, the law firm. The code is licensed; the
names and marks ("Neon Law", "Neon Law Foundation", "Navigator", and the NLF logo) are reserved — see the Trademarks
note in [`README.md`](README.md#trademarks). Forks ship under their own name via the `navigator rebrand` white-label
seam.

## Language: Rust only

Every executable and library in this workspace is written in **Rust**. There are no shell scripts and no Makefile — the
`navigator` CLI (the `cli` crate) orchestrates every machine-bound flow: `start-dev-server`, `deploy`, `e2e`,
`grant-staff`, `power-push`, and the rest.

Concretely:

- **Browser-driven tests** use `fantoccini` (WebDriver) — see `web/tests/browser_e2e.rs`.
- **Migration tooling** is `sea-orm-migration` invoked from Rust binaries.
- **Build orchestration** is `cargo` first.
- **CI workflows** (`.github/workflows/`) shell out to `cargo`, the `navigator` CLI, and `kubectl`. They never embed
  application logic — the smoke check and staff-seed are `cli e2e` / `cli grant-staff`, not bash.
- New scripts should be Rust binaries under `cli`, a new `cli` subcommand, or a new workspace crate.

If a task description implies adding another language, push back. Either find a Rust equivalent or carve a clean seam to
a separate repo.

## Human language: English-first

English is the official language of this product. It is the default for every surface and the **only** language of every
binding or internal artifact: the legal **template bodies** in `templates/` (and therefore the notation a client signs),
the portal UI, the `/docs` tree, code, and comments. We localize narrowly and deliberately — out of respect for the
people the mission serves, not as a general policy — in exactly two places:

- **Marketing pages** may be translated to reach prospective clients in their own language (the `/es` Tier-A pages plus
  the mission letter; attorney-reviewed in-language — see [`docs/i18n.md`](docs/i18n.md)). These are the only
  fully-localized *pages* in the app.
- **Questionnaire intake prompts** may carry attorney-reviewed localized variants (the `question_translations` table)
  so a client can understand the question they are answering. This is field-level accessibility, not a translated page,
  and it never bypasses the `staff_review` gate.

Everything else stays English. A legal **template body is English-only, no exceptions** — the binding artifact a client
signs is in English even when the questionnaire that gathered the answers was localized. Do not add portal, `/docs`, or
transactional-email localization; if a task implies it, push back and keep the surface English.

## Cloud: GCP only

GCP-specific code is isolated to two crates so the rest of the workspace stays provider-agnostic:

- **`cloud`** owns object storage. Depends on `google-cloud-storage`; exposes the `StorageService` trait that every
  other crate consumes. `web` depends on `cloud`, never on the GCP SDK directly.
- **`cli`** owns project provisioning. The `cli gcp setup --project-id <ID>` subcommand talks to Google's REST APIs
  via `google-cloud-auth` + `reqwest` to stand up VPC, Cloud SQL, GCS buckets, and Cloud Run on a fresh project. The
  REST plumbing lives in `cli/src/devx/gcp/` (the orchestration code collapsed in from the former `devx` binary). Apart
  from `archives`' BigQuery client, nothing else in the workspace depends on `google-cloud-auth`.

Other GCP touchpoints stay spec-compliant rather than SDK-bound:

- **Database** → Cloud SQL for Postgres (`web` speaks vanilla Postgres via SeaORM; the GCP-specific piece is the
  connection URL).
- **OIDC** → Cloud Identity / Google Identity Services (the OIDC flow itself is provider-agnostic; the deploy story uses
  Google's discovery doc).
- **Per-Project archive** → every Project is an append-only, single-branch (`main`) git repository served Rust-native
  from `web` at `/projects/<id>.git`, gated by a `web`-minted Personal Access Token + the existing project ACL, with Git
  LFS objects in `cloud::StorageService` (the `repos` crate, `web::git_http` + `web::git_lfs`,
  `store::git_access_tokens`, `cli git`). The full rationale, councils, and storage/auth/retention decisions are in
  [`docs/git-project-repos.md`](docs/git-project-repos.md).
- **Google Drive per-project sync is removed.** The old `projects.drive_folder_id` column, the `DriveSync` Restate
  workflow, the `aida_drive_*` MCP tools, and the web/CLI sync surfaces have been dropped — the git repo above is the
  per-Project document system of record. The `cloud::drive` OAuth door (`cli drive login` / `cli drive ls`, an
  installed-app refresh token at `~/.config/navigator/drive_token.json`) is kept for ad-hoc browsing only; Drive is no
  longer a document-ingest surface.

Local development uses cloud-agnostic equivalents (`fake-gcs-server` for GCS, Keycloak for OIDC) so the same Rust code
paths run in dev and prod.

### Production GCP at a glance

The canonical workspace surface is provider-neutral — the source tree builds and tests without any cloud account, and
every cloud-specific identifier is env-driven. NeonLaw's own production deploy is one concrete instantiation of the
example overlay at [`examples/deploy/k8s/gke/`](examples/deploy/k8s/gke/), with values supplied via Kubernetes Secrets
and the runtime `.env`. Other deployers substitute their own placeholders into the same overlay — see
[`docs/oss-install.md`](docs/oss-install.md).

Shape of the reference deployment:

- Single GCP project, single region. Project ID is `YOUR_PROJECT_ID` in the manifests; the real value lives in the
  deployer's `.env`, never in source.
- One GKE Autopilot cluster, one Cloud SQL Postgres instance, one global external HTTPS LB with host-based routing.
- `www.your-domain.example` → `navigator-web` Service. Serves `/`, `/portal`, `/api`, and `/mcp`. No Envoy in this path.
- `workflows.your-domain.example` → `workflows-service` Service (Restate worker + Envoy sidecar). Public endpoint that
  **Restate Cloud** dials to drive durable execution. Envoy is Restate's sidecar — not in the MCP / web request path.
- GCS buckets named `<project>-assets`, `<project>-logs`, `<project>-source`. Artifact Registry at
  `us-west4-docker.pkg.dev/<project>/navigator/`.

`/mcp` is gated by an in-app Google OAuth access-token validator (`web::google_oauth`) that calls Google's `tokeninfo`
endpoint — NOT by Identity-Aware Proxy. We tried IAP first; it rejects Gemini Enterprise's opaque `ya29.*` access tokens
with "Unable to parse JWT" because IAP only accepts JWT-shaped ID tokens. The `navigator-web-mcp` Service +
BackendConfig + path-routed Ingress are kept as scaffolding (`iap.enabled: false`) so we can flip IAP back on the day a
different client sends compatible tokens.

Full resource map (Workload Identity GSAs, IAM bindings, Secret Manager entries, the placeholder substitution table,
what's deliberately not deployed) lives in [`cloud/README.md`](cloud/README.md). The end-to-end "adopt this for your own
cloud" walk-through is [`docs/oss-install.md`](docs/oss-install.md).

## Workspace layout

See `README.md` for the full crate tree. This workspace contains these crates:

```text
rules        lib   — validation rules
store        lib   — SeaORM entities, migrations, canonical seed
repos        lib   — per-Project bare git repos (append-only, single `main`); backs `web::git_http`
import       lib   — bulk contact-import engine (entities + persons + roles); one lib, many surfaces
cli          bin   `navigator` — validate, import, import-contacts, seed, list; login + live-site matter driver; KIND dev-loop + deploy + `gcp setup` orchestration (in `cli::devx`)
web          bin   `web` — axum + SeaORM + maud; hosts both AIDA surfaces + git smart-HTTP + LFS
views        lib   — maud HTML view components
workflows    lib   — durable workflow primitives (Restate-shaped); `web` submits jobs to the broker
workflows-service bin `workflows-service` — Restate worker; hosts the `Notation`, `Archives`, `DriveSync`, billing-canary services + journal; only `restate-sdk` consumer
cloud        lib   — storage trait + GCS/Fs backends
compass      bin   `compass` — downstream consumer
mcp          lib   — MCP server merged into `web` at /mcp (Claude / LibreChat / Cursor)
features     lib   — Cucumber-rust BDD suite (`cargo test -p features`)
forms        lib   — vendored government forms registry (FORMS.toml ledger + bundled canonical PDFs)
lsp          bin   `navigator-lsp` — LSP server: rule diagnostics + source.fixAll
pdf          lib   — Typst-backed PDF rendering (Noto Serif firm typeface); persists via `cloud`
archives     lib   — nightly Postgres→Parquet snapshot Restate workflow + diagnostic email
statutes     lib   — weekly Nevada Revised Statutes scraper; bin `statutes_sync` reconciles into Postgres
billing      lib   — `BillingProvider` seam (Xero `ACCREC` invoices / stub) for the matter-close fee
billing-workflows lib — worker-side billing workflows (nightly Xero canary), hosted by workflows-service
```

## AIDA — the agent

**AIDA** is Navigator's domain agent persona. One tool catalog (defined in `mcp/src/tools/`), two protocol surfaces,
LLM-agnostic by design:

- **A2A** — public agent card at `/api/aida.json`, JSON-RPC at `/api/aida/rpc`. Used by Gemini Enterprise. Free-form
  `message/send` is routed by `web::agent_router::AgentRouter` (a trait — `GeminiRouter` via Vertex AI Gemini Flash in
  prod, `NullRouter` in KIND).
- **MCP** — JSON-RPC at `/mcp`. Used by Claude.ai Connectors, Claude Code, LibreChat, Cursor. Client-side LLM does the
  routing.

Skill names: MCP keeps the `aida_` prefix (multi-server tool lists need namespacing); A2A strips it (AIDA is the
namespace). The `web::a2a` bridge translates both directions and the snapshot test in `web/src/a2a.rs` fails on drift.

Swapping the router (Claude, Vertex Model Garden, local) means writing a new `impl AgentRouter` and selecting it from
`web::build_router` — never fork the tool catalog.

## Toolchain

Pinned in `rust-toolchain.toml`: **Rust 1.95.0**, `rustfmt` + `clippy` components, edition 2021. Workspace clippy:
pedantic at warn level, `unsafe_code = "forbid"`.

## Database

Postgres everywhere — Cloud SQL in prod, in-cluster Postgres in KIND, `testcontainers`-spun Postgres in `cargo test`. No
SQLite, no `APP_ENV` switch: `store::DbConfig::from_env` reads `DATABASE_URL` and errors when unset. Tests that need a
DB call `store::test_support::pg()` (one container per `cargo test` binary, one schema per test). Docker is therefore
required on every contributor laptop and CI runner.

## Secrets — Doppler

Values for the variables in `.env.example` live in **Doppler** (project `navigator`, configs `dev` for local dev +
tests, `prd` for production). Local dev injects them with `doppler run -- cargo run -p web`; `.devx/env` still supplies
the KIND-cluster wiring on top. Production never talks to Doppler — `prd` is rendered into GCP Secret Manager and the
existing CSI driver mounts it into the `navigator-web-secrets` Secret. Doppler is the operational layer **above** the
env-var interface, not a code dependency: the workspace builds, tests, and runs with no Doppler account, and `.env`
still works for OSS forks. The full workflow — local `doppler run`, the prod render, rotation, adding a new key — is in
[`docs/secrets-doppler.md`](docs/secrets-doppler.md). `.env.example` stays the committed contract; Doppler holds values.

## Running the app and other machine-bound actions

Running, previewing, or screenshotting `web` uses the `navigator` KIND loop: `cargo run -p cli -- start-dev-server`
brings every dependency up in KIND and writes `.devx/env`; `source .devx/env` then `cargo run -p web` runs the binary in
your shell against those in-cluster deps (Postgres, Keycloak, fake-gcs, Restate, OPA). The host `web` binary reaches
each dep over a port-forward, and its `DATABASE_URL` points at the port-forwarded in-cluster Postgres; `cargo test`
takes its Postgres from `testcontainers`. See the `kind-local-dev` skill and `docs/RUNBOOK.md`.

Commands that drive the cluster, a browser, or a cloud project — `docker`, `kind`, `kubectl`, `gcloud`, the e2e and
`power-push` flows — run on the user's machine. For those, propose the exact commands for the user to run; they can
prefix a command with `!` to run it in-session so its output lands in the conversation.

### Begin every session with the KIND loop

**Default to bringing the KIND loop up at the start of the session and leaving it up** — most Navigator work eventually
touches the cluster (the app, its Postgres/Keycloak/OPA/GCS/Restate deps, the `cli`, e2e), so standing it up first means
you are never blocked mid-task. Never point `web` at ad-hoc local services. The `web-preview` skill is the full recipe;
the short form:

1. `cargo run --release -p cli -- start-dev-server` brings up Postgres, Keycloak, fake-gcs, OPA, and Restate in KIND
   and writes `.devx/env`. Postgres is up and `web` runs its migrations on boot, so the schema is ready — "begin with
   KIND, all databases set up."
2. Launch `web` **under Doppler**, with `.devx/env` sourced after so the KIND wiring wins:

   ```bash
   doppler run --project navigator --config dev -- \
     bash -c 'set -a; source .devx/env; set +a; cargo run -p web'
   ```

   `web` binds `:3001`. It will **not** boot from `.devx/env` alone — `enforce_prod_invariants` needs Doppler-only
   secrets (`SENDGRID_EVENTS_*`, `DOCUSIGN_HMAC_KEY`), so skipping `doppler run` crash-loops with "production invariants
   violated."

A `web` request exercises the real dependencies, so keep them in view: Postgres (`postgres-in-kind`), Keycloak OIDC
(`keycloak-oidc`), OPA authorization (`opa-policy`), GCS object storage, Restate (`durable-execution`), and the Grafana
LGTM telemetry sink (`grafana-lgtm`). Cloud-cost and GCP questions go through `gcp-spend` and the GCP-only code
(`cloud`, `cli gcp`).

#### Local telemetry: Grafana LGTM

The KIND loop stands up **Grafana LGTM** (the `grafana/otel-lgtm` one-process image: Grafana + Loki for logs + Tempo for
traces + Prometheus for metrics + a bundled OTel Collector) as a dependency, so the same OpenTelemetry that ships to
Cloud Trace / Cloud Logging in prod has a local home. `start-dev-server` wires it both ways with no manual steps:

- **In-cluster `web` + `workflows-service`** already set `OTEL_EXPORTER_OTLP_ENDPOINT` to
  `http://lgtm.navigator.svc.cluster.local:4317`, so a cross-service trace (`web` → Restate → a workflow handler) lands
  in Tempo intact.
- **Host-side `web`** (`cargo run -p web`) gets `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` written into
  `.devx/env` (the OTLP gRPC port is port-forwarded). Setting that endpoint is exactly what flips `telemetry::init` from
  human-readable stdout logs to **JSON logs + OTLP export** of traces, metrics, and logs.
- **Browse it** at `http://localhost:3000` (Grafana, anonymous Admin, printed by `start-dev-server`). Explore → Tempo,
  search `service.name=navigator-web`; Explore → Loki for logs; Prometheus for the `navigator.workflow.trigger.fired`
  and `navigator.mcp.tool.called` metrics.

To run host `web` with plain stdout logs and no export, set `OTEL_EXPORTER_OTLP_ENDPOINT=` (empty) in `.env` — it loads
first and wins over `.devx/env`. The **one rule still binds locally**: identifiers and counts in telemetry, never client
content. Full setup, the loop, and what to look at are in the `grafana-lgtm` skill; the emit-side seam (how to add a
span/metric) is the `observability` skill.

The one caveat: `cargo test` takes its Postgres from `testcontainers` (above), so a pure test run does not strictly
require the cluster — but keep it up anyway, since almost everything else in a session does. See the `web-preview` and
`kind-local-dev` skills.

**Screenshots and scratch output never land in the working tree.** Any screenshot taken while previewing or verifying
`web` (browser captures, `fantoccini` `screenshot()`, `chromedriver` output, ad-hoc UI grabs) is written to
`/tmp/navigator-screenshots/` (create it with `mkdir -p` first), never to the repo root or any tracked directory. This
keeps the working tree clean so there is nothing to hand-delete afterward. The same goes for any other throwaway scratch
file — render it under `/tmp`. Committed visual artifacts are the exception and stay where they belong (e.g.
`docs/erd.svg` from the `erd-visualization` skill, vendored images under `web/public/`).

## Markdown linting

Every `.md` file in the workspace must pass the navigator CLI's markdown rules — the M-family Markdown rules plus S101
(120-character line limit). Dogfood the workspace's own binary; never hand-roll a different linter.

```bash
# Lint one file or directory (works on any .md path)
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes <path>

# Lint every workspace README in one pass
for d in rules store views workflows cloud web cli compass mcp; do
  cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes "$d"
done
```

Why the two flags:

- `--markdown-only` skips the F-family rules (Navigator notation frontmatter) so READMEs and prose docs don't trip on
  them.
- `--no-default-excludes` validates `README.md`, `CLAUDE.md`, and `LICENSE.md`, which are skipped by name by default.

Run this before committing any change that adds or edits `.md` files (per-crate READMEs, `docs/`, `CLAUDE.md` itself).
It must exit `0`.

## Critical: no assumptions, always test functionality

Verify behavior; do not assume it. Before reporting that anything works — a fix, a feature, a refactor, a config change
— exercise the actual code path and observe the result. "It compiles," "it looks right," and "it should work" are not
evidence; a passing test, a real request/response, or observed output is.

- **Never assume.** If you don't know how a function, route, schema, or dependency actually behaves, read it or run it —
  don't infer from the name or from memory. Recalled memories and prior context describe what *was* true; confirm it
  still is before relying on it.
- **Always test the functionality you changed.** Add or run the test that covers the new behavior (TDD: same commit as
  the implementation). For anything HTTP- or UI-facing, drive the real path — `cargo test`, the KIND loop, or a browser
  run — and confirm the observed result matches the claim. See the `verify` and `run` skills.
- **Report faithfully.** If you didn't test it, say so. If a test failed or a step was skipped, say that with the
  output. State something is done only once you've verified it.

## Commit discipline

- **Always work on a new branch, ship through a PR, let auto-merge land it — never commit directly to `main`.** This is
  the canonical flow every skill that commits or ships inherits; the steps are always the same:
  1. **Branch.** Before the first edit of any task, create and switch to a topic branch (`git switch -c <kebab-topic>`,
     e.g. `git switch -c daily-cd-pipeline`). If you find yourself on `main` with uncommitted work, branch first and
     carry the changes over to the new branch — never commit them to `main`.
  2. **Push + open a PR.** `git push -u origin <branch>` then `gh pr create`. `main` is merge-only: it advances solely
     through PRs, never a direct push.
  3. **Enable auto-merge.** `gh pr merge --auto --squash`. The PR flow (`ci.yml`) runs on the PR, and GitHub merges it
     automatically once every required check is green — you do not babysit the merge or merge by hand. (Auto-merge is a
     GitHub-native setting, not a fourth workflow; see "CI/CD — three workflows, no more" below.)

  This is non-negotiable and global: you never have to invent per-skill branch ceremony — every skill that commits or
  ships (e.g. [`power-push`](.claude/skills/power-push/SKILL.md)) assumes this exact branch → PR → auto-merge flow.
- TDD: tests in the same commit as the implementation they cover.
- Always run before committing:

  ```bash
  cargo fmt
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

  Plus the markdown lint above if you touched any `.md` file.

## CI/CD — three workflows, no more

GitHub Actions carries exactly **three** workflows, one per trigger. Do not add a fourth; fold any new automation into
the matching one.

- **PR flow** — [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Runs only on every `pull_request` targeting
  `main` — never on `push`, so `main` itself runs no CI on merge (it advances merge-only, and the heavy paths ride the
  release tag). Lean by design: it runs `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings`,
  then `cargo test --workspace` — nothing else. The job keeps target artifacts out of the cache, disables CI debug info,
  and runs `cargo clean` between clippy and test so the standard hosted runner has enough disk. One shared
  `postgres:17-alpine` container backs the whole job via `TEST_DATABASE_URL` (so `store::test_support` makes a per-test
  schema in that single container instead of spawning a testcontainer per binary). Integration/KIND/docker/browser work
  does **not** run here. **Auto-merge** is a GitHub-native repo setting, not a workflow. Enable it per PR with
  `gh pr merge --auto --squash`; GitHub squash-merges the PR the moment this `ci.yml` run goes green, which is why
  three workflows still suffice.
- **Cron flow** — [`.github/workflows/release-tag.yml`](.github/workflows/release-tag.yml). Fires daily at **02:00 PST**
  (`0 10 * * *` UTC). Its only job is to cut a calendar release tag `YY.MM.DD` (e.g. `26.06.18` for 2026-06-18) and push
  it with a PAT (`secrets.RELEASE_PAT`) so the push re-triggers the tag flow below.
- **Tag flow** — [`.github/workflows/deploy.yml`](.github/workflows/deploy.yml). Triggered by the `YY.MM.DD` tag push.
  Runs the full **KIND integration** suite, then builds both images and pushes them to **ghcr.io** tagged with that
  date, then emails a deploy report to `nick@neonlaw.com` via SendGrid (from `support@neonlaw.com`, the
  `DEFAULT_FROM_EMAIL` in `workflows/src/email/service.rs`; key in `secrets.SENDGRID_API_KEY`).

## Where to find things

- `web/content/marketing/mission.md` — why this project exists (access to justice, two-org split, what the firm's fee
  actually buys). Rendered live at `/foundation/mission` under the Foundation brand. Every product-surface decision
  should be justifiable against this file.
- `README.md` — workspace overview, install, demo.
- `AGENTS.md` — the agent-identity index: AIDA, the tool catalog, the two councils (Engineering `/council`, Legal
  `aida_spawn_legal_council`), the MCP + A2A surfaces, and the LLM-agnostic router seam. Published at `/docs/agents`.
- `docs/` — workspace documentation; the whole tree is published at `/docs/:slug` (glossary, notation, infra docs),
  served verbatim from these files via `web::docs` so a git reader and a `/docs` visitor see the same bytes.
- `docs/oidc.md` — OIDC + DB-role authz architecture (Mermaid).
- `docs/RUNBOOK.md` — verified KIND deploy + Chrome walkthrough.
- `docs/glossary.md` — vocabulary: notation, template, workflow, jurisdiction, …
- `.claude/skills/council/` — Council of Twelve, the architecture-review pattern. Invoke (`/council`) for design
  decisions, abstraction calls, and doc-clarity reviews.
- `cli/README.md` — per-subcommand reference.
- `k8s/` — KIND-ready Kubernetes manifests.
- `templates/` — markdown notation templates by category.
- `store/seeds/` — canonical reference-data YAML (bundled via `include_str!`).

## Local-only convention: `prompts/`

The `prompts/` directory at the workspace root is where draft briefs, multi-session prompts, and reusable kickoff texts
live (for example, a prompt that fires `/council` and asks for a design document on a particular feature). It is
intentionally **gitignored** — only the code we ship belongs in the repo.

- **Save prompts here**, named by topic (e.g. `prompts/retainer-questionnaire-design.md`).
- **Future agent sessions will not see these files** unless the user pastes the contents back in or names the file
  explicitly. Plan accordingly: when designing a multi-session workflow, the hand-off is the *prompt text*, not the repo
  state.
- **Do not commit prompts.** If a prompt encodes a decision worth preserving, lift the decision into the code, a doc, a
  skill, or a glossary entry — the durable surface — and leave the prompt itself in `prompts/` for the user to discard
  at will.
