# Env-driven orchestration — one config surface, three audiences

The orchestration is part of the `navigator` CLI (the `cli` crate, `cli::devx` module in `cli/src/devx/mod.rs`). Its
production GCP path already reads configuration from `NAVIGATOR_*` environment variables via clap; its KIND/local path
historically used hard-coded module constants. This document records the design that brings the KIND path onto the same
env-driven surface, so one tool serves three audiences from one config file:

1. **Local dev** — a contributor runs `cargo run -p cli -- start-dev-server` against KIND with an empty `.env` and gets
   today's exact behavior.
2. **GCP dev** — the same operator runs `navigator gcp setup` / `deploy` against a real project, values from `.env`.
3. **OSS / multi-cloud forks** — a fork plugs its own cluster, namespace, overlay, and ports into `.env` and runs the
   same `navigator` CLI with no Rust edits, mirroring the `power-push` promise that "nothing is hard-coded."

The Council of Twelve review shaped this design; its findings are folded in below.

## The seam: one `KindConfig`, resolved once

All KIND/local knobs collapse into a single `KindConfig` struct, resolved once in `main()` from the environment and
threaded into the subcommands that need it. Each field falls back to a `DEFAULT_*` constant — the same value the old
inline `const` held — so an empty `.env` reproduces prior behavior exactly.

```rust
struct KindConfig {
    cluster: String,             // NAVIGATOR_KIND_CLUSTER       default "navigator"
    namespace: String,           // NAVIGATOR_K8S_NAMESPACE      default "navigator"
    deps_overlay: String,        // NAVIGATOR_KIND_DEPS_OVERLAY  default "k8s/overlays/kind-deps"
    full_overlay: String,        // NAVIGATOR_KIND_OVERLAY       default "k8s/overlays/kind"
    gke_overlay: String,         // NAVIGATOR_GKE_OVERLAY        default "examples/deploy/k8s/gke"
    postgres_port: u16,          // NAVIGATOR_KIND_POSTGRES_PORT default 15432
    restate_ingress_port: u16,   // NAVIGATOR_KIND_RESTATE_INGRESS_PORT default 9080
    restate_admin_port: u16,     // NAVIGATOR_KIND_RESTATE_ADMIN_PORT  default 9070
    opa_port: u16,               // NAVIGATOR_KIND_OPA_PORT      default 8181
    keycloak_port: u16,          // NAVIGATOR_KIND_KEYCLOAK_PORT default 30080
    fake_gcs_port: u16,          // NAVIGATOR_KIND_FAKE_GCS_PORT default 30443
    web_port: u16,               // NAVIGATOR_KIND_WEB_PORT      default 3001
}
```

Why a struct threaded once, not `env::var` at each call site: the constants were read at 20+ call sites across `up`,
`deploy`, `down`, `status`, `render_env`, and the cluster-lifecycle helpers. Scattering `env::var` would re-fragment the
config and make the next knob land inconsistently. One `from_env()` is the single place a reader looks, and the single
place a new knob is added.

## Naming: role, not provider

Variables are named `NAVIGATOR_<scope>_<thing>` so `.env.example` reads as one coherent table rather than two dialects:

- **Shared concepts get one var.** A Kubernetes namespace is the same idea in KIND and GKE, so it is
  `NAVIGATOR_K8S_NAMESPACE` (no `KIND`/`GKE` prefix).
- **Provider-specific concepts fork by scope.** The cluster name differs by provider — prod already has
  `NAVIGATOR_GKE_CLUSTER_NAME`, so the KIND cluster is `NAVIGATOR_KIND_CLUSTER`.
- **Overlay paths generalize.** `NAVIGATOR_KIND_OVERLAY` (full local stack) and `NAVIGATOR_GKE_OVERLAY` are the same
  idea at two scopes; a fork points either at its own kustomize overlay.

## Host ports: two kinds, one of which touches YAML

The host ports split into two categories with very different blast radius:

- **Pure port-forward ports** — postgres (15432), restate ingress (9080), restate admin (9070), opa (8181), and the
  local web port (3001). These are host-side `kubectl port-forward LOCAL:REMOTE` choices; `LOCAL` is arbitrary and
  touches nothing in the cluster. Making them env-driven is pure Rust with zero manifest changes.
- **Create-time NodePort mappings** — keycloak (30080) and fake-gcs (30443). These reach the host through
  `k8s/kind-config.yaml` `extraPortMappings`, which bind at cluster-create time. The `containerPort` is the Service's
  NodePort and stays fixed; only the `hostPort` is the host-side knob.

To make keycloak/fake-gcs host ports overridable without breaking a standalone `kind create cluster` against the
committed config, the `navigator` CLI renders `k8s/kind-config.yaml` to a temp file before cluster creation,
substituting only the two `hostPort:` values from the resolved `KindConfig`. When the ports are at their defaults the
rendered file is byte-identical to the committed one, so the committed `k8s/kind-config.yaml` keeps its literal defaults
and remains usable on its own. The `containerPort` / NodePort values are never touched, so the Service manifests stay in
sync.

## Implementation sequence

Three commits, smallest-first, so the risk-bearing YAML work lands last:

1. **Cluster + namespace.** Introduce `KindConfig` + `from_env()` + the `DEFAULT_*` constants; thread `&KindConfig`
   through the subcommands. Only `cluster` and `namespace` read env here; the threading is the bulk of the work and
   happens once, so later slices are additive field reads.
2. **Overlay paths.** Add the three overlay fields.
3. **Host ports + `kind-config.yaml` templating.** Add the seven port fields; render `k8s/kind-config.yaml` to a temp
   file for keycloak/fake-gcs.

## Testing

The orchestration had no tests before this work. The load-bearing test is "no env set → `KindConfig::from_env()` equals
the old constants exactly" — the safety net for the whole change. Each slice adds default-vs-override coverage for its
new fields, plus a `render_env` test (ports thread into the generated `.devx/env`) and a `kind-config.yaml` render test
(default ports → byte-identical output; overridden ports → only `hostPort` lines change). Tests land in the same commit
as the code they cover, per workspace TDD discipline.

## Related

- [`RUNBOOK.md`](RUNBOOK.md) — the dev loop this extends.
- [`cloud-operations.md`](cloud-operations.md) + [`.env.example`](../.env.example) — the env-driven prod surface this
  stays consistent with.
- [`oss-install.md`](oss-install.md) — `navigator gcp setup` env conventions.
