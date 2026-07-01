# Cloud operations

This page replaces the old private cloud runbooks with one common operating model. Public docs are the shared surface
every LLM and human maintainer should read first.

Neon Law Navigator is GCP-wired and provider-agnostic. The production path uses GKE Autopilot, Cloud SQL for Postgres,
GCS, Secret Manager, Cloud Logging, Cloud Trace, BigQuery billing export, and Restate Cloud. The application code keeps
the cloud boundary behind traits, protocols, and env vars: `cloud::StorageService`, SeaORM/Postgres, OIDC, OPA, Restate,
SendGrid, Kubernetes, and `web::agent_router::AgentRouter`.

## Former private-runbook coverage

- **KIND local dev** — source of truth: [`RUNBOOK.md`](RUNBOOK.md) and
  [`test-database.md`](test-database.md).
- **GCP REST setup** — source of truth: [`oss-install.md`](oss-install.md), this page, and
  `cli/src/devx/gcp/` module docs.
- **GKE production** — source of truth: [`gke-prod.md`](gke-prod.md) and [`gitops.md`](gitops.md). **Ship** — source of
  truth: [`gke-prod.md`](gke-prod.md) and [`deploy/gke-ship-example.md`](deploy/gke-ship-example.md).
- **GCP spend** — source of truth: this page. **Prod DB access** — source of truth: this page. **Observability/LGTM** —
  source of truth: [`observability.md`](observability.md) and [`durable-workflows.md`](durable-workflows.md).
- **OIDC/OPA/Keycloak** — source of truth: [`oidc.md`](oidc.md), [`access-model.md`](access-model.md), and
  [`RUNBOOK.md`](RUNBOOK.md).

The collapse rule is simple: durable policy, invariants, architecture, and operator recipes live in `docs/`.

## Local development

The standard local loop is KIND through the `navigator` CLI:

```bash
cargo run --release -p cli -- start-dev-server   # once; reuses an existing cluster on re-run
set -a; source .devx/env; set +a
cargo run -p web                                  # Ctrl-C and re-run to iterate
cargo run --release -p cli -- down                # full teardown — only for a clean rebuild, not routine cleanup
```

`start-dev-server` brings up Postgres, Keycloak, fake-gcs-server, OPA, Restate, `workflows-service`, and Grafana LGTM in
KIND, then writes `.devx/env` for the host-side `web` process. The cluster is a **persistent dev fixture**: leave it up
between sessions and re-run `start-dev-server` to restore port-forwards after a sleep or reboot (it reuses the existing
cluster). See [`RUNBOOK.md`](RUNBOOK.md#keep-the-deps-up-across-sessions-the-persistent-fixture).

Scratch artifacts go under `/tmp`, never the repo. Screenshots normally go under `/tmp/navigator-screenshots/`.

The KIND **dependency tier** is the exception to "local stacks are task resources": it is a reusable dev fixture, so
leave the cluster up between sessions. Everything else an agent spins up — rebuilt dev images, browser drivers, the
host-side `web` process — is a per-task resource to stop at handoff. So before handing off a created or updated PR, stop
`web` and task-created browser drivers, remove task-created standalone containers/images, and prune task-created Docker
build cache — but do **not** `down`/`kind delete` the dependency cluster as routine cleanup, and do not prune Docker
volumes unless the user approves the data loss. Full teardown is for a deliberate clean rebuild only.

## GCP setup

`navigator gcp setup` provisions GCP by calling REST APIs directly from `cli/src/devx/gcp/` with `reqwest`. There is no
`gcloud` shell-out for the setup pipeline and no broad Google SDK wrapper. That is deliberate: raw REST gives the CLI a
single dry-run intercept point and keeps endpoint behavior testable with wiremock.

When touching `cli/src/devx/gcp/`, keep four things correct:

- `GcpService::default_base_url()` in `cli/src/devx/gcp/client.rs`. Each per-step endpoint path in `services.rs`,
  `network.rs`, `sql.rs`, `buckets.rs`, and `run.rs`. The JSON request body shape. The long-running-operation polling
  path passed to `lro::wait`.

Every step follows the same conventions:

- POST the create/enable operation and treat `409 Conflict` as success. Wait for LROs on 2xx responses that return an
  operation name; skip the wait on 409. Let `GcpClient` handle dry-run recording. Do not add a `gcloud` fallback or move
  base URLs into env vars.

When an endpoint drifts, update the module's wiremock test to match Google's current docs first, then update the
implementation and run the dry-run command from [`oss-install.md`](oss-install.md).

## Production deploy

Code reaches production through PRs and dated images:

1. Merge through the normal PR flow in [`gitops.md`](gitops.md).
2. The release-tag workflow cuts a `YY.M.D` tag.
3. The deploy workflow builds and publishes both images to ghcr.io:
   `navigator-web` and `navigator-workflows-service`.
4. An operator rolls GKE onto the published tag.

Always roll `navigator-web` and `workflows-service` together at the same `YY.M.D` tag. Version skew between the web
surface and durable worker is an avoidable production risk.

Before a rollout, check the new binary's required env/secret keys against the live production Secret. `web` enforces
boot invariants and crash-loops loudly when a required key is missing. If the image tag is unchanged and only a Secret
changed, restart the deployments so pods re-read `envFrom`.

Run production cluster commands under the production secret context. Never paste real secret values into chat, docs,
commits, or PR bodies.

## Production database

Production is Cloud SQL for Postgres. Ad-hoc access goes through `cloud-sql-proxy` with IAM service-account
impersonation, not password shortcuts.

Read-only `SELECT`s are allowed when the user asks for inspection. Before any `INSERT`, `UPDATE`, `DELETE`, or DDL:

- Write the exact SQL to a timestamped file under `/tmp/navigator-prod-sql/`. Show the user the path and contents. Wait
  for explicit approval for that exact statement. Scope the write with a guard on the old value. Wrap the write in a
  transaction and verify it. Revoke the temporary IAM impersonation grant when done.

The canonical seed is idempotent: it inserts missing rows and does not update existing production rows. A live data fix
needs a guarded update, a migration, or an app seam.

## Spend reporting

Report GCP spend from the BigQuery Cloud Billing export, not console guesses or rate-card math. Always show:

- gross cost, credits, which are negative, net cost, which is `gross + credits`, currency, and whether the current day
  is partial because billing export data lags by roughly 24 hours.

Discover the project from env and the billing table from BigQuery. Do not hard-code billing account generated table
names into docs or code.

## Observability

Every service binary emits through `telemetry::init("navigator-<name>")`. With no `OTEL_EXPORTER_OTLP_ENDPOINT`, logs
stay human-readable on stdout. With the endpoint set, logs become JSON and traces/metrics export through OTLP.

The load-bearing rule is:

> Identifiers and counts, never content.

Safe telemetry fields include ids, service names, outcomes, durations, status codes, and counts. Unsafe fields include
client names, email addresses, answer bodies, document bodies, privileged facts, and full request or tool arguments.
This rule applies in local Grafana LGTM, Cloud Logging, Cloud Trace, BigQuery, and any future sink.

Use `navigator doctor`, Cloud Logging/BigQuery, the Restate console, and the six-hourly Heartbeat email to debug missing
periodic jobs or durable workflow failures. The architecture details live in [`observability.md`](observability.md) and
[`durable-workflows.md`](durable-workflows.md).

## Website publication

Top-level files in `docs/` are already published at `/docs/:slug` by `web::docs`. The site bakes the docs into the
binary with `include_str!`, renders markdown under the Foundation brand, and rewrites top-level doc links to site
routes. That gives every maintainer and LLM the same documentation surface.

Good next steps for the website:

- Add a `/docs` hub that lists every `DocsIndex::docs()` entry instead of requiring users to know a slug. Add a short
  "For agents" section on `/navigator` linking to [`agent-decision-councils.md`](agent-decision-councils.md), this page,
  [`access-model.md`](access-model.md), [`glossary.md`](glossary.md), and [`RUNBOOK.md`](RUNBOOK.md).
- Keep top-level docs concise and push long command transcripts into examples such as
  [`deploy/gke-ship-example.md`](deploy/gke-ship-example.md).
- Keep public docs as the source of truth. If an invariant matters, lift it into `docs/`.
