# GKE production deployment

The Navigator production deployment runs on **GKE Autopilot** with every supporting service managed by Google or
Restate. The daily operator workload is reviewing dependency PRs and glancing at dashboards; no node patching, no DB
failover drills, no Helm chart babysitting.

## Architecture at a glance

```text
internet
   │
   ▼
┌───────────────────────────────────┐
│  Global External App LB (Gateway) │ ← Cloud Armor (DDoS + WAF)
│  ← Certificate Manager (TLS)      │ ← Identity-Aware Proxy (admin)
└───────────────────────────────────┘
   │
   ▼
┌───────────────────────────────────┐
│  GKE Autopilot                    │ ← Workload Identity for GCP
│  ┌─────────────────────────────┐  │
│  │ navigator-web (+OPA sidecar)│──┼──→  Cloud SQL for Postgres (PSC)
│  │ workflows-service           │──┼──→  Restate Cloud
│  └─────────────────────────────┘  │──→  GCS (object storage)
└───────────────────────────────────┘   ↑
   ▲                                    │
   │   ┌────────────────────────────────┘
   │   │ Secrets via Secret Manager CSI driver
   │   ▼
   │ Secret Manager
   │
   └── Config Sync pulls from github.com/neonlaw/Navigator
       (path: examples/deploy/k8s/gke)
```

**Decisions:** see [[project-gcp-production-stack]] in memory and the workspace `CLAUDE.md`.

## What lives where

| Concern | Managed by | Manifest |
| --- | --- | --- |
| Compute | GKE Autopilot | (cluster, no manifest) |
| Edge LB + WAF | Cloud Armor + Gateway API | `examples/deploy/k8s/gke/gateway/` |
| TLS | Certificate Manager | `examples/deploy/k8s/gke/gateway/managed-cert.yaml` |
| Postgres | Cloud SQL Enterprise Plus | (out-of-cluster; PSC endpoint) |
| Object storage | GCS | (out-of-cluster; bucket per env) |
| OIDC | Identity Platform | (out-of-cluster; issuer URL) |
| Workflows | Restate Cloud | (out-of-cluster; bearer-token auth) |
| Secrets | Secret Manager + CSI | `examples/deploy/k8s/gke/secrets/` |
| Image registry | ghcr.io (public) | `examples/deploy/k8s/gke/patches/web-image.yaml` |
| Delivery | Config Sync | RootSync (created at bootstrap) |
| Backup | Backup for GKE | `examples/deploy/k8s/gke/backup/backupplan.yaml` |
| Logs / metrics / traces | Cloud Logging + GMP + Cloud Trace | (auto, no manifest) |
| Long-term log archive | Cloud Logging sink → GCS | (gcloud-provisioned; see below) |

## Bootstrap

A fresh cluster requires gcloud commands that can't run from CI — they need a human under `gcloud auth login`. The
`navigator` CLI prints the exact sequence:

```bash
cargo run --release -p cli -- gke-bootstrap
```

Output is the full set of `gcloud services enable …`, `gcloud container clusters create-auto …`, RootSync manifest, and
post-bootstrap verification commands. Run them top-to-bottom; the RootSync at the end points at
`examples/deploy/k8s/gke` on `main`.

After the cluster is up, fill in the placeholders in `examples/deploy/k8s/gke/` (search for `YOUR_PROJECT_ID`,
`<navigator-domain>`, `<restate-tenant>`, `<cloud-sql-host>`). Commit and push — Config Sync reconciles within ~15
seconds.

## Daily deploy flow

CI/CD is exactly three workflows (see [`gitops.md`](gitops.md#cicd--three-workflows-no-more)): a lean PR flow
(`ci.yml`), a nightly cron flow that cuts a calendar release tag (`release-tag.yml`), and a tag flow that
integration-tests and publishes the images (`deploy.yml`).

```text
PR merged to main
  └─→ .github/workflows/ci.yml runs fmt + clippy + cargo test --workspace
      (no images built — the PR flow is lean by design)

Daily 02:00 PST (10:00 UTC)
  └─→ .github/workflows/release-tag.yml cuts tag YY.MM.DD (e.g. 26.06.18)
      and pushes it with secrets.RELEASE_PAT
            └─→ the tag push triggers .github/workflows/deploy.yml
                  ├─ KIND integration suite (e2e + interop + browser)
                  ├─ build + push both images to ghcr.io tagged YY.MM.DD + latest
                  └─ post a "ready to deploy" hand-off to the engineering Slack channel
                        Images are on the shelf, tagged by date.
```

The images are published, not rolled out — promoting a dated image to the GKE cluster (the Config Sync reconcile, or an
operator-driven `power-push`) is a separate, deliberate step, not part of the nightly tag flow.

The published packages are **public** on ghcr.io, so the GKE nodes pull them anonymously — there is no imagePullSecret
and no registry credential to rotate. Old dated tags are pruned after **14 days** by the maintenance workflow
(`cleanup.yml`); a fork that defers a roll past two weeks should pin and roll a tag while it is still on the shelf.

Friday-Sunday is deliberately skipped — see the comment in `deploy.yml`.

Manual rollout: `gh workflow run deploy.yml` triggers the same sequence on demand.

## What lives outside this repo

These are operator-managed resources you maintain via gcloud / console, not via this repo's manifests:

1. **Cloud SQL instance** — provisioned once via `gcloud sql instances create`. The connection URL goes into Secret
   Manager as `navigator-database-url`.
2. **GCS bucket** — `gsutil mb gs://navigator-prod`. The Workload Identity service account for `navigator-web` needs
   `roles/storage.objectAdmin` on the bucket.
3. **Identity Platform tenant** — configured via the console. OAuth client secret goes into Secret Manager as
   `navigator-oauth-client-secret`.
4. **Restate Cloud tenant** — register at <https://cloud.restate.dev>. The tenant URL and bearer token go into Secret
   Manager as `navigator-restate-broker-url` (consumed as `RESTATE_BROKER_URL`) and `navigator-restate-auth-token`
   (consumed as `RESTATE_AUTH_TOKEN`).
5. **DNS A record** for `<navigator-domain>` → the static IP reserved as `navigator-gateway-ip`.
6. **Cloud Logging → GCS sink** — a log router sink that archives `web` container logs to `gs://YOUR_PROJECT_ID-logs`
   for long-horizon audit. Provisioned via `gcloud` (see "Long-term log archive" below), not Config Sync.

Everything else flows through Config Sync.

## Long-term log archive

GKE forwards every container's stdout to Cloud Logging automatically, but the `_Default` bucket retains only 30 days —
fine for live triage, not enough for multi-year audit. A **log router sink** copies `navigator-web` logs into the
NEARLINE bucket `gs://YOUR_PROJECT_ID-logs` (the same bucket `navigator gcp setup` already creates), where the 365-day →
Coldline lifecycle keeps them cheap and durable for years.

The original design routed these via a Config Connector `LoggingLogSink` CR. That path is shelved: Config Connector does
not reliably reconcile on this cluster's GKE version, so the sink is **provisioned directly with `gcloud`** and lives
outside the repo's manifests on purpose — there is nothing under `examples/deploy/k8s/gke/` to keep it in sync with, and
putting it there would falsely imply Config Sync owns it.

Substitute `YOUR_PROJECT_ID` (the deployer's `NAVIGATOR_GCP_PROJECT_ID`) before running:

```bash
# Create the sink: route web container logs in the navigator namespace to GCS.
gcloud logging sinks create navigator-web-to-gcs \
  storage.googleapis.com/YOUR_PROJECT_ID-logs \
  --log-filter='resource.type="k8s_container"
                resource.labels.namespace_name="navigator"
                resource.labels.container_name="web"' \
  --project=YOUR_PROJECT_ID

# Grant the sink's auto-created writer identity permission to write the bucket.
WRITER=$(gcloud logging sinks describe navigator-web-to-gcs \
  --project=YOUR_PROJECT_ID --format='value(writerIdentity)')
gsutil iam ch "${WRITER}:roles/storage.objectCreator" gs://YOUR_PROJECT_ID-logs
```

Verify the sink is writing (objects appear under `logs/...` prefixes within ~1 hour of the next matching log line):

```bash
gcloud logging sinks describe navigator-web-to-gcs --project=YOUR_PROJECT_ID
gsutil ls "gs://YOUR_PROJECT_ID-logs/k8s_container/**" | head
```

This is operator-managed state, like the Cloud SQL instance and Identity Platform tenant above — it is **not** rebuilt
by `kubectl apply -k`. If you later get Config Connector reconciling, the `LoggingLogSink` CR can replace these
commands; until then this section is the source of truth for the sink's existence.

## Verifying a deploy

```bash
# Pod rollout
kubectl --namespace navigator rollout status deployment/navigator-web

# Image actually in use
kubectl --namespace navigator get deployment/navigator-web \
    -o jsonpath='{.spec.template.spec.containers[?(@.name=="web")].image}'

# Config Sync reconcile health
kubectl --namespace config-management-system get rootsync navigator \
    -o jsonpath='{.status.sync.commit}'

# Cloud Armor blocks (last 1h)
gcloud logging read \
    'resource.type="http_load_balancer"
     AND jsonPayload.statusDetails="denied_by_security_policy"' \
    --limit 50 --freshness=1h
```

## Trust boundary

**Pull-based deploy** = no external system holds cluster credentials. Specifically:

- GHCR push token is scoped to package writes only.
- GitHub Actions repo write token can commit to `main` but never reaches `kube-apiserver`.
- Config Sync inside the cluster uses an in-cluster ServiceAccount to pull the public repo — no external token at all.
- Workload Identity binds each Kubernetes ServiceAccount to a GCP service account, so pods talk to GCP without JSON
  keys.

Result: nothing outside Google's perimeter has a credential that can touch the cluster control plane or the data plane.

## Restore from backup

Backup for GKE snapshots run daily at 05:00 UTC; retention is 30 days. Restore is a single CLI:

```bash
gcloud container backup-restore backups list \
    --location=us-west4 --backup-plan=navigator-daily

gcloud container backup-restore restores create my-restore \
    --location=us-west4 \
    --restore-plan=<plan> \
    --backup=<backup-id>
```

Practice this quarterly against a scratch cluster. Untested backups are theatre.
