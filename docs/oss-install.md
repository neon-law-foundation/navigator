# Installing Navigator on your own cloud

Navigator's canonical build is just **`cargo build` + `docker build`**. Nothing in the workspace's default surface
assumes a particular cloud account, project ID, OAuth client, or domain. To run it against production traffic you
assemble three pieces — a runtime (Kubernetes, ECS, or plain Compose), a Postgres database, and a few SaaS dependencies
— and wire them together through env vars documented in [`../.env.example`](../.env.example).

This page walks the end-to-end setup against **GCP**, because the workspace ships a working example overlay for that
path. The same shape works against EKS, AKS, or self-hosted Kubernetes; see [`multi-cloud.md`](multi-cloud.md) for those
routes.

## 0. Prerequisites

- Rust 1.96 (`rustup toolchain install 1.96.0`)
- Docker (for image builds and the testcontainers-backed test suite)
- `kubectl`, `kustomize`, `gcloud` (only if you're following the GCP example)
- A domain you control, with the ability to set A records

## 1. Clone and build

```bash
git clone <your-fork-url> navigator
cd navigator
cargo build --workspace          # pulls dependencies + compiles every crate
cargo test  --workspace          # spins up testcontainers Postgres per test binary
```

Both commands work without any cloud account. The test suite uses `testcontainers` to spin up Postgres per test binary;
no `.env` needed yet.

## 2. Configure your `.env`

Copy the template and start filling values:

```bash
cp .env.example .env
```

The minimum to boot `web` against a real Postgres:

```dotenv
DATABASE_URL=postgres://user:pass@host:5432/navigator
NAVIGATOR_STORAGE_BACKEND=fs                   # `gcs` once you have GCS wired
NAVIGATOR_OPA_URL=http://opa:8181              # the OPA sidecar
SESSION_SECRET=<32 bytes from `openssl rand -hex 32`>
OAUTH_ISSUER_URL=https://accounts.google.com
OAUTH_CLIENT_ID=<your client id>
OAUTH_CLIENT_SECRET=<your client secret>
OAUTH_REDIRECT_URI=https://www.your-domain.example/auth/callback
RESTATE_BROKER_URL=<your Restate Cloud or in-cluster operator URL>
SENDGRID_API_KEY=<key>
SENDGRID_INBOUND_SECRET=<random secret>
```

Every variable is documented inline in `.env.example`. Production binaries **crash on startup** rather than degrading
silently if any of `RESTATE_BROKER_URL`, `NAVIGATOR_OPA_URL`, `NAVIGATOR_STORAGE_BACKEND=gcs`, `SENDGRID_API_KEY`, or
`SENDGRID_INBOUND_SECRET` are missing — see `web::config::enforce_prod_invariants`.

### Third-party integrations: a separate vendor account per environment

Some integrations talk to an external SaaS that issues real, billable, or legally binding actions — DocuSign
(e-signature) today, Xero (accounting/billing) next. For these, create **two accounts with the vendor**: a
development/sandbox account you use locally and in CI, and a production account you use only in prod. The sandbox
account keeps test data — unsigned envelopes, draft invoices — out of your real books and off real signers.

There is **no `APP_ENV` switch**. Selection is by env *file*:

- `.env` holds your **sandbox** credentials and is auto-loaded on startup — local dev and tests run against the vendor's
  sandbox by default.
- `.env.production` holds your **production** credentials. It is gitignored by the `.env.*` rule; never commit it. To
  run against production locally, source it over the defaults before launching the binary:

  ```bash
  set -a; source .env.production; set +a
  ```

Both files use the **same variable names** (e.g. `DOCUSIGN_*`) — the file is the namespace, so no code branches on
environment. In the deployed cluster the production values arrive via the Kubernetes Secret (Secret Manager →
`navigator-web-secrets`), so no file is sourced there. Any vendor you leave entirely unconfigured falls back to an
in-process **stub** that performs no external calls, so a fresh fork boots and self-tests without touching a real
account. See [`third-party-integrations.md`](third-party-integrations.md) for the full convention.

## 3. (GCP path) Provision the cloud resources

The `navigator` CLI ships a one-shot, idempotent provisioner for the GCP-side infrastructure — VPC, Cloud SQL Postgres,
two GCS buckets, GKE Autopilot cluster, Fleet membership, Gateway static IP.

```bash
gcloud auth application-default login
cargo run -p cli -- gcp setup \
  --project-id YOUR_PROJECT_ID \
  --region us-west2 \
  --cluster-name navigator-prod \
  --sql-instance navigator-pg \
  --vpc-name navigator-vpc \
  --gateway-ip-name navigator-gateway-ip
```

Each flag has a sensible default (see `cargo run -p cli -- gcp setup --help`) and falls back to a `NAVIGATOR_*` env var
if unset. Pass `--dry-run` first to print the exact REST calls / `gcloud` invocations the run will emit.

The subcommand prints a generated Postgres password **once** to stderr — paste it into your Secret Manager / Kubernetes
Secret immediately; there is no recovery path.

## 4. Adapt the example overlay

Copy [`examples/deploy/k8s/gke/`](../examples/deploy/k8s/gke/) to a private location (or to your own kustomize overlay
branch) and substitute the placeholders documented in [`examples/deploy/README.md`](../examples/deploy/README.md):

```bash
cp -r examples/deploy/k8s/gke /tmp/my-overlay
find /tmp/my-overlay -type f \( -name '*.yaml' -o -name '*.yml' \) -print0 \
  | xargs -0 sed -i \
      -e 's|YOUR_PROJECT_ID|acme-prod-1234|g' \
      -e 's|YOUR_PROJECT_NUMBER|987654321098|g' \
      -e 's|YOUR_OAUTH_CLIENT_ID_BROWSER|...|g' \
      -e 's|YOUR_OAUTH_CLIENT_ID_GEMINI|...|g' \
      -e 's|YOUR_DRIVE_FOLDER_ID|...|g' \
      -e 's|your-domain.example|acme.com|g'
```

Create the runtime Kubernetes Secret (out-of-band — `kubectl create secret` keeps the values out of the manifest tree):

```bash
kubectl -n navigator create secret generic navigator-web-secrets \
  --from-literal=DATABASE_URL='...' \
  --from-literal=OAUTH_CLIENT_SECRET='...' \
  --from-literal=SESSION_SECRET="$(openssl rand -hex 32)" \
  --from-literal=RESTATE_BROKER_URL='...' \
  --from-literal=RESTATE_AUTH_TOKEN='...' \
  --from-literal=SENDGRID_API_KEY='...' \
  --from-literal=SENDGRID_INBOUND_SECRET="$(openssl rand -hex 32)"
```

Apply:

```bash
kubectl apply -k /tmp/my-overlay
```

## 5. Build and push the image

The Dockerfile at the repo root produces a single multi-stage image with both `web` and `workflows-service` binaries
inside. Tag it for your registry and push:

```bash
TAG=$(git rev-parse --short HEAD)
docker build -t my-registry/navigator-web:$TAG .
docker push my-registry/navigator-web:$TAG
```

Then patch your overlay's image reference to the new tag (or set it via kustomize `images:` in your private overlay).

## 6. Verify

`kubectl get pods -n navigator` should show `navigator-web` running. Hit `https://www.your-domain.example/health` (must
return `OK`) and `https://www.your-domain.example/` (must render the home page). The first inbound request triggers OPA,
OIDC, and Restate handshakes — any missing env var crashes the pod with a structured `enforce_prod_invariants` error
before serving traffic, which is the loud-failure-by-design behavior.

## Where things go from here

- For Restate Cloud setup, see [`gke-prod.md`](gke-prod.md).
- For the Gemini Enterprise (A2A) wiring, see
  [`gemini-enterprise-mcp.md`](gemini-enterprise-mcp.md).
- For an OSS-friendly weekly deploy via GitHub Actions, copy
  [`../examples/deploy/ci/deploy-gke.yml.example`](../examples/deploy/ci/deploy-gke.yml.example) to
  `.github/workflows/deploy.yml` in your fork and set the project / region values as repository variables.
