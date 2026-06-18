# Third-party integrations — one vendor account per environment

Navigator talks to a handful of external services. They fall into two kinds:

- **Binding vendors** perform real, billable, or legally binding actions on the firm's behalf — DocuSign for
  e-signature, Xero for accounting and billing. For every such vendor we keep **two separate vendor apps/accounts**: a
  development (sandbox) account used for local dev and CI, and a production account used only in prod. This mirrors each
  vendor's own recommended setup — DocuSign's demo vs. production environments, Xero's demo company vs. live
  organisation — and it is the default way we develop against any binding third party.
- **Platform services** are the cloud infrastructure the app runs on — durable execution, object storage, the database,
  identity, the agent-router LLM, and outbound/inbound email. These don't take legally binding actions, so they don't
  need the two-account split; a fork points them at its own project / account through the same `<VENDOR>_*` env
  variables.

The [full catalog](#current-integrations) below lists every external service the application code itself dials. Purely
operational layers that sit *above* the env-var interface — Doppler (secret values), DNSimple (DNS) — are deliberately
out of scope here: they are not code dependencies, and a fork can swap them freely.

## Why two accounts

- **No legal or financial weight in dev.** A test envelope or a draft invoice created against the sandbox account is not
  a binding signature or a real ledger entry. A leaked dev key cannot mint a production signature request.
- **Clean books and clean signers.** Test data stays out of the real accounting ledger and off real signers' inboxes.
- **Self-testable forks.** An OSS adopter can stand up their own sandbox account and exercise the full flow without
  touching a real account or paying for live API calls.

## How we switch: by env file, not `APP_ENV`

There is no runtime mode switch — `APP_ENV` was removed in the SQLite cutover. The environment is selected by which env
*file* is loaded:

- `.env` holds the **sandbox** credentials and is auto-loaded on startup, so local dev and `cargo test` run against the
  vendor sandbox by default.
- `.env.production` holds the **production** credentials. It is gitignored by the `.env.*` rule and never committed. To
  run against production locally, source it over the defaults before launching the binary:

  ```bash
  set -a; source .env.production; set +a
  ```

Both files use the **same variable names** (`DOCUSIGN_*`, `XERO_*`, …) — the file is the namespace, so no code branches
on environment. In the deployed cluster the production values arrive via the Kubernetes Secret (Secret Manager →
`navigator-web-secrets`), so no file is sourced there.

Any vendor left entirely unconfigured falls back to an in-process **stub** that performs no external calls. That is the
safe default: a fresh checkout boots and self-tests without touching a real account.

## Current integrations

| Service | Purpose | Kind | Env prefix |
| --- | --- | --- | --- |
| DocuSign | E-signature | binding | `DOCUSIGN_*` |
| Xero | Accounting / billing (`ACCREC` invoices) | binding | `XERO_*` |
| Restate Cloud | Durable workflow execution (`workflows-service`) | platform | `RESTATE_*` |
| Google Cloud | Storage, Cloud SQL, OIDC, archive | platform | `NAVIGATOR_*`, `GOOGLE_OAUTH_*`, `DATABASE_URL` |
| Vertex AI | A2A agent-router LLM (Gemini Flash in prod) | platform | `NAVIGATOR_GCP_*` |
| SendGrid | Outbound + inbound email | platform | `SENDGRID_*` |

Notes:

- **Xero ↔ Mercury.** Xero reconciles against the firm's bank (Mercury) inside Xero itself. Navigator never speaks to
  Mercury — our only integration boundary is the Xero API.
- **Google Cloud is several spec-compliant touchpoints, not one SDK.** Object storage goes through the `cloud`
  crate's `StorageService` trait (GCS in prod, filesystem/`fake-gcs-server` in dev); the database is vanilla Postgres
  over `DATABASE_URL` (Cloud SQL in prod); OIDC is Google Identity validated against `GOOGLE_OAUTH_*`; the per-Project
  archive is Drive REST v3. See [`cloud/README.md`](../cloud/README.md) for the full resource map.
- **Vertex AI is pluggable.** The router is the `web::agent_router::AgentRouter` trait — `GeminiRouter` (Vertex AI) in
  prod, `NullRouter` in KIND. Swapping to another LLM means a new `impl`, not a new vendor account.

When you add a **binding** vendor, follow the two-account shape: create the sandbox + production accounts, add a
`<VENDOR>_*` block to `.env.example` that references this convention, put sandbox credentials in `.env` and production
credentials in `.env.production`, and fall back to a stub when the vendor is unconfigured. A **platform** service needs
only its own `<VENDOR>_*` block and a stub/local equivalent (`fake-gcs-server`, in-cluster Postgres, the `NullRouter`)
so a fresh checkout boots and self-tests without any cloud account.

## Not in this catalog — and why

A few external-looking things are deliberately absent. They are not third-party SaaS vendors, so the per-environment
account convention does not apply to them:

- **OPA (Open Policy Agent)** is **first-party infrastructure you self-host**, not a vendor. The *same* OPA container
  runs in both environments — a sidecar in the `web` pod in prod
  ([`examples/deploy/k8s/gke/patches/web-resources.yaml`](../examples/deploy/k8s/gke/patches/web-resources.yaml)) and
  the in-cluster service in KIND ([`k8s/base/opa/opa.yaml`](../k8s/base/opa/opa.yaml)), reached via `NAVIGATOR_OPA_URL`.
  There is no "OPA account" to sign up for.
- **OIDC identity is already the Google Cloud row.** In production, sign-in is **Google Identity** (validated against
  `GOOGLE_OAUTH_*`) — counted under Google Cloud above, not as a separate vendor. **Keycloak** is only its **local
  stand-in** (the dev/KIND OIDC provider), exactly as `fake-gcs-server` stands in for GCS and in-cluster Postgres for
  Cloud SQL. The identity provider is pluggable and spec-compliant either way.
- **Doppler and DNSimple** sit *above* the env-var interface — Doppler holds secret *values* (the app reads plain env
  vars; see [`secrets-doppler.md`](secrets-doppler.md)), DNSimple holds DNS records. Neither is a code dependency, and a
  fork can swap both freely.

## Related

- `.env.example` — the canonical per-variable reference; this convention is stated in its top "Conventions" block.
- [`oss-install.md`](oss-install.md) — the install walkthrough's env-configuration step.
- [`env-driven-devx.md`](env-driven-devx.md) — the broader "one config surface, three audiences" env philosophy.
