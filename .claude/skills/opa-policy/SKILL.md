---
name: opa-policy
description: >
  Open Policy Agent (OPA) as the authorization decision point for `web` — sidecar deployment, REST query API, Rego
  policy authoring, ConfigMap-bundled policies, hot reload. Trigger when editing `k8s/opa/opa.yaml`, writing or
  modifying Rego, touching the OPA middleware in `web`, setting `NAVIGATOR_OPA_URL`, or adding a new `/portal/...` route
  that needs policy enforcement. Also trigger before reaching for a role-based authorization library — we route authz
  through OPA.
---

# Open Policy Agent (OPA) for navigator-web authorization

OPA is the **decision point**; the web server is the enforcement point. The split lets us change policy without redeploying the binary and lets us share the same decision engine across multiple services if we ever need to.

## Deployment shape

OPA runs as a **sidecar** in the `navigator-web` Pod:

- Web container listens on `:3001`.
- OPA container listens on `:8181`, loaded with a Rego bundle from a ConfigMap mount.
- Both containers share the pod network — web calls OPA on `http://localhost:8181`.

Manifest: `k8s/opa/opa.yaml` (the standalone OPA Deployment + ConfigMap for the realm) and the OPA sidecar block in `k8s/web/web.yaml`.

Standalone OPA exists for two reasons:
1. Lets you `kubectl exec` into it and run `opa eval` against the live policy.
2. Lets other services adopt OPA later without a per-pod sidecar.

In production, the sidecar pattern is what enforces decisions; the standalone Deployment is a debugging convenience.

## Query API

The web server posts request metadata to:

```
POST http://localhost:8181/v1/data/navigator/authz/allow
Content-Type: application/json

{
  "input": {
    "path": "/portal/admin/templates",
    "method": "GET",
    "session": { "person_id": 42, "roles": ["staff"] }
  }
}
```

OPA returns `{"result": true | false}`. The middleware honors `true`, returns 403 on `false`, and **fails closed** on transport errors (anything other than 200 with a boolean) — except in dev where `NAVIGATOR_OPA_URL` is unset and the middleware logs a one-line warning at boot and passes through.

`/v1/data/<package>/<rule>` is the canonical query shape. Package + rule names must match the Rego.

## Rego policy

Default policy in `k8s/opa/opa.yaml`:

```rego
package navigator.authz

default allow := false

allow if {
    input.method != ""
    not requires_staff
}

allow if {
    requires_staff
    "staff" in input.session.roles
}

requires_staff if startswith(input.path, "/portal/admin/")
```

Rules to keep:

- **`default allow := false`** at the top of every package. Default-deny is the only safe default.
- One rule per intent; let OPA's logical OR (multiple `allow if` blocks) compose them.
- Don't put business logic in Rego that belongs in Rust. Rego decides "is this allowed"; the database is still the source of truth for the data being protected.
- Tests for policy live alongside the Rego (`*_test.rego`); `opa test k8s/opa/policies/` runs them.

## Updating policy

```bash
# Edit the ConfigMap
$EDITOR k8s/opa/opa.yaml

# Apply
kubectl --context kind-navigator -n navigator apply -f k8s/opa/opa.yaml

# Hot reload (OPA polls the ConfigMap mount; no restart needed)
# Verify:
kubectl --context kind-navigator -n navigator logs -l app=opa --tail=20
```

OPA's `discovery` config or `bundles` config controls the reload mechanism. For ConfigMap-mounted policies, OPA's `--watch` flag reloads on file change — confirm it's set in the container args.

## Local Rego development

```bash
# Evaluate a policy + input pair without OPA running
opa eval -d k8s/opa/policies/ -i input.json 'data.navigator.authz.allow'

# Run the test suite
opa test -v k8s/opa/policies/

# Format
opa fmt -w k8s/opa/policies/
```

`opa` CLI is the single tool; install it via Homebrew, scoop, or the official release. Don't shell out to it from Rust — call the REST API.

## Decision logs

OPA can emit a structured decision log per query (input + result + policy version). Wire it to OTel collector or stdout in dev. Critical for post-incident debugging: "did OPA say yes or no for that user at 14:32?".

In `k8s/opa/opa.yaml`, the `decision_logs.console = true` setting in OPA's config dumps each decision as JSON to stdout — pickable by `kubectl logs`.

## Middleware

`web` has `opa_guard` middleware applied via `axum::middleware::from_fn_with_state`. It:

1. Builds `input` from request method/path + session cookie's `person_id` + roles.
2. POSTs to `${NAVIGATOR_OPA_URL}/v1/data/navigator/authz/allow`.
3. On `result: true` → call `next.run(req).await`.
4. On `result: false` → return `403 Forbidden`.
5. On transport error → log via `tracing` + return `503 Service Unavailable` (fail-closed in prod). In dev (env var unset), skip.

## Anti-patterns

- Embedding allowlists in Rust (`if path.starts_with("/portal/admin/") && !user.is_staff { … }`). That's authorization business logic — it belongs in Rego.
- Calling OPA from every handler. Authz is a cross-cutting concern → middleware applied once, with route-shape input.
- Hardcoding `localhost:8181` in handlers. Use `state.opa_url` (from `NAVIGATOR_OPA_URL`); the sidecar binds to localhost only by convention.
- Treating OPA as a config store. OPA evaluates policies against input + (optional) data; it's not a generic K/V store. Real data lives in Postgres.
- Running without the dev-only pass-through. If OPA is mandatory in dev, every contributor needs the full sidecar stack just to log in. The unset-env-var pass-through is the right ergonomic tradeoff.

## Canonical sources

- OPA project (CNCF graduated): <https://www.openpolicyagent.org/>
- OPA repository: <https://github.com/open-policy-agent/opa>
- OPA documentation (REST API, deployment models, decision logs): <https://www.openpolicyagent.org/docs/latest/>
- Rego language reference: <https://www.openpolicyagent.org/docs/latest/policy-language/>
- OPA HTTP API: <https://www.openpolicyagent.org/docs/latest/rest-api/>
- OPA Envoy plugin (if you ever want L7-mesh enforcement): <https://www.openpolicyagent.org/docs/latest/envoy-introduction/>
- CNCF project page: <https://www.cncf.io/projects/open-policy-agent-opa/>
