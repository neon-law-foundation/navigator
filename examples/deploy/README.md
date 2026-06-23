# `examples/deploy/` — sample cloud-deployment scaffolding

This directory holds the GKE / Cloud SQL deployment that NeonLaw's own production cluster runs against, restructured as
**examples** so the canonical workspace surface is just "build the crates + build the Docker images". The OSS user picks
one of these example overlays, substitutes the placeholders for their own cloud identifiers, and applies. Container
images are pulled from **public ghcr.io** (`ghcr.io/neon-law-foundation/navigator-*`) — published by CI, pulled
anonymously, so there is no in-cluster registry credential and no Artifact Registry to provision.

Nothing under `examples/deploy/` is on the default cargo build path. Nothing under here gets imported by Rust code. The
only Rust crate that references it is `cli`, and only via the `KUSTOMIZE_GKE = "examples/deploy/k8s/gke"` constant that
the `navigator kustomize-gke` subcommand (in the `cli::devx` module) consumes — and that's a deploy-time operator tool,
not part of the application's runtime.

## Layout

- **`k8s/gke/`** — Kustomize overlay for GKE Autopilot: Deployment + Ingress + ManagedCertificate + IAM + Config Sync.
  Production-ready shape; substitute placeholders to run.
- **`k8s/exports/`** — Optional nightly `archives` export `CronJob` that snapshots Postgres → Parquet → GCS → BigLake.
  Skip if you don't run analytics.

There is no CI example to copy: a fork inherits the canonical
[`.github/workflows/deploy.yml`](../../.github/workflows/deploy.yml), which builds every image and publishes it to that
fork's own `ghcr.io` (the publish job derives the owner from `${{ github.repository_owner }}`) tagged `YY.MM.DD` +
`latest`. Make those packages public so the cluster pulls them anonymously, then pin the dated tag in your overlay (or
roll it with `navigator power-push`).

## Placeholder contract

Every cloud-specific value in these files is a placeholder that begins with `YOUR_` or uses an unambiguous example
domain. The substitution table:

- **`YOUR_PROJECT_ID`** — Your GCP project's lowercase ID, e.g. `acme-prod-1234`. Discover with
  `gcloud config get-value project`.
- **`YOUR_PROJECT_NUMBER`** — Numeric form of the same project. Discover with
  `gcloud projects describe YOUR_PROJECT_ID --format='value(projectNumber)'`.
- **`YOUR_OAUTH_CLIENT_ID_BROWSER`** — OAuth 2.0 client ID for the browser-side SSO flow (Google / Workspace SSO).
  Discover in GCP Console → APIs & Services → Credentials, or via `gcloud iam oauth-clients list`.
- **`YOUR_OAUTH_CLIENT_ID_GEMINI`** — OAuth 2.0 client ID for the Gemini Enterprise data-store integration (if you
  wire it up). Drop this entry from `GOOGLE_OAUTH_CLIENT_IDS` if you don't use Gemini. Created when you enable the
  Gemini Enterprise connector.
- **`YOUR_DRIVE_FOLDER_ID`** — Google Drive shared-drive ID used for per-Project archives. Open the shared drive;
  the URL path segment after `folders/` is the ID.
- **`your-domain.example`** — The hostname your deployment serves under, e.g. `app.acme.com`. From your DNS provider.
- **`workflows.your-domain.example`** — Public ingress for the `workflows-service` worker that Restate Cloud dials.
  From your DNS provider.

## Substituting in-place

For a one-shot fork (the common case), drop the new values into a copy and apply:

```sh
cp -r examples/deploy/k8s/gke /tmp/my-overlay
find /tmp/my-overlay -type f \( -name '*.yaml' -o -name '*.yml' \) -print0 \
  | xargs -0 sed -i \
      -e 's|YOUR_PROJECT_ID|acme-prod-1234|g' \
      -e 's|YOUR_PROJECT_NUMBER|987654321098|g' \
      -e 's|YOUR_OAUTH_CLIENT_ID_BROWSER|123-abcdef|g' \
      -e 's|YOUR_OAUTH_CLIENT_ID_GEMINI|456-ghijkl|g' \
      -e 's|YOUR_DRIVE_FOLDER_ID|0ABCDEF1234567|g' \
      -e 's|your-domain.example|acme.com|g'
kubectl apply -k /tmp/my-overlay
```

For a long-lived fork, copy the example overlay into a sibling directory (e.g. `k8s/overlays/my-prod/`) and commit your
substituted values to your own private branch or repo. Keep `examples/deploy/k8s/gke/` itself untouched so upstream
merges remain clean.

## Secrets stay out of these files

These manifests reference values via env / `envFrom` from a Kubernetes Secret named `navigator-web-secrets` that you
create out-of-band with `kubectl create secret generic`. The Secret holds:

- `DATABASE_URL`
- `OAUTH_CLIENT_SECRET`
- `SESSION_SECRET`
- `RESTATE_BROKER_URL`
- `RESTATE_AUTH_TOKEN`
- `SENDGRID_API_KEY`
- `SENDGRID_INBOUND_SECRET`

None of these belong in a checked-in YAML.

## Other clouds

EKS / AKS / generic Kubernetes paths are sketched in [`../../docs/multi-cloud.md`](../../docs/multi-cloud.md). The Rust
app itself is cloud-agnostic — only this overlay tree is GCP-specific.
