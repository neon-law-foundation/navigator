# Running Navigator on AWS, Azure, or self-hosted Kubernetes

Navigator's application code is cloud-agnostic. The Rust workspace depends on two abstractions — `cloud::StorageService`
and `store::Db` — plus a handful of SaaS-shaped env-driven integrations (OIDC, OPA, Restate, SendGrid). Nothing in the
canonical build pulls in a GCP-only SDK at compile time.

What ships **wired up** is the GCP path (see [`oss-install.md`](oss-install.md)). What ships **sketched** below are the
moving parts you'd swap to run on a different cloud. None of the sketches has production-equivalent test coverage today
— patches welcome.

## What's actually cloud-bound

- **Object storage** — today: `cloud::GcsStorage` (talks the GCS REST API directly via `reqwest`). Expects: a
  GCS-compatible HTTP API. S3 and Azure Blob differ at the wire level — see "Storage backends" below.
- **Postgres** — today: Cloud SQL. Expects: any managed or self-hosted Postgres 14+ that speaks the vanilla wire
  protocol. SeaORM doesn't care which cloud.
- **Identity / OIDC** — today: Google Identity Services. Expects: any OpenID-Connect compliant provider (Auth0, Okta,
  Keycloak, Azure AD, AWS Cognito). The flow follows the spec, not Google.
- **Workflow durability** — today: Restate Cloud (managed). Expects: either Restate Cloud (anywhere) or the Restate
  Operator running in your own cluster. The wire protocol is the same.
- **Email** — today: SendGrid. Expects: any SMTP-shaped backend. The `EmailService` trait is the abstraction; add a
  `SesEmail` or `SmtpEmail` implementation and select via `NAVIGATOR_EMAIL_BACKEND`.
- **Container runtime** — today: GKE Autopilot. Expects: any Kubernetes 1.27+ cluster. EKS, AKS, kind, k3s — the
  manifests are vanilla Kubernetes apart from a few GKE-only annotations (Workload Identity, ManagedCertificate).
- **LLM router (optional)** — today: Vertex AI Gemini Flash. Expects: any LLM the `AgentRouter` trait can dispatch.
  The prod implementation is one of three (`GeminiRouter`, `NullRouter`, …); add a `BedrockRouter` or an
  `AzureOpenAIRouter` next to it and wire via `web::build_router`.

## AWS / EKS sketch

1. **Identity**: register an Auth0 or Cognito user pool. Point `OAUTH_ISSUER_URL`, `OAUTH_CLIENT_ID`,
   `OAUTH_CLIENT_SECRET`, and `OAUTH_REDIRECT_URI` at it. The browser-side flow doesn't change.
2. **Postgres**: RDS Postgres or Aurora Postgres. Set `DATABASE_URL` to the instance's wire endpoint (with
   `?sslmode=require`).
3. **Storage**: today there is **no S3 `StorageService` implementation**. The `cloud` crate has the `StorageService`
   trait and an `FsStorage` (dev) and `GcsStorage` (prod) implementation; adding `S3Storage` is the right next step. The
   trait is small (five async methods), so the work is bounded. Until that exists, S3 deployments either (a) run a
   GCS-compatibility shim (Cloudflare R2 with the GCS layer, or `fake-gcs-server` for non-prod) or (b) carry a local
   S3-backed fork of `cloud`.
4. **Workflow runtime**: run the Restate Operator in your EKS cluster (it has no GCP-only assumptions), or sign up for
   Restate Cloud (multi-region; works from anywhere).
5. **Kubernetes manifests**: start from `examples/deploy/k8s/gke/` and remove the GKE-specific bits —
   `ManagedCertificate`, `BackendConfig`, `iam.gke.io/gcp-service-account` annotations, the Workload Identity wiring.
   Replace the Ingress class with `alb` or `nginx`. Cert-manager + Let's Encrypt is the easy path for TLS.
6. **Email**: SendGrid runs from anywhere. If you want SES instead, write an `SesEmail: EmailService` and add a `ses`
   branch to `workflows-service::email_config::select_backend`.

## Azure / AKS sketch

The shape is identical to EKS, with two substitutions:

- **Identity** — Microsoft Entra ID (formerly Azure AD) is a fine OIDC provider; the redirect URI shape is the same.
- **Storage** — Azure Blob Storage. Same gap as S3: write an `AzureBlobStorage: StorageService`.

The Kubernetes manifests don't need cluster-specific changes beyond the ingress class and the cert source.

## Self-hosted / generic Kubernetes

If you're running k3s, k0s, kind, or a vanilla kubeadm cluster:

- Run Postgres in-cluster via the Bitnami / Zalando operator, or point at any external instance.
- Run Keycloak in-cluster for OIDC (see [`oidc.md`](oidc.md) — the KIND dev path uses exactly this).
- Run the Restate Operator in-cluster for durable workflows.
- Use the `fs` storage backend (`NAVIGATOR_STORAGE_BACKEND=fs`) backed by a PersistentVolume, or stand up
  `fake-gcs-server` / MinIO for S3-compatible storage.

This is essentially the KIND dev path scaled out — see [`cli/README.md`](../cli/README.md).

## Status of the cloud-agnostic surface

| Item | Status |
| --- | --- |
| `cloud::StorageService` trait | exists, used by `web` |
| `cloud::FsStorage` (dev) | ships |
| `cloud::GcsStorage` (GCP) | ships |
| `cloud::S3Storage` (AWS) | not implemented |
| `cloud::AzureBlobStorage` | not implemented |
| `EmailService::SendGridEmail` | ships |
| `EmailService::SesEmail` | not implemented |
| `EmailService::SmtpEmail` (generic) | not implemented |
| `AgentRouter::GeminiRouter` | ships |
| `AgentRouter::NullRouter` | ships |
| `AgentRouter::ClaudeRouter` / `BedrockRouter` / `AzureOpenAIRouter` | not implemented |

Pull requests adding `S3Storage`, `SesEmail`, or `BedrockRouter` are welcome — each is a self-contained addition behind
an existing trait, and the test surface is small.
