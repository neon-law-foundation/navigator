# cloud

Provider-quarantined GCP object storage. An async `StorageService` trait with `FsStorage` (dev, the default) and
`GcsStorage` (Google Cloud Storage) backends — every other crate depends on the trait, not on `google-cloud-storage`.

Also ships a tiny `redirect` binary deployed to Cloud Run that serves HTTPS redirects for navigator-owned hostnames that
don't host an app (`chat.your-domain.example` → Gemini Enterprise landing; naked `neonlaw.com` →
`https://www.your-domain.example{path}`). See [Redirect service (Cloud Run)](#redirect-service-cloud-run) below.

GCP **provisioning** (VPC, Cloud SQL, GCS buckets, GKE Autopilot cluster) lives in
[`cli::devx::gcp`](../cli/src/devx/gcp/) and is reached via `navigator gcp setup` — see
[`cli/README.md`](../cli/README.md).

## The per-Project archive is a git repository

**Every Project is its own git repository — there is no Google Drive.** Each matter's documents live in one append-only
git repo with a single `main` branch, served Rust-native from `web` over smart-HTTP; the commit log *is* the matter's
audit trail. The lawyer's "open the matter" gesture is to clone the repo's git URL,
`https://www.your-domain.example/projects/<project-id>.git`, authenticating with a short-lived, Project-scoped Personal
Access Token `web` mints. The full durable design — transport, auth, append-only enforcement, governed expunge — is
[`docs/git-project-repos.md`](../docs/git-project-repos.md).

GCS stays in the picture in exactly one place: as the **Git LFS object store** behind the [`StorageService`] trait.
Large binary artifacts (rendered PDFs, signed copies, images) are routed to LFS by each repo's `.gitattributes` and land
in the **private** documents bucket `gs://YOUR_PROJECT_ID-documents/` (the `FsStorage` backend in dev) — never in the
public `-assets` bucket. The LFS pointer is committed in the pack; the object reconciles by its `oid` (sha256). The git
repos themselves live on a POSIX volume, not a bucket (GCS is not a filesystem); see the durable design doc.

The legacy Google Drive ingest path (the `cli drive` OAuth door, the `cloud::drive` REST client, the `DriveSync`
workflow, and the `projects.drive_folder_id` column) has been **removed** — git is the per-Project document system of
record. No `google-cloud-drive` crate, and no `drive.readonly` OAuth app, is in the dependency graph.

### Ingestion audit trail

A `documents` row is the canonical audit-trail record for one inbound artifact, and the matter repo's commit history is
the durable record of every version. The channel name lives on `documents.source` (`upload`, `email`, …); rows tagged
`drive_sync` predate the git pivot and are retained as historical provenance only.

## Getting started

```bash
# Library tests — fs round-trip + GcsStorageConfig env parsing.
# Wiremock-backed; no GCP credentials needed.
cargo test -p cloud
```

`GcsStorage` uses Application Default Credentials by default — so `gcloud auth application-default login` on a laptop
and Workload Identity Federation on a GKE workload. For local development against a GCS emulator (`fake-gcs-server`),
set `NAVIGATOR_STORAGE_ENDPOINT` to the emulator URL and the crate skips auth entirely.

## Backend selection

`cloud::from_env()` picks a backend based on `NAVIGATOR_STORAGE_BACKEND`:

| Value | Backend | Notes |
| --- | --- | --- |
| unset / `fs` | `FsStorage` | Writes to `$NAVIGATOR_STORAGE_FS_ROOT` (default `./var/storage`). |
| `gcs` / `google` | `GcsStorage` | See bucket-name precedence below; optional `NAVIGATOR_STORAGE_ENDPOINT`. |

### Bucket naming convention (`<project>-<suffix>`)

Every bucket name is the GCP project id plus a fixed suffix, so a fork that owns project `acme-prod` gets
`acme-prod-assets`, `acme-prod-documents`, and so on. Nothing else is hard-coded — the suffix is the contract, the
project id flows through `.env`. The five suffixes and who owns each:

| Bucket | Suffix | Contents | Resolved by |
| --- | --- | --- | --- |
| Assets | `-assets` | **Public** marketing photos (responsive variants) | `NAVIGATOR_ASSETS_BUCKET` (CLI) |
| Documents | `-documents` | Private client PDFs (`notations/<id>/…`) + `blobs/<sha>` | `NAVIGATOR_DOCUMENTS_BUCKET` |
| Exports | `-exports` | Archives snapshots (Parquet / Iceberg) | `NAVIGATOR_STORAGE_BUCKET` |
| Logs | `-logs` | Log sink (Nearline) | — (sink config) |
| Source | `-source` | legacy git-bundle archive (ship no longer writes it) | — (`gcloud storage cp`) |

Every bucket except `-assets` is private. The `cloud` crate only ever opens the **documents** and **exports** buckets at
runtime; assets is write-only via `cli assets upload`, and logs/source are managed outside the `StorageService` seam.

### Two resolution lanes, one `StorageService` trait

`GcsStorageConfig` exposes two bucket resolvers, so a single pod can open two different buckets without env-var
collisions:

- **`cloud::from_env()`** (the *documents-preferred* lane) resolves `NAVIGATOR_DOCUMENTS_BUCKET` first, then falls back
  to `NAVIGATOR_STORAGE_BUCKET`. This is the lane `web` and the worker's `document_open__*` PDF-render step use — both
  must land client documents in the **documents** bucket.
- **`cloud::exports_from_env()`** (the *exports* lane) resolves `NAVIGATOR_STORAGE_BUCKET` **only**, never
  `NAVIGATOR_DOCUMENTS_BUCKET`. This is the lane the `archives` snapshot workflow uses, so nightly Parquet always lands
  in the **exports** bucket.

The split matters most on the **`workflows-service` worker**, which runs both lanes on one pod and therefore carries
**both** env vars: `NAVIGATOR_DOCUMENTS_BUCKET=<project>-documents` (render lane) and
`NAVIGATOR_STORAGE_BUCKET=<project>-exports` (archives lane). If the worker had only `from_env()`, setting
`NAVIGATOR_DOCUMENTS_BUCKET` would silently divert the Archives snapshots into the documents bucket — hence the
dedicated `exports_from_env()`. And if the worker's render lane resolved to the *exports* bucket (or, with
`NAVIGATOR_STORAGE_BACKEND` unset, to local `FsStorage`), `web` would read the rendered retainer PDF from the documents
bucket and 500 with `object not found` — which is exactly the break the split was added to close.

> **`NAVIGATOR_STORAGE_BACKEND` is required on the worker.** It is unset by default → `fs`, which writes to the pod's
  ephemeral disk. The worker must set `NAVIGATOR_STORAGE_BACKEND=gcs` alongside both bucket vars, or every rendered PDF
  and every snapshot is written into the void. See the `workflows-service` Deployment in the deploy overlay.

The trait is `Send + Sync`, returns `StorageError` (concrete enum so callers can match on `NotFound`), and stores key
plus bytes plus content-type — enough to round-trip an inbound email or a generated PDF without coupling callers to
GCS-specific types. `put_cached(key, bytes, content_type, cache_control)` is `put` plus an HTTP `Cache-Control`
directive; its default impl ignores the header and delegates to `put` (so `FsStorage` is unaffected), and only
`GcsStorage` overrides it — via a multipart upload — to stamp the header on the stored object.

### Public assets bucket (responsive photography)

The marketing photography is **not** stored in the documents bucket (`NAVIGATOR_DOCUMENTS_BUCKET`) and **not** baked
into the `navigator-web` image. It is served from the public **`NAVIGATOR_ASSETS_BUCKET`** (the `YOUR_PROJECT_ID-assets`
bucket), uploaded by `cli assets upload` through `put_cached`:

- Keys mirror the on-disk variant tree: `img/<slug>/<slug>-<width>w.<ext>`. Each object carries
  `Cache-Control: public, max-age=604800` (~1 week). It is **bounded, never `immutable`** — the variant URLs carry no
  `?v=` cache-bust token, so `immutable` would pin a stale photo forever; a bounded max-age lets a re-`build` +
  re-`upload` propagate once the week elapses.
- `web` points `NAVIGATOR_ASSET_BASE_URL` at `https://storage.googleapis.com/YOUR_PROJECT_ID-assets` so every
  `<picture>`/preload URL (resolved through `views::assets::asset_url`) sources from the bucket — zero app-code change.
  Unset, the seam defaults to `/public` so KIND / `cargo test` / OSS forks render unchanged. When the var names an
  absolute origin, `web` widens its CSP `img-src` to include that origin automatically, so the browser serves the
  cross-origin photos instead of blocking them to alt text. Vendored JS/CSS stays same-origin on `/public`.
- The bucket is the only one with an `allUsers` viewer binding — see the IAM bindings note under the resource map below.

## Production GCP resource map

Single-project, single-region: **`YOUR_PROJECT_ID`**, **`us-west4`** (Las Vegas). Everything below lives in this one
project.

### Provisioned by `navigator gcp setup`

REST-driven, idempotent (`409 Conflict` = success). See [`cli/src/devx/gcp/mod.rs`](../cli/src/devx/gcp/mod.rs) for the
pipeline order.

- **API enablement** — ~30 services (`serviceusage.batchEnable`), in [`services`](../cli/src/devx/gcp/services.rs).
  **VPC** — `navigator` (custom-mode), in [`network`](../cli/src/devx/gcp/network.rs). **Cloud SQL Postgres** — instance
  `navigator-pg`, database `navigator`, user `web`, in [`sql`](../cli/src/devx/gcp/sql.rs).
- **GCS buckets** (all `us-west4`, uniform access), in [`buckets`](../cli/src/devx/gcp/buckets.rs): -
  `YOUR_PROJECT_ID-assets` — Standard, **public** marketing photography only (the sole bucket with an `allUsers`
  binding). - `YOUR_PROJECT_ID-documents` — Standard, **private** client documents (the content-addressed `blobs/<sha>`
  objects `web` writes); web GSA gets `roles/storage.objectUser`, **no** public binding. - `YOUR_PROJECT_ID-logs` —
  Nearline. - `YOUR_PROJECT_ID-source` — git bundles for production rollout, created manually via `gsutil mb`. -
  `YOUR_PROJECT_ID-exports` — Standard, archives data snapshots (Parquet today, Iceberg metadata in the Commit 3
  follow-up). Layout is a contract — see "Archives bucket layout" below. `navigator gcp setup` provisions the assets,
  documents, and logs buckets; `-source` and `-exports` are created manually via `gsutil mb`. All five buckets carry a
  365-day → Coldline lifecycle policy (applied via `gsutil lifecycle set` on 2026-05-24; `-documents` gets the same
  lifecycle when provisioned).
- **GKE Autopilot cluster** — `navigator-prod` (regional, `us-west4`), in [`gke`](../cli/src/devx/gcp/gke.rs). **Global
  static IPv4** — `navigator-ingress-ip` (shell-out in `gke.rs`). **Fleet membership + Config Sync `RootSync`** —
  created via shell-out in `gke.rs`. RootSync is **not currently reconciling** because the repo isn't pushed to a git
  remote yet.

There is **no Artifact Registry**. Container images are built by CI (`deploy.yml`) and published to the **public**
`ghcr.io/neon-law-foundation/navigator-*` packages, tagged `YY.M.D` (the release date) + `latest`; the GKE nodes pull
them anonymously, so there is no in-cluster registry credential and nothing to rotate. `navigator gcp setup` never
provisioned an Artifact Registry repo.

### Archives bucket layout

The `archives` CronJob writes to `gs://YOUR_PROJECT_ID-exports/` under this fixed layout:

```text
iceberg/
  <postgres_table_name>/
    _schema.json   (column-set fingerprint, drift detector)
    metadata/      (Iceberg metadata.json + manifests, Commit 3)
    data/
      <yyyy-mm-dd>/
        part-<uuid-v7>.parquet  (one Parquet file per run)
catalog/           (filesystem catalog state, Commit 3)
```

This layout is the contract between the writer (`archives`) and the readers (BigLake external tables, future
Spark/DuckDB jobs). Don't change it without coordinating both sides.

### Deploying `archives` to GKE

The `Archives` workflow is hosted by the `workflows-service` worker (all workflows live there — no separate archives
pod). So shipping archives is: rebuild `workflows-service` (the `archives` lib compiles in) and ship the thin trigger
image for the nightly CronJob.

```bash
# 1. Roll workflows-service onto the latest published image (it now hosts the
#    Archives workflow). Use the ship runbook; ensure the storage env
#    (NAVIGATOR_STORAGE_BUCKET=YOUR_PROJECT_ID-exports) is on the Deployment so
#    the snapshot phase can write Parquet. Re-register with Restate after the roll.

# 2. Point the nightly trigger CronJob at the published ghcr image. CI (deploy.yml)
#    builds and publishes navigator-archives-trigger to ghcr.io tagged YY.M.D;
#    the GKE nodes pull it anonymously (public package). Pin the manifest to the tag:
TAG=$(git ls-remote --tags --refs origin | grep -oE '[0-9]{2}\.[0-9]{2}\.[0-9]{2}$' | sort | tail -1)
sed -i "s|:YY.M.D|:$TAG|" examples/deploy/k8s/exports/cron-archives-trigger.yaml
kubectl --context=gke_YOUR_PROJECT_ID_us-west4_navigator-prod apply -k examples/deploy/k8s/exports/

# 3. Trigger a run to seed the bucket so external-table schema inference works:
kubectl --context=gke_YOUR_PROJECT_ID_us-west4_navigator-prod \
  create job --from=cronjob/archives-trigger -n navigator bootstrap-001
```

To enable the nightly GCP cost-by-service summary (a `gcp_cost` Parquet snapshot + a COST section in the diagnostic
email), set `BILLING_EXPORT_TABLE` + `BIGQUERY_PROJECT` on the `workflows-service` Deployment and grant its GSA
`roles/bigquery.jobUser` + `roles/bigquery.dataViewer` on the billing dataset. Unset → the cost phase is a clean no-op.

### Redirect service (Cloud Run)

Tiny axum binary (`cargo run -p cloud --bin redirect`) that serves 308 redirects keyed on the inbound `Host` header:

| Host | Target |
| --- | --- |
| `neonlaw.com` | `https://www.your-domain.example{path_and_query}` (path-preserving naked→www canonicalization) |
| `chat.your-domain.example` | Fixed Gemini Enterprise landing URL (regardless of path) |

Dispatch is in [`src/redirect.rs`](src/redirect.rs); [`src/bin/redirect.rs`](src/bin/redirect.rs) is the Cloud Run
entrypoint (honors `$PORT`). Tested both as a pure function over `(host, uri)` and through the router with
`tower::ServiceExt::oneshot` — `cargo test -p cloud` runs both.

**Why Cloud Run and not DNSimple URL records?** Both `chat` and naked-apex were previously `URL` records in DNSimple's
hosted redirector (the source of `3.131.150.69` in `dig` output). That redirector serves HTTP only on our
`solo-v2-monthly` plan; HTTPS URL records require the Teams plan (+$18/mo) AND a manually-issued SSL certificate per
host. Cloud Run domain mappings auto-provision and renew certs and stay inside the free tier for the request volume
these hosts see (dozens/day).

**Region note: `us-west1`, not `us-west4`.** Cloud Run domain mappings are not supported in `us-west4` (the rest of our
stack runs there); they are supported in `us-west1` (Oregon — geographically closest supported region). The image is
pulled from `ghcr.io` (region-agnostic), so the Cloud Run region is independent of where the rest of the stack runs.

**Org-policy prerequisite.** The org `neonlaw.com` enforces `constraints/iam.allowedPolicyMemberDomains`, which blocks
`allUsers` IAM bindings (Cloud Run requires that for unauthenticated public access). The project-level override is set
to `allValues: ALLOW` for the `YOUR_PROJECT_ID` project specifically; the org-wide default remains restrictive. To
re-apply if drift:

```bash
cat > /tmp/redirect-allusers.yaml <<EOF
constraint: constraints/iam.allowedPolicyMemberDomains
listPolicy:
  allValues: ALLOW
EOF
gcloud resource-manager org-policies set-policy /tmp/redirect-allusers.yaml --project=YOUR_PROJECT_ID
```

Setting that policy requires `roles/orgpolicy.policyAdmin` at the org level (not bundled into
`roles/resourcemanager.organizationAdmin`).

#### Deploy

CI (`deploy.yml`) builds the redirect image and publishes it to the **public**
`ghcr.io/neon-law-foundation/navigator-redirect` package, tagged `YY.M.D` + `latest`; Cloud Run pulls it anonymously.
Deploying is just pointing the service at the published tag:

```bash
TAG=$(git ls-remote --tags --refs origin | grep -oE '[0-9]{2}\.[0-9]{2}\.[0-9]{2}$' | sort | tail -1)
IMAGE="ghcr.io/neon-law-foundation/navigator-redirect:$TAG"

gcloud run deploy redirect \
  --project=YOUR_PROJECT_ID --region=us-west1 \
  --image="$IMAGE" \
  --allow-unauthenticated \
  --min-instances=0 --max-instances=5 \
  --memory=128Mi --cpu=1
```

`min-instances=0` keeps the service in the always-free tier; the first request after idle takes ~1s to cold-start
(acceptable for a redirect).

#### Domain mappings (Google-managed certs, free)

```bash
gcloud beta run domain-mappings create \
  --service=redirect --region=us-west1 \
  --domain=chat.your-domain.example
gcloud beta run domain-mappings create \
  --service=redirect --region=us-west1 \
  --domain=neonlaw.com
```

Each command prints the DNS record(s) to add. `chat.your-domain.example` gets a single `CNAME` to
`ghs.googlehosted.com.`; naked `neonlaw.com` gets four `A` records (`216.239.32.21`, `.34.21`, `.36.21`, `.38.21`) plus
four `AAAA` records (`2001:4860:4802:32::15`, `:34::15`, `:36::15`, `:38::15`) — apex can't `CNAME`. Both hostnames must
be domain-verified at [Google Search Console](https://search.google.com/search-console) under the same account as the
GCP project. Apex verification covers subdomains when you use **Domain** mode (not URL-prefix mode) and the
`google-site-verification=…` TXT lands on the apex.

**Watch for Google Sites custom-domain conflicts.** Both Cloud Run domain mappings and Google Sites custom domains go
through the same `Server: ghs` frontend; if a Sites mapping exists for the same hostname, it wins (the response will be
a 301 to a `sites.google.com/<workspace>/<site>/` URL). Disconnect via the Sites editor → gear → Custom domains, or via
the Workspace Admin Console → Apps → Sites → Web addresses.

#### DNS swap with the DNSimple CLI

DNSimple ships a Go CLI — install with `brew install dnsimple/tap/dnsimple` on macOS or the one-liner at
<https://dnsimple-cli.netlify.app/install.sh> on Linux. We use it for the one-off swap rather than the [official Rust
SDK](https://crates.io/crates/dnsimple) (`dnsimple` v6.x, actively maintained) — for two records, scripted Rust is
overkill. The Rust SDK is the right pick if we ever need programmatic zone-record reconciliation from inside the `cli`
crate.

Pre-swap state (`dnsimple zones records list neonlaw.com`) has three relevant `URL` records — capture their IDs first:

```bash
dnsimple zones records list neonlaw.com --json \
  | jq -r '.data[] | select(.type=="URL") | "\(.id)\t\(.name)\t\(.content)"'
```

Then delete the apex + chat `URL` records and create the records Cloud Run handed us in the previous step:

```bash
# Replace <APEX_URL_ID> and <CHAT_URL_ID> with the IDs from above.
dnsimple zones records delete neonlaw.com <APEX_URL_ID>
dnsimple zones records delete neonlaw.com <CHAT_URL_ID>

# chat.your-domain.example → Cloud Run (CNAME).
dnsimple zones records create neonlaw.com \
  --type=CNAME --name=chat \
  --content=ghs.googlehosted.com. --ttl=300

# neonlaw.com apex → Cloud Run (four A records; substitute the
# IPs `gcloud beta run domain-mappings create` printed).
for ip in <IP1> <IP2> <IP3> <IP4>; do
  dnsimple zones records create neonlaw.com \
    --type=A --name="" --content="$ip" --ttl=300
done
```

`www.your-domain.example` itself is served by Neon Law Navigator (see the "navigator-web hostnames" section below) — the
naked-apex 308 lands the user on it, and the GKE LB owns HTTPS there.

#### Verify

```bash
# Expect: 308 → https://www.your-domain.example/
curl -sI https://neonlaw.com | grep -iE '^(HTTP|location)'

# Expect: 308 → https://vertexaisearch.cloud.google.com/…
curl -sI https://chat.your-domain.example | grep -iE '^(HTTP|location)'
```

First-issue Google-managed cert provisioning takes 15–60 minutes after DNS propagates; until then `curl` will fail TLS.
Plain `http://` works immediately and 308s through the same target. The Cloud Run `domain-mappings describe` status may
sit on `CertificatePending` for longer than the cert actually takes to start serving — trust the curl, not the status.

### navigator-web hostnames (`www.your-domain.example` + `workflows.your-domain.example`)

The GKE LB serves Neon Law Navigator on two hostnames; both `A` records point at the global static IPv4
`navigator-ingress-ip`. DNS is at **DNSimple** — same zone (`neonlaw.com`) and same `dnsimple` CLI as the redirect swap
above. The CLI install is one-liner at <https://dnsimple-cli.netlify.app/install.sh> on Linux. On macOS, use `brew` to
install `dnsimple/tap/dnsimple`; it reads `DNSIMPLE_TOKEN` (or `~/.config/dnsimple/credentials.yml`).

**Retirement of `navigator.neonlaw.com` and `workflows.navigator.neonlaw.com`.** The old hostnames are no longer
referenced anywhere in the workspace. Remove their records once the new ones resolve and the Google-managed certs flip
Active.

Get the LB IP:

```bash
LB_IP=$(gcloud compute addresses describe navigator-ingress-ip \
  --global --project=YOUR_PROJECT_ID --format='value(address)')
echo "$LB_IP"
```

Capture the existing record IDs so you have something to delete (the previous `www` was a DNSimple `URL` record pointing
at `3.131.150.69`):

```bash
dnsimple zones records list neonlaw.com --json \
  | jq -r '.data[] | select(.name=="www" or .name=="navigator" or .name=="workflows.navigator") |
           "\(.id)\t\(.type)\t\(.name)\t\(.content)"'
```

Then swap the records — remove the old, create the new:

```bash
# Replace the IDs printed above.
dnsimple zones records delete neonlaw.com <WWW_RECORD_ID>
dnsimple zones records delete neonlaw.com <NAVIGATOR_RECORD_ID>
dnsimple zones records delete neonlaw.com <WORKFLOWS_NAVIGATOR_RECORD_ID>

# www.your-domain.example → GKE LB (A record).
dnsimple zones records create neonlaw.com \
  --type=A --name=www --content="$LB_IP" --ttl=300

# workflows.your-domain.example → same GKE LB (A record).
dnsimple zones records create neonlaw.com \
  --type=A --name=workflows --content="$LB_IP" --ttl=300
```

Google-managed cert provisioning waits on DNS resolving to the LB's IP; expect 15–60 minutes per hostname before the
ManagedCertificate status flips from `Provisioning` to `Active`:

```bash
kubectl --context=gke_YOUR_PROJECT_ID_us-west4_navigator-prod \
  get managedcertificate -n navigator
```

After both certs are Active, re-register the worker URL with Restate Cloud so the broker stops dialing the old hostname:

```bash
cargo run --release -p cli -- restate register
```

The OAuth 2.0 client (`YOUR_OAUTH_CLIENT_ID_BROWSER.apps.googleusercontent.com`) lists
`https://www.your-domain.example/auth/callback` as an authorized redirect URI; if the old
`https://navigator.neonlaw.com/auth/callback` is still listed there, remove it via the [GCP Console at
console.cloud.google.com/apis/credentials](https://console.cloud.google.com/apis/credentials?project=YOUR_PROJECT_ID).

### Cost monitoring (BigQuery billing export)

Cloud Billing exports daily usage + cost rows from billing account `013469-2BBE03-532C72` into the BigQuery dataset
`YOUR_PROJECT_ID.billing_export` (us-west4). Two tables once populated (~24h after enablement):

- `gcp_billing_export_v1_013469_2BBE03_532C72` — standard daily cost by SKU/service/project.
  `gcp_billing_export_resource_v1_013469_2BBE03_532C72` — per-resource detail (which bucket, which instance, which Cloud
  Run service).

Export enablement is **Console-only** — Google has not exposed `BillingAccounts.updateBillingExportSettings` via
`gcloud` or public REST. To re-enable or change destination, edit at
<https://console.cloud.google.com/billing/013469-2BBE03-532C72/export>. The dataset was created with:

```bash
bq mk --dataset --location=us-west4 YOUR_PROJECT_ID:billing_export
```

Net 30-day cost by service:

```sql
SELECT
  service.description AS service,
  ROUND(SUM(cost), 2) AS gross_usd,
  ROUND(SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS credits_usd,
  ROUND(SUM(cost) - SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS net_usd
FROM `YOUR_PROJECT_ID.billing_export.gcp_billing_export_v1_013469_2BBE03_532C72`
WHERE _PARTITIONTIME BETWEEN TIMESTAMP_TRUNC(TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY), DAY)
                          AND CURRENT_TIMESTAMP()
GROUP BY service
ORDER BY net_usd DESC;
```

Daily cost across last 30 days (a quick "anything weird?" trend check):

```sql
SELECT
  DATE(usage_start_time) AS day,
  ROUND(SUM(cost) - SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)), 2) AS net_usd
FROM `YOUR_PROJECT_ID.billing_export.gcp_billing_export_v1_013469_2BBE03_532C72`
WHERE _PARTITIONTIME BETWEEN TIMESTAMP_TRUNC(TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 30 DAY), DAY)
                          AND CURRENT_TIMESTAMP()
GROUP BY day
ORDER BY day;
```

If you want to ask "who is the noisiest single resource?", switch to the `_resource_v1_` table and group by
`resource.name` instead of `service.description`. Always filter by `_PARTITIONTIME` to keep the scan cheap — both tables
are partitioned by ingestion day.

### BigQuery bootstrap (one-time, per table)

The `Archives` workflow writes the Parquet files nightly. There is no metadata-refresh step: BigLake external tables
over GCS Parquet re-scan their `uris` glob at query time, so newly written partitions show up on the next query without
a refresh. The external tables themselves are created once, by hand, via:

```bash
bq mk --location=us-west4 --dataset YOUR_PROJECT_ID:navigator_bi
bq mk --connection --location=us-west4 \
  --project_id=YOUR_PROJECT_ID \
  --connection_type=CLOUD_RESOURCE exports
# Grant the connection's auto-generated GSA roles/storage.objectViewer
# on gs://YOUR_PROJECT_ID-exports/.
```

Then one `CREATE EXTERNAL TABLE` per registered entity, following this shape (substitute the SQL table name for
`persons`):

```sql
CREATE EXTERNAL TABLE `YOUR_PROJECT_ID.navigator_bi.persons`
WITH CONNECTION `us-west4.exports`
OPTIONS (
  format = 'PARQUET',
  uris = ['gs://YOUR_PROJECT_ID-exports/iceberg/persons/data/*'],
  hive_partition_uri_prefix = 'gs://YOUR_PROJECT_ID-exports/iceberg/persons/data',
  require_hive_partition_filter = false
);
```

The 26-table set is the `archives::ALL_TABLES` registry (the same list the snapshot phase walks).

Because the export issues no BigQuery DDL/DML at runtime, the worker's GSA needs no BigQuery role — only
`roles/storage.objectUser` on `gs://YOUR_PROJECT_ID-exports/` to write the Parquet. The one-time `bq mk` /
external-table creation above is run by an operator, not the workflow.

The Iceberg-managed table flavor (where `format = 'ICEBERG'` and the writer authors `metadata/v<n>.metadata.json` on
each snapshot) is the deferred follow-up. The bucket layout's `iceberg/<table>/metadata/` prefix is reserved for it.

### Out-of-band (`gcloud` / `kubectl` commands, not in the overlay)

Config Connector CRs would normally express these; the controller isn't reliably standing up on this cluster's GKE
version, so we accept the operational tax. See [`examples/deploy/k8s/gke/iam.yaml`](../examples/deploy/k8s/gke/iam.yaml)
for the history and the rationale.

- **Workload Identity GSAs**: - `navigator-web@YOUR_PROJECT_ID.iam.gserviceaccount.com` — bound to the `navigator-web`
  KSA. - `workflows-service@YOUR_PROJECT_ID.iam.gserviceaccount.com` — bound to the `workflows-service` KSA.
- **IAM bindings**: - `roles/cloudsql.client` on both GSAs (Cloud SQL Auth Proxy). - `roles/storage.objectUser` on the
  assets bucket **and** the documents bucket → web GSA (web uploads photography variants to `-assets` and reads/writes
  client `blobs/<sha>` on `-documents`). - `roles/secretmanager.secretAccessor` on each secret → the GSA that consumes
  it. - `roles/storage.objectViewer` for **`allUsers`** on the **assets bucket only** — this is what makes the
  responsive photography publicly readable at `https://storage.googleapis.com/YOUR_PROJECT_ID-assets/img/...`. Scope it
  to the assets bucket and **never** to `-documents`, `-logs`, or `-source`; those carry confidential client documents,
  operational data, and git bundles. The binding relies on the project-level `iam.allowedPolicyMemberDomains` org-policy
  override already set on this project. Apply with `gsutil iam ch allUsers:objectViewer gs://YOUR_PROJECT_ID-assets`,
  then verify that the binding scope is the assets bucket only, using `gsutil iam get gs://YOUR_PROJECT_ID-assets`;
  confirm `gsutil iam get gs://YOUR_PROJECT_ID-documents` shows **no** `allUsers` member.

  > **Cloud SQL `instances.export` IAM change (Google notice, effective 2026-08-01) — Neon Law Navigator is not
    affected.** Google is removing the `cloudsql.instances.export` permission from `roles/cloudsql.viewer` and the
    legacy `READER` role; after the rollout, the *managed* "export instance to GCS" feature (Console, `gcloud`, or the
    Cloud SQL Admin API) needs `roles/cloudsql.editor` or a custom role that carries `cloudsql.instances.export`. **No
    Neon Law Navigator path uses that managed export.** Despite the name, the nightly `archives` snapshot is an
    *application-level* export: `archives::runner` runs a SeaORM `SELECT` (`fetch_batch`) over the **Cloud SQL Auth
    Proxy** (`roles/cloudsql.client`), encodes Parquet, and writes it to `gs://YOUR_PROJECT_ID-exports/` via
    `cloud::StorageService` — it never calls `cloudsql.instances.export`. Neither GSA holds `roles/cloudsql.viewer` or
    legacy `READER` (they hold `roles/cloudsql.client` only), and no human grant relies on viewer-plus-export. So **no
    IAM change is required, and we deliberately do *not* grant `roles/cloudsql.editor` to any service account** — that
    role re-adds whole-instance exfiltration on a confidential client database. The only legitimate future use of the
    managed export is an ad-hoc DR dump a *human* runs; after 2026-08-01 that human needs `roles/cloudsql.editor` (grant
    it ephemerally; see [`docs/cloud-operations.md`](../docs/cloud-operations.md)), never a standing binding.
- **OAuth 2.0 client (Google Sign-in)**: `YOUR_PROJECT_NUMBER-…apps.googleusercontent.com`, used by the browser SSO flow
  into `/portal`.
- **K8s Secret `navigator-web-secrets`** — holds `DATABASE_URL`, `SESSION_SECRET`, `OAUTH_CLIENT_SECRET`,
  `RESTATE_BROKER_URL`, `RESTATE_AUTH_TOKEN`, `SENDGRID_API_KEY`, and optionally `SENDGRID_FROM_EMAIL`. Created manually
  from values fetched out of Secret Manager / the Restate CLI. The same Secret is `envFrom`'d by **both**
  `navigator-web` (direct sends from the admin "Send welcome" button) and `workflows-service` (durable
  `email_send__welcome` dispatch from the `onboarding__welcome` workflow) — one rotation lifts both pods.
- **ManagedCertificate CRs**: - `navigator-web` for `www.your-domain.example`. - `navigator-workflows` for
  `workflows.your-domain.example`.

### LB and host routing

One Global External Application Load Balancer fronts the cluster (legacy Ingress, `kubernetes.io/ingress.class: gce`).
Host-based routing splits traffic to two backend services:

- **`www.your-domain.example`** → Service `navigator-web` → `web` pod (plus `opa` sidecar and `cloud-sql-proxy`
  sidecar). Serves `/`, `/portal`, `/api`, and `/mcp`. Direct LB → pod via NEG (container-native LB). **No Envoy in this
  path.**
- **`workflows.your-domain.example`** → Service `workflows-service` → `worker` pod (plus `envoy` sidecar). Public
  endpoint that **Restate Cloud** dials to drive durable execution. The Envoy sidecar belongs to Restate's worker; it is
  **not** in the MCP or web request path. The worker also POSTs to SendGrid directly when a workflow lands on an
  `email_send__<slug>` step — the `onboarding__welcome` flow is the only such trigger today.

HTTP → HTTPS 308 redirect comes from `networking.gke.io/v1beta1.FrontendConfig`. TLS via Google-managed certs (one per
hostname, both attached to the same Ingress).

### Off-platform dependencies

- **Restate Cloud** — durable-execution control plane. Tenant ingress URL lives in `RESTATE_BROKER_URL`; Restate reaches
  `workflows-service` over the public LB at `workflows.your-domain.example`.
- **Google Sign-in** — browser SSO (id_token → session cookie for `/portal`).
  `OAUTH_ISSUER_URL=https://accounts.google.com`.

### Deliberately not deployed (each has a draft prompt)

- **Cloud Armor** (`ComputeSecurityPolicy` + `GCPBackendPolicy`) — removed to save ~$15/mo. No WAF / rate-limit at the
  LB today.
- **Config Connector** — controller isn't running on this GKE version; IAM stays gcloud-driven. **CSI Secret Manager
  driver** — driver name mismatch with the upstream `SecretProviderClass`. `navigator-web-secrets` is a plain K8s Secret
  managed by hand.
- **VPC Service Controls / Private Service Connect** — Restate Cloud reaches the worker over the public LB; private
  ingress is a later hardening step.

### Live: in-app Google OAuth validation on `/mcp`

`/mcp` is gated by `web::google_oauth::require_google_oauth`, which validates the bearer token Gemini Enterprise sends
by calling Google's tokeninfo endpoint (`https://oauth2.googleapis.com/tokeninfo`) and verifying:

- `aud` (or `azp`) is in an allowlist of OAuth client IDs from `GOOGLE_OAUTH_CLIENT_IDS` `email_verified` is `true`
  `email` ends with `@<GOOGLE_OAUTH_REQUIRED_HD>` (Workspace domain enforcement)

The path-routed `navigator-web-mcp` Service + BackendConfig + Ingress rule are kept as scaffolding so a future
IAP-compatible caller can be re-enabled with one flag flip (today the BackendConfig has `iap.enabled: false`). See
`docs/gemini-enterprise-mcp.md` for the history that explains why the scaffolding exists.

**Pinned identifiers (cite these in code / scripts; don't re-derive):**

- **Project number** — `YOUR_PROJECT_NUMBER` **IAP-scaffolding K8s Service** — `navigator-web-mcp` (selector matches
  `navigator-web` pods). Currently un-gated at the LB (BackendConfig has `iap.enabled: false`); kept for future use.
- **GKE-mangled backend service name** — `k8s1-e820f1a0-navigator-navigator-web-mcp-80-44ef5975` **OAuth clients
  accepted by `web::google_oauth`** (env `GOOGLE_OAUTH_CLIENT_IDS`, comma-separated): -
  `YOUR_OAUTH_CLIENT_ID_BROWSER.apps.googleusercontent.com` — the canonical client: navigator-web browser SSO **and**
  the Gemini Enterprise Custom MCP Server data store. One client, one rotation, one audit trail. -
  `YOUR_OAUTH_CLIENT_ID_GEMINI.apps.googleusercontent.com` — original Gemini Enterprise client, kept in the allowlist
  while the legacy data store is being retired. Drop from the env list (and from APIs & Services → Credentials) once no
  `python-httpx` traffic has cited it in 30 days.
- **Required Workspace domain** (`GOOGLE_OAUTH_REQUIRED_HD`) — `neonlaw.com` **Canonical Gemini Enterprise MCP URL** —
  `https://www.your-domain.example/mcp`. The retired `navigator.neonlaw.com` hostname **must not** appear in any Gemini
  Enterprise data store config; DNS no longer resolves, so `/mcp` requests die before leaving Google's network. See
  `docs/gemini-enterprise-mcp.md` → "Common pitfalls" for the symptom + first-step diagnostic.

**Existing IAM bindings (currently dormant)** on
`projects/YOUR_PROJECT_NUMBER/iap_web/compute/services/k8s1-e820f1a0-navigator-navigator-web-mcp-80-44ef5975`
(`roles/iap.httpsResourceAccessor`):

- `domain:neonlaw.com` — bound while IAP was active. Stays in place for the day IAP is re-enabled; harmless while
  `iap.enabled: false`.

**Workspace groups** (Cloud Identity, used by Gemini Enterprise app-sharing UI which does NOT accept `domain:`):

- `gemini-users@neonlaw.com` — created via `gcloud identity groups create` against org `517367957661`. The Gemini
  Enterprise App that wraps the `navigator-crm` data store is shared with this group. Add members with the standard
  `gcloud identity groups memberships add` against `--group-email=gemini-users@neonlaw.com` and the user email.

The mangled backend-service name is stable across redeploys (GKE NEG names are deterministic from the namespace +
Service + port + a content hash); rebuilding the LB or recreating the Service preserves it as long as those four don't
change.

**Path routing.** `www.your-domain.example/mcp` and `/mcp/*` route to the `navigator-web-mcp` Service (IAP on).
Everything else routes to the original `navigator-web` Service (IAP off) so the marketing site, `/portal` browser SSO,
and `/api` stay public-reachable. Both Services select the same pods.

**Allowlist mechanism quirk.** IAP IAM (`roles/iap.httpsResourceAccessor`) accepts `user:`, `group:`, `serviceAccount:`,
`domain:` — but **rejects bare OAuth client IDs**. OAuth clients for programmatic access go in a separate stanza,
`IapSettings.accessSettings.oauthSettings.programmaticClients`, updated via `gcloud iap settings set` (or the equivalent
REST PATCH). Same gate, two policy surfaces. `navigator gcp iap grant` handles the IAM case; OAuth-client allowlist is
currently a one-line gcloud invocation documented in the runbook.
