# Open Policy Agent (OPA)

How Neon Law Navigator runs authorization as a separate decision engine. OPA is the **decision point**; `web` is the
**enforcement point**. The split lets policy change without redeploying the binary, and keeps one decision engine that
other services can adopt later.

This page owns the *system* — deployment shape, the query mechanics, Rego authoring, and the Rust client. The
*semantics* of who-can-see-what (the `input` document, the allow rules, admin bypass, project scoping) live in
[`access-model.md`](access-model.md#how-opa-decides); read that first for any change to what a rule decides.

## Deployment shape

OPA runs as a **sidecar** in the `navigator-web` Pod:

- The web container listens on `:3001`.
- The OPA container listens on `:8181`, loaded with a Rego bundle from a ConfigMap mount.
- Both containers share the Pod network, so `web` calls OPA on `http://localhost:8181`.

The manifest is [`k8s/base/opa/opa.yaml`](../k8s/base/opa/opa.yaml) — the OPA Deployment plus the ConfigMap that carries
the policy. In production the sidecar is what enforces decisions; a standalone Deployment is a debugging convenience
that lets you `kubectl exec` in and run `opa eval` against the live policy.

## Query API

The web server posts request metadata to `<base>/v1/data/navigator/authz/allow` and reads back `result.allow` (a
boolean). `<base>` comes from `NAVIGATOR_OPA_URL`.

```text
POST http://localhost:8181/v1/data/navigator/authz/allow
Content-Type: application/json
```

The request body is the `input` document — its exact fields (`path` as a segment array, `method`, `session.role`,
`project_id`) are canonical in [`access-model.md`](access-model.md#how-opa-decides). `/v1/data/<package>/<rule>` is the
query shape; the package and rule names must match the Rego.

## Rego policy

The live policy ships in [`k8s/base/opa/opa.yaml`](../k8s/base/opa/opa.yaml) under package `navigator.authz`, with tests
in [`k8s/base/opa/navigator_test.rego`](../k8s/base/opa/navigator_test.rego). Rules to keep:

- **`default allow := false`** at the top of every package. Default-deny is the only safe default.
- One rule per intent; let OPA's logical OR (multiple `allow if` blocks) compose them.
- Don't put business logic in Rego that belongs in Rust. Rego decides "is this allowed"; the database is still the
  source of truth for the data being protected.
- Keep policy tests alongside the Rego (`*_test.rego`).

### Local Rego development

```bash
# Evaluate a policy + input pair without OPA running:
opa eval -d k8s/base/opa/ -i input.json 'data.navigator.authz.allow'

# Run the test suite:
opa test -v k8s/base/opa/

# Format:
opa fmt -w k8s/base/opa/
```

The `opa` CLI is the single tool; install it via Homebrew, scoop, or the official release. Don't shell out to it from
Rust — call the REST API.

### Updating policy

```bash
# Edit the ConfigMap, then apply:
kubectl --context kind-navigator -n navigator apply -f k8s/base/opa/opa.yaml

# OPA hot-reloads the ConfigMap mount (the container runs with --watch); no restart needed. Verify:
kubectl --context kind-navigator -n navigator logs -l app=opa --tail=20
```

### Decision logs

OPA can emit a structured decision log per query (input, result, policy version). With `decision_logs.console = true` in
OPA's config, each decision prints as JSON to stdout and is pickable by `kubectl logs`. Wire it to the OTel collector or
stdout in dev — it is what answers "did OPA say yes or no for that user at 14:32" after an incident.

## The Rust client

`web::policy` ([`web/src/policy.rs`](../web/src/policy.rs)) holds both halves:

- **`PolicyClient`** — reads the base URL via `PolicyClient::from_env` (`NAVIGATOR_OPA_URL`). When the var is unset it
  builds a **passthrough** client that returns `allow=true` without touching the network, so a contributor can log in
  without the full sidecar stack. `evaluate` POSTs the `input` and returns `result.allow`.
- **`require_policy`** — the Axum middleware, applied once via `axum::middleware::from_fn_with_state`. It builds `input`
  from the request method/path plus the session, posts to OPA, and on `allow=true` calls the next handler. On
  `allow=false` it returns `403 Forbidden`; on a transport error it **fails closed** (deny), except for the unset-env
  passthrough above.

## Anti-patterns

- **Authz logic in Rust** (`if path.starts_with("/portal/admin/") && session.role != "staff" { … }`). That is an
  authorization rule — it belongs in Rego.
- **Calling OPA from every handler.** Authz is cross-cutting → one middleware with route-shape input, not a per-handler
  call.
- **Hardcoding `localhost:8181`.** Use the client built from `NAVIGATOR_OPA_URL`; the sidecar binds to localhost only by
  convention.
- **Treating OPA as a config or K/V store.** OPA evaluates policies against `input` (and optional `data`); real data
  lives in Postgres.
- **Dropping the dev passthrough.** Without the unset-env pass-through, every contributor needs the full sidecar stack
  just to log in.

## Related

- [`access-model.md`](access-model.md) — the role + participation model and the canonical `input` document and allow
  rules. [`oidc.md`](oidc.md) — how the session (and its `role`) is populated at login.
- OPA documentation: <https://www.openpolicyagent.org/docs/latest/>. Rego language reference:
  <https://www.openpolicyagent.org/docs/latest/policy-language/>. REST API:
  <https://www.openpolicyagent.org/docs/latest/rest-api/>.
