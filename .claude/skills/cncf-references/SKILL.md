---
name: cncf-references
description: >
  Index of canonical sources to consult when you have a Kubernetes, observability, identity, policy, storage, or
  service-mesh question in this workspace. Trigger when about to recommend a CNCF-tracked tool, when about to web-search
  for cloud-native concepts, or when verifying that a project is still actively maintained. Always prefer the project's
  own docs over secondary tutorials.
---

# Canonical sources for cloud-native + adjacent tools

This is an **index**, not a reference page. Each link points to the project's own primary documentation — its GitHub org/repo (where the code, releases, and security advisories live) or its docs site. Prefer these over secondary tutorials, Medium posts, or AI-generated walkthroughs.

When choosing a tool, the CNCF maturity level (Sandbox / Incubating / Graduated) and the last release date are the two signals worth checking before recommending it.

## CNCF discovery

- **CNCF landscape** (every project, sorted by category + maturity): <https://landscape.cncf.io/>
- **CNCF projects index** (graduated, incubating, sandbox lists): <https://www.cncf.io/projects/>
- **CNCF GitHub org**: <https://github.com/cncf>
- **CNCF Technical Oversight Committee (TOC)**: <https://github.com/cncf/toc>

## Tools we use today

| Tool | Role | Primary docs | Repo |
|---|---|---|---|
| **Kubernetes** | Container orchestration | <https://kubernetes.io/docs/> | <https://github.com/kubernetes/kubernetes> |
| **KIND** | Kubernetes in Docker (local dev) | <https://kind.sigs.k8s.io/> | <https://github.com/kubernetes-sigs/kind> |
| **nginx-ingress** | Ingress controller in the KIND cluster | <https://kubernetes.github.io/ingress-nginx/> | <https://github.com/kubernetes/ingress-nginx> |
| **Open Policy Agent** | Authorization decision engine | <https://www.openpolicyagent.org/docs/latest/> | <https://github.com/open-policy-agent/opa> |
| **OpenTelemetry** | Distributed tracing + metrics SDK | <https://opentelemetry.io/docs/> | <https://github.com/open-telemetry/opentelemetry-rust> |
| **Keycloak** | OIDC identity provider (local) | <https://www.keycloak.org/documentation> | <https://github.com/keycloak/keycloak> |
| **fake-gcs-server** | GCS emulator for local dev | — | <https://github.com/fsouza/fake-gcs-server> |
| **Restate** | Durable workflow broker | <https://docs.restate.dev/> | <https://github.com/restatedev/restate> |
| **PostgreSQL** | Database (Cloud SQL in prod) | <https://www.postgresql.org/docs/current/> | <https://github.com/postgres/postgres> |

## Adjacent CNCF projects worth knowing about (not adopted yet)

| Tool | Role | When you might reach for it |
|---|---|---|
| **CloudNativePG** | Postgres operator for K8s | If we ever need a real stateful in-cluster Postgres. <https://github.com/cloudnative-pg/cloudnative-pg> |
| **cert-manager** | Automated certificate issuance | If/when we terminate TLS in-cluster. <https://github.com/cert-manager/cert-manager> |
| **external-secrets** | Sync secrets from GCP Secret Manager / Vault | When secrets outgrow `kubectl create secret`. <https://github.com/external-secrets/external-secrets> |
| **Linkerd** / **Istio** | Service mesh (L7 routing, mTLS) | Multi-service deployment where mTLS-by-default matters. <https://linkerd.io/> · <https://istio.io/> |
| **Prometheus** | Metrics scraping + storage | Standard pairing with OTel; export metrics via OTLP, store in Prometheus. <https://github.com/prometheus/prometheus> |
| **Grafana** | Metrics + log visualization | UI on top of Prometheus / Loki / Tempo. <https://github.com/grafana/grafana> |
| **Jaeger** / **Tempo** | Distributed tracing storage | OTel collector → Jaeger or Tempo. <https://github.com/jaegertracing/jaeger> · <https://github.com/grafana/tempo> |
| **Argo CD** | GitOps-style deploys | When Makefile + `kubectl apply -k` outgrows one developer. <https://github.com/argoproj/argo-cd> |
| **Falco** | Runtime security | Production hardening. <https://github.com/falcosecurity/falco> |

## Rust-side dependencies (not CNCF, but canonical)

- **Tokio**: <https://github.com/tokio-rs/tokio> · <https://tokio.rs/>
- **Axum**: <https://github.com/tokio-rs/axum>
- **Tower**: <https://github.com/tower-rs/tower>
- **SeaORM**: <https://github.com/SeaQL/sea-orm> · <https://www.sea-ql.org/SeaORM/>
- **sqlx**: <https://github.com/launchbadge/sqlx>
- **Maud**: <https://github.com/lambda-fairy/maud>
- **reqwest**: <https://github.com/seanmonstar/reqwest>
- **oauth2-rs**: <https://github.com/ramosbugs/oauth2-rs>
- **jsonwebtoken**: <https://github.com/Keats/jsonwebtoken>
- **fantoccini** (WebDriver): <https://github.com/jonhoo/fantoccini>
- **tracing**: <https://github.com/tokio-rs/tracing>
- **OpenTelemetry Rust**: <https://github.com/open-telemetry/opentelemetry-rust>

## Rust language and toolchain

- **The Rust Book**: <https://doc.rust-lang.org/book/>
- **Rust by Example**: <https://doc.rust-lang.org/rust-by-example/>
- **Rust API Guidelines**: <https://rust-lang.github.io/api-guidelines/>
- **Rust Reference**: <https://doc.rust-lang.org/reference/>
- **Edition Guide**: <https://doc.rust-lang.org/edition-guide/>
- **Async Book**: <https://rust-lang.github.io/async-book/>
- **Clippy lint index**: <https://rust-lang.github.io/rust-clippy/master/>
- **rustfmt config**: <https://rust-lang.github.io/rustfmt/>
- **crates.io** (package registry): <https://crates.io/>
- **docs.rs** (auto-generated docs): <https://docs.rs/>
- **Rust release notes**: <https://github.com/rust-lang/rust/blob/master/RELEASES.md>

## How to evaluate a new dependency

Before adding a new crate or CNCF project:

1. **Is it in the CNCF landscape, or does it have an active GitHub repo?** No GitHub, no commits in the last 12 months → look elsewhere.
2. **What's the maturity / version?** CNCF Sandbox = experimental, Incubating = production with care, Graduated = production. crates.io 0.x = breaking changes possible; 1.x+ = semver promised.
3. **Who maintains it?** A single-maintainer project is a bus-factor risk. CNCF-graduated projects have governance docs in the repo.
4. **What does it pull in?** `cargo tree -p <crate>` reveals the transitive cost. A crate that pulls in tokio + reqwest + serde is cheap; one that pulls in a custom async runtime is not.
5. **Security advisories?** Check `cargo audit` and the project's GitHub security advisories tab.

## Anti-patterns

- Recommending tools based on memory without checking the linked source first — versions and APIs drift fast.
- Citing Medium / blog tutorials over the project's own docs. Tutorials go stale within months.
- Picking a tool because it appears in many tutorials, without checking CNCF maturity or recent releases.
- Reaching for a CNCF tool to solve a problem a Rust library already covers (e.g., Argo Workflows for a job that's three `tokio::spawn` calls).

## Related skills

- [[rust-concurrency]] — Tokio + async fundamentals
- [[rust-axum]] — HTTP routing on Axum
- [[rust-sea-orm]] — ORM patterns
- [[rust-service-lifecycle]] — startup, shutdown, probes
- [[rust-best-practices]] — language conventions
- [[kind-local-dev]] — KIND cluster lifecycle
- [[postgres-in-kind]] — in-cluster Postgres
- [[keycloak-oidc]] — OIDC identity flow
- [[opa-policy]] — authorization via OPA
