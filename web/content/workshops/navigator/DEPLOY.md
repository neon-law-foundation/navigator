# Deploy the Neon Law Navigator

Our firm runs Neon Law Navigator on Google Cloud. The Foundation gives the recipe away. This workshop stands up your
**own** instance — the same Rust stack our attorneys use, on your own Google Cloud project, for your own community. One
command does most of the work: `navigator gcp setup`, a provisioner written in Rust that talks to Google's REST APIs
directly and ships with a dry-run so you can read the whole plan before a single packet leaves your laptop.

Two things to hold up front. This provisions **billable** Google Cloud resources — a Cloud SQL instance, a GKE Autopilot
cluster, three storage buckets — so it is not free, and you should set a budget alert before you begin. And this is a
deployment guide for engineers standing up infrastructure. With that said: you can run the same stack we run. Let's
stand it up.

> **Want a free win first?** You do not need a cloud account — or a credit card — to see Neon Law Navigator run.
> `cargo run -p cli -- start-dev-server` brings the whole stack up locally in
> [KIND](https://kind.sigs.k8s.io/) (Postgres, OIDC, storage, the workflow broker, OPA), then `source .devx/env` and
> `cargo run -p web` serves it on `localhost`. Boot it empty, click around, and only come back here when you want it on
> the public internet. The full local loop is the `kind-local-dev` path in [`docs/RUNBOOK.md`](/docs/RUNBOOK).

**Set the budget alert before you provision** — one command caps the surprise so the bill cannot run away while you
learn:

```bash
gcloud billing budgets create --billing-account "$BILLING_ACCOUNT_ID" \
  --display-name "Neon Law Navigator" --budget-amount 200USD \
  --threshold-rule percent=0.5 --threshold-rule percent=0.9
```

Idle, the stack runs on the order of a small-instance Cloud SQL plus an Autopilot cluster's baseline — budget for it,
watch the first invoice, and scale the SQL tier down if it is more than you need.

## Agenda

Six steps, each tagged with its Bloom verb. You are the operator; the `navigator` CLI is the instrument:

- **Create** — stand up a billed project and authenticate.
- **Predict** — run `--dry-run` and read every API call before sending one.
- **Identify** — name the thirteen Google Cloud APIs the provisioner enables.
- **Explain** — describe the VPC, the Cloud SQL instance, and the three buckets.
- **Execute** — bring up the cluster, the static IP, and Fleet membership.
- **Verify** — ship the `web` image and confirm `/readyz` answers 200.

---

Each step is tagged with the Bloom verb it exercises (the [Anderson & Krathwohl 2001
revision](https://en.wikipedia.org/wiki/Bloom%27s_taxonomy)). You are the operator; the `navigator` CLI is the
instrument. **Create** — stand up a billed Google Cloud project and authenticate so `navigator` can act on your behalf.
**Predict** — run `navigator gcp setup --dry-run` and read every API call the provisioner _would_ make before sending
one. **Identify** — name the thirteen Google Cloud APIs the provisioner enables, and why each is needed. **Explain** —
describe the VPC, the Cloud SQL Postgres instance, and the three storage buckets, and why re-running setup is always
safe. **Execute** — bring up the GKE Autopilot cluster, the static IP, and Fleet membership with one command. **Verify**
— ship the `web` image and confirm the running service answers `/readyz` with a 200.

## Bring your own project

`navigator gcp setup` provisions _into_ a project; it does not create one. Start by creating a project and attaching a
billing account, then authenticate so the CLI can act as you:

```bash
gcloud projects create your-project-id --name "Neon Law Navigator"
gcloud billing projects link your-project-id --billing-account "$BILLING_ACCOUNT_ID"
gcloud auth application-default login
```

---

The [project creation guide](https://cloud.google.com/resource-manager/docs/creating-managing-projects) walks both the
create and the billing link. The provisioner then reads [Application Default
Credentials](https://cloud.google.com/docs/authentication/provide-credentials-adc) — the same ADC every Google client
library uses — so one `gcloud auth application-default login` covers it.

You also need `gcloud`, `kubectl`, and `docker` on your `PATH`: the cluster steps shell out to `gcloud`, the image ships
with `docker`, and you reconcile manifests with `kubectl`. Nothing in this workshop hard-codes our project name, region,
or cluster — every value flows through a flag or an environment variable, because this guide ships as open source for
you to point at your own cloud.

## Dry-run first

Before you change anything, read the plan. The `--dry-run` flag records every REST call and `gcloud` shell-out and
prints them without sending traffic:

```bash
cargo run -p cli -- gcp setup --project-id your-project-id --dry-run
```

---

`gcloud` has no universal dry-run equivalent, so we built one — it prints the plan without sending traffic or touching
your `gcloud` session. You will see thirteen planned actions: eight REST calls (enable APIs, create the VPC, create the
Cloud SQL instance plus its database and user, create three buckets) followed by five `gcloud` shell-outs for the
cluster. Read them, confirm the project ID and region are yours, then drop `--dry-run` to execute. Every step is
idempotent, so a re-run after a partial failure never produces duplicates.

## The APIs that light up

A real run first enables thirteen Google Cloud APIs in a single [Service
Usage](https://cloud.google.com/service-usage/docs/enable-disable) `batchEnable` call: `compute`, `sqladmin`,
`servicenetworking`, `storage`, `iam`, `container`, `gkebackup`, `configconnector`, `anthosconfigmanagement`, `logging`,
`secretmanager`, `certificatemanager`, and `speech`.

---

That `batchEnable` completes as one long-running operation. Enabling an already-enabled API is a no-op, so this step —
like every step — is safe to repeat. Nothing else in the run works until these are on, which is why it goes first.

## Network, SQL, and three buckets

With the APIs on, the CLI provisions the data plane over REST:

- A **custom-mode VPC** — no auto-created subnets, regional routing. A **Cloud SQL for Postgres** instance running
  Postgres 15, with a `navigator` database and a `web` user. **Three Cloud Storage buckets**, all uniform bucket-level
  access: `-assets` (public), `-documents` (private client documents), `-logs` (Nearline audit logs).

---

The [Cloud SQL for Postgres docs](https://cloud.google.com/sql/docs/postgres) cover the instance; the `web` user's
password is generated for you and printed to stderr **exactly once** — copy it then, because it is never stored or
echoed again. (In `--dry-run` no password is generated or printed.) The three
[buckets](https://cloud.google.com/storage/docs/creating-buckets) are: `your-project-id-assets` — public marketing
photography, the only bucket that ever gets a public binding; `your-project-id-documents` — **private** client
documents, where `web` writes content-addressed blobs, kept separate from assets so confidential data is never
co-mingled with anything public; and `your-project-id-logs` — long-lived audit and access logs, on the Nearline class
because they are written far more than read. Every create call treats an HTTP **409 Conflict** as success — that is
Google's "already exists" response — which is exactly what makes re-running setup safe rather than destructive.

## The cluster comes up

The cluster is the one part driven through `gcloud` rather than REST. In order, the provisioner reserves a static IP,
creates the [GKE Autopilot](https://cloud.google.com/kubernetes-engine/docs/concepts/autopilot-overview) cluster, and
registers it as a Fleet member:

```bash
cargo run -p cli -- gcp setup --project-id your-project-id --region us-west4
```

---

The Container API spec is roughly two hundred lines of JSON, while the Autopilot one-liner does the same job with sound
defaults. The provisioner reserves a global static IP for the Gateway (so your DNS A record survives a cluster
re-create), creates the Autopilot cluster on the `rapid` release channel with the Secret Manager add-on, then registers
the cluster as a Fleet member. If you point `--config-sync-repo` at your fork, it also applies a [Config
Sync](https://cloud.google.com/kubernetes-engine/enterprise/config-sync/docs/overview) `RootSync` so the cluster pulls
its manifests from Git; omit the flag and that step is skipped — the right default until you are running GitOps.

## Secrets: the invariants that gate the boot

`web` fails **loudly** rather than degrading silently: a missing required value crashes startup with a structured
`enforce_prod_invariants` error naming exactly what is absent. So a `CrashLoopBackOff` is almost always a missing
secret:

```bash
kubectl logs deploy/navigator-web -n navigator
```

---

The boot-invariant set is `DATABASE_URL`, `RESTATE_BROKER_URL`, `NAVIGATOR_OPA_URL`, `NAVIGATOR_STORAGE_BACKEND=gcs`,
`SESSION_SECRET`, the `SENDGRID_*` keys, and `DOCUSIGN_HMAC_KEY`. The always-current list and every variable's meaning
live in the canonical docs — this workshop deliberately does not copy them so it cannot drift. See
[`docs/oss-install.md`](/docs/oss-install) §4 and `.env.example`.

Two rules keep client data safe, and the deploy will not let you skip them. First, **secrets never live in the manifest
tree** — create the runtime Secret with `kubectl create secret` out-of-band (the full `--from-literal` command is in
`oss-install.md` §4), so credentials never enter Git. Second, **one interface, your choice of source** — NeonLaw keeps
values in [Doppler](https://www.doppler.com/) (`dev` for local and CI, `prd` rendered into Secret Manager); see
[`docs/secrets-doppler.md`](/docs/secrets-doppler). Doppler is optional: a fork can ignore it and use a gitignored
`.env` instead. The env-var interface is identical either way.

## Sign-in: bring an OIDC provider, never store passwords

Neon Law Navigator **never stores a password** — no password column, no hashing crate. Identity is delegated to an
**OIDC-compatible provider** you bring, via the standard Authorization Code + PKCE flow. Four env vars wire it:

```bash
OAUTH_ISSUER_URL=...        # the provider's issuer; discovery hangs off /.well-known/openid-configuration
OAUTH_CLIENT_ID=...
OAUTH_CLIENT_SECRET=...
OAUTH_REDIRECT_URI=https://www.your-domain.example/auth/callback
```

---

Neon Law Navigator speaks the standard Authorization Code + PKCE flow against the provider (`/auth/login` →
`/auth/callback`) and discovers every endpoint from `<issuer>/.well-known/openid-configuration`, so no provider URL is
hard-coded. Worked examples for Keycloak, Google, Auth0, and Okta live in `.env.example`.

**Why we delegate rather than store.** A legal-services portal holding its own password hashes would own a breach
liability, a reset / MFA / lockout system to build and operate, and an account-recovery support burden — none of which
serve the mission. Delegating to a provider that already does identity well keeps that surface off our plate, and the
identity/authorization split keeps it clean: the provider asserts only _who you are_ (a stable `sub` and an `email`),
while your `persons` row owns _what you can do_ (the single `role`). Granting or revoking access is one SQL statement,
never a provider reconfiguration — see [`docs/oidc.md`](/docs/oidc) for the full model.

**Email/password without Google — bring a provider that hosts its own login.** The person a clinic serves may have no
Google account, and "Sign in with Google" cannot be the only front door of a public legal-services portal. Any standards
OIDC provider that hosts its own email/password login page works with the env-swap above and **zero Neon Law Navigator
code changes** — because email/password is a feature of the _provider_, not of Neon Law Navigator. **Keycloak** is the
same open-source IdP the local KIND loop already runs; it serves email/password, self-registration, password reset, and
email verification from its own hosted pages, runs in your cluster with no per-user fee, and is the recommended
no-Google path. **Auth0 / Okta** are hosted SaaS equivalents: same four env vars, same redirect flow, their own login +
reset pages.

**GCP Identity Platform — the Google-managed option.** If you want Google to own the account store (NeonLaw's own prod
choice — see [`docs/gke-prod.md`](/docs/gke-prod)), Identity Platform is Google's customer-identity service, with
multi-tenancy that maps one tenant per white-label client. One honest caveat to plan for: its _first-party_
email/password is the client-SDK + ID-token-verify model (tokens issued by `https://securetoken.google.com/<project>`),
a different integration than the pure redirect env-swap above — follow Google's setup for it. For the shortest no-code
email/password road, a hosted-login IdP like Keycloak is the simpler choice.

## The external surface — every third party, in one place

Neon Law Navigator's whole external surface is six services, in two kinds — **platform services** (the cloud the stack
runs on) and **feature vendors** (each lights up one capability and stubs out cleanly when unconfigured):

| Service | What it gives you | Kind | At boot |
| --- | --- | --- | --- |
| Google Cloud | Storage, Cloud SQL, OIDC, archive | platform | required — provisioned by `navigator gcp setup` |
| Restate Cloud | Durable workflow execution (`workflows-service`) | platform | required — the workflow broker |
| Vertex AI | The A2A agent-router LLM (Gemini Flash in prod) | platform | optional — `NullRouter` until configured |
| DocuSign | E-signature | feature | stubs until `DOCUSIGN_*` is set |
| Xero | Accounting / billing (`ACCREC` invoices) | feature | stubs until `XERO_*` is set |
| SendGrid | Outbound + inbound email | feature | stubs until `SENDGRID_*` is set |

---

The full catalog, with env prefixes and the per-environment account rule, is
[`docs/third-party-integrations.md`](/docs/third-party-integrations) — the table above is the deployer's-eye view of it.
Two things worth saying out loud for a copyist:

- **It boots empty.** You need **none** of the feature vendors to stand the platform up. Any vendor you leave
  unconfigured falls back to an in-process **stub** that makes no external calls, so a fresh deploy serves the portal,
  the per-matter git repositories, and the document surfaces on nothing but Postgres and storage. Wire a real vendor
  only when you want that feature. (Locally, the platform services have stand-ins too — `fake-gcs-server`, in-cluster
  Postgres, the KIND Restate Operator — so the whole stack runs with no cloud account at all.)
- **One vendor account per environment.** When you wire a feature vendor, use a **separate account per environment** — a
  free sandbox for dev and CI, a production account only in prod — so test data never lands in your real books or in
  front of a real signer. That convention, and the "one app, two environments" model that keeps tests off your
  production envelope quota, lives in [`docs/third-party-integrations.md`](/docs/third-party-integrations) and
  [`docs/docusign-esignature.md`](/docs/docusign-esignature).

One boundary worth naming: **Xero reconciles against the firm's bank (Mercury) inside Xero** — Neon Law Navigator never
speaks to Mercury. Our only integration edge is the Xero API. That is the shape to copy: integrate the system of record,
not everything it in turn connects to.

## Ship and verify

Provisioning gives you an empty cluster; now ship `web` onto it. GitHub Actions builds and publishes every image
publicly, so the cluster pulls anonymously — pin the dated tag and reconcile the overlay:

```bash
kubectl apply -k examples/deploy/k8s/gke
```

---

You do not build or push an image by hand: GitHub Actions builds every image on the daily tag and publishes it to
**public [GitHub Container Registry](https://docs.github.com/packages)** at `ghcr.io/<your-org>/navigator-web:YY.MM.DD`
(the publish job derives the owner from your own fork's repository, automatically). Because the packages are public, the
cluster pulls them **anonymously** — there is no in-cluster registry credential to create and no Artifact Registry to
provision.

One prerequisite the apply step depends on: **make your `navigator-*` packages public, once.** A package published by
Actions defaults to private even when the repo is public, and a private package the cluster can't read fails as an
opaque `ImagePullBackOff`. Flip each at GitHub → your org → **Packages** → the package → **Package settings** → **Change
visibility** → Public. "Public" means **pull-only to the world** — anyone can read the image bytes, but only your org
can publish them, and it does **not** make your client data public: the private documents bucket from the last step
stays private. Then pin the dated `YY.MM.DD` tag in the overlay — **never `:latest` on a workload**, because a moved
`latest` is a deploy you can't audit — and reconcile the parameterized overlay shipped in `examples/deploy/k8s/gke` with
your project's values supplied as a Kubernetes Secret and the runtime `.env`.

Then confirm the service is live — and you can do it **from the page itself**. The site footer renders the deployed
release as "Neon Law Navigator YY.MM.DD", so the moment your new image is serving traffic the footer changes: that is
your end-to-end "it worked." For a scripted check:

```bash
curl -fsS https://www.your-domain.example/readyz
curl -fsS https://www.your-domain.example/version   # {"release":"YY.MM.DD","commit":"…",…}
```

`web` exposes a readiness endpoint that returns `200 OK` only once it has a database connection and its dependencies in
hand, and a `/version` endpoint whose `release` field is the very same `YY.MM.DD` the footer shows. A `200` on `/readyz`
means the same stack our firm runs is now answering on your own cloud, and the `release` field — identical to the footer
line — tells you which dated image landed, so every push is verifiable without shelling into a pod. (NeonLaw's own ship
step is wrapped in a one-shot `power-push` helper that resolves the latest published tag and rolls both deployments onto
it in a single command — handy, but NeonLaw-specific; the generic `kubectl apply` above is all the deploy actually
requires.)

## Drive it from the CLI

Once your instance answers `/readyz`, the `navigator` CLI runs the firm's whole matter flow against it from your
terminal. It authenticates like `gcloud auth login` and lands a short-lived (~8h) token at `~/.navigator.json`:

```bash
cargo install --path cli          # installs `navigator` on your PATH
# …or skip installing — the bin/navigator wrapper builds + runs it on first call:
export PATH="$PWD/bin:$PATH"       # then just call `navigator`
```

---

The login is a browser-loopback OAuth that reuses your instance's existing OIDC session and stores the token `0600` at
`~/.navigator.json` (a single gcloud-style dotfile; set `NAVIGATOR_CONFIG_DIR` to use the legacy
`<dir>/credentials.json` location instead). **The host is your deployment's** — `www.your-domain.example`, a staging
host, or `http://localhost:8080` for the KIND loop — so the same CLI drives whichever instance you point it at, each
keyed separately in the credential file. That is the whole reason `--host` is a flag and nothing about a domain is baked
in: this CLI is for _your_ install, not ours. The crate is `cli`; the binary it builds is `navigator`.

Then log in and drive a matter end to end:

```bash
navigator login --host www.your-domain.example   # opens the browser → ~8h token, stored 0600 (~/.navigator.json)
navigator whoami                                  # "you@example.org (admin) — expires in 7h52m"
navigator projects list                           # GET /portal/projects.csv → table (or --json)
navigator project open --name "Estate of Doe" \
  --client-name "Jane Doe" --client-email jane@example.org \
  --scope "Flat-fee estate planning"              # opens the matter; retainer parks at staff_review
navigator retainer approve <notation-id>          # renders + parks the retainer PDF (no envelope yet)
navigator notation status <notation-id>           # state + signature request id + document_ready
navigator retainer send <notation-id>             # dispatches one real envelope (409 until document_ready)
navigator retainer send <notation-id>             # idempotent — reuses the same envelope, no second send
navigator logout
```

Use that same sequence to **verify a fresh install end to end** — it is the smallest real exercise of the durable
pipeline. Point the client email at an inbox you control (never a third party — `send` transmits a binding engagement
letter), and walk the three assertions: `retainer approve` should leave the notation parked at
`document_open__retainer_pdf`; `notation status` should flip to `document_ready:true` once the worker has rendered and
persisted `document.pdf` (cross-check that the rendered object actually landed in your private documents bucket with
`gcloud storage ls gs://your-project-id-documents/notations/<notation-id>/`); and `retainer send` should report
`sent_for_signature__pending` with a signature request id, then reuse that same envelope on a second run. When you sign
or decline, the inbound webhook should log a HMAC-verified `esignature webhook: signature event` in the `navigator-web`
pod. Decline or void the test envelope afterward so no live engagement lingers against a real inbox.

After a single `login`, `--host` is optional — the one stored host is used — so the later commands stay short. Every
command is a thin client over a route `web` already serves, sent with `Authorization: Bearer <token>`: your instance
resolves that token back into your session and runs the same handler the browser does, so the `staff_review` gate, the
role check, and the `authored_by` provenance all hold unchanged. The send is a durable two-step: `retainer approve`
renders + parks the PDF on the worker, and the separate `retainer send` dispatches the envelope only after confirming
the PDF landed (`document_ready:true`), returning a `409` with a JSON reason — never an opaque 500 — until then. Sending
a retainer for signature stays a deliberate authenticated human command (`retainer send`) — it is never exposed as an
agent-routable tool. The full per-subcommand reference is the `cli` crate's `README.md` in the source tree.

## Make it yours — white-label under your own brand

Neon Law Navigator runs two brands from one binary, and every brand-identifying string is env-driven — so you can ship
it under your own name without forking source. Describe your organization once in a `navigator.yaml` brand pack:

```bash
cp navigator.example.yaml navigator.yaml   # then edit: names, emails, domain, logos
cargo run -p cli -- rebrand verify           # validate the pack
cargo run -p cli -- rebrand apply --out .devx/brand.env   # NAVIGATOR_* env + logos copied to web/public/
source .devx/brand.env
```

---

The pack sets only **identity** — names (`NAVIGATOR_BRAND_FIRM`), support addresses, postal addresses, the primary
domain, the consultation link, and your `logo-firm.svg` / `logo-firm.png`. It never machine-generates your binding legal
text.

Most firms already run their own marketing site and have a team for it, so Neon Law Navigator does not need to be your
public website — it can be just the client portal and workflow engine. **`NAVIGATOR_PORTAL_ONLY=true`** mounts only the
application surface (`/portal`, auth, `/api`, `/mcp`, the git transport, webhooks, the health probes, and the legal
pages) and drops the public marketing + Foundation site; `/` redirects to `/portal`, and your own website links to your
Neon Law Navigator portal. **`NAVIGATOR_TERMS_URL` / `NAVIGATOR_PRIVACY_URL`** point the footer's Terms and Privacy
links at the legal pages your own attorney publishes on your own site; `brand verify` rejects a portal-only pack with an
empty `terms_url` — so you never ship NeonLaw's bundled, Nevada-governed terms under your name.

If you would rather not run the install yourself, the Foundation will do it for you, migrate your data, and train your
team: see [Neon Law Foundation Nimbus](/foundation/nimbus).

## Canonical references

This workshop is the narrative; these docs are the source of truth and stay current — prefer them when they disagree:

- [`docs/oss-install.md`](/docs/oss-install) — the full end-to-end install (env, Secret, overlay, image, verify).
  [`docs/secrets-doppler.md`](/docs/secrets-doppler) — secrets management (Doppler `dev`/`prd`, or the `.env` fallback).
  [`docs/third-party-integrations.md`](/docs/third-party-integrations) — the per-environment vendor-account convention.
  [`docs/docusign-esignature.md`](/docs/docusign-esignature) — e-signature setup and the one-app, two-environment model.

---

This is the access-to-justice fight made deployable: the cheaper and more repeatable it is to stand up a grounded legal
harness, the more clinics and small firms can run one. Read the [Foundation mission](/foundation/mission) for why that
matters — and when your instance is live, tell us at
[support@neonlaw.org](mailto:support@neonlaw.org?subject=Deployed+the+Neon+Law+Navigator) so we can point the next
deployer at what you learned.
