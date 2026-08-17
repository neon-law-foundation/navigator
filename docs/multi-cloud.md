# Running Neon Law Navigator on AWS, Azure, or self-hosted Kubernetes

Neon Law Navigator's application code is cloud-agnostic. The Rust workspace depends on two abstractions —
`cloud::StorageService` and `store::Db` — plus a handful of SaaS-shaped env-driven integrations (OIDC, embedded Rego,
Restate, SendGrid). Nothing in the canonical build pulls in a GCP-only SDK at compile time.

What ships **wired up** is the GCP path (see [`oss-install.md`](oss-install.md)). What ships **sketched** below are the
moving parts you'd swap to run on a different cloud. None of the sketches has production-equivalent test coverage today
— patches welcome.

## What's actually cloud-bound

- **Object storage** — today: `cloud::GcsStorage` (talks the GCS REST API directly via `reqwest`). Expects: a
  GCS-compatible HTTP API. S3 and Azure Blob differ at the wire level — see "Storage backends" below.
- **The store** — today: a hosted SurrealDB. Expects: any SurrealDB reachable over `ws://` / `wss://`. The client
  doesn't care which cloud.
- **Identity / OIDC** — today: Google Identity Services. Expects: any OpenID-Connect compliant provider (Auth0, Okta,
  Rauthy, Azure AD, AWS Cognito). The flow follows the spec, not Google.
- **Workflow durability** — today: Restate Cloud (managed). Expects: either Restate Cloud (anywhere) or the Restate
  Operator running in your own cluster. The wire protocol is the same.
- **Email** — today: SendGrid. Expects: any SMTP-shaped backend. The `EmailService` trait is the abstraction; add a
  `SesEmail` or `SmtpEmail` implementation and select via `NAVIGATOR_EMAIL_BACKEND`.
- **Container runtime** — today: GKE Autopilot. Expects: any Kubernetes 1.27+ cluster. EKS, AKS, kind, k3s — the
  manifests are vanilla Kubernetes apart from a few GKE-only annotations (Workload Identity, ManagedCertificate).
- **LLM router (optional)** — today: Vertex AI Gemini Flash. Expects: any LLM the `AgentRouter` trait can dispatch.
  The prod implementation is one of three (`GeminiRouter`, `NullRouter`, …); add a `BedrockRouter` or an
  `AzureOpenAIRouter` next to it and wire via `portal::bootstrap`.

## AWS / EKS sketch

1. **Identity**: register an Auth0 or Cognito user pool. Point `OAUTH_ISSUER_URL`, `OAUTH_CLIENT_ID`,
   `OAUTH_CLIENT_SECRET`, and `OAUTH_REDIRECT_URI` at it. The browser-side flow doesn't change.
2. **The store**: any reachable SurrealDB. Set `NAVIGATOR_SURREAL_ENDPOINT` to its wire endpoint (with
   `?sslmode=require`).
3. **Storage**: select `NAVIGATOR_STORAGE_BACKEND=s3` and provide a region, endpoint, bucket names, and credentials.
   `S3Storage` uses `SigV4` and forced path-style addressing, so AWS S3 and conforming S3-compatible services share the
   same application contract.
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
  **Storage** — Azure Blob Storage. Same gap as S3: write an `AzureBlobStorage: StorageService`.

The Kubernetes manifests don't need cluster-specific changes beyond the ingress class and the cert source.

## Self-hosted / generic Kubernetes

If you're running k3s, k0s, kind, or a vanilla kubeadm cluster:

- Run SurrealDB in-cluster, or point at any external instance. Run Rauthy in-cluster
  for OIDC (see [`oidc.md`](oidc.md) — the KIND dev path uses exactly this). Run the Restate Operator in-cluster for
  durable workflows. Garage is the default open S3-compatible implementation; another conforming endpoint may be
  supplied through the same environment variables. The single-node KIND StatefulSet is disposable and is not a
  production HA topology.

This is essentially the KIND dev path scaled out — see [`cli/README.md`](../cli/README.md).

Garage is AGPL-3.0 software. Navigator runs the unmodified `dxflrs/garage:v2.3.0` image as a separate service and
communicates with it only through the S3 network API; its license and source remain those of the upstream project.

## Status of the cloud-agnostic surface

| Item | Status |
| --- | --- |
| `cloud::StorageService` trait | exists, used by `web` |
| `cloud::FsStorage` (dev) | ships |
| `cloud::GcsStorage` (GCP) | ships |
| `cloud::S3Storage` (S3-compatible) | ships; Garage is the open local/on-prem default |
| `cloud::AzureBlobStorage` | not implemented |
| `EmailService::SendGridEmail` | ships |
| `EmailService::SesEmail` | not implemented |
| `EmailService::SmtpEmail` (generic) | not implemented |
| `AgentRouter::GeminiRouter` | ships |
| `AgentRouter::NullRouter` | ships |
| `AgentRouter::ClaudeRouter` / `BedrockRouter` / `AzureOpenAIRouter` | not implemented |

Pull requests adding `SesEmail` or `BedrockRouter` are welcome — each is a self-contained addition behind an existing
trait, and the test surface is small.
