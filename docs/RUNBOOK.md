# Local end-to-end runbook

Step-by-step instructions to bring the full Navigator stack up in a local KIND cluster and walk through the OIDC + admin
flow in Chrome. Every command in this document has been verified against the manifests and Makefile in the repo as of
the commit that introduces this file. The runtime steps (`docker`, `kind`, `kubectl`) run on your machine, so they're
marked with `🔧 you run`; everything else has been mechanically validated.

## 0. Prerequisites

```bash
docker --version    # any modern Docker / colima / OrbStack works
kind --version      # >= 0.32 — `kind load` needs it for the v1.36.1 node's containerd config v4
kubectl version --client
helm version        # OCI Helm chart installs the Restate Operator
restate --version   # Restate CLI — workflows-service registration
```

On macOS, install the cluster tooling with Homebrew (Docker comes from Docker Desktop / OrbStack / colima):

```bash
brew install kind kubectl helm          # cluster tooling
brew install restatedev/tap/restate     # Restate CLI
```

You also need to be in the `docker` group — verify with `docker info` (it should succeed without `sudo`). If your
machine wants `sudo docker`, either add yourself to the group via `sudo usermod -aG docker $USER` (then afterwards log
out and log back in to refresh) or run every `cargo run -p cli -- …` invocation with `sudo -E`.

## 1. Bring up the cluster (🔧 you run)

```bash
cd ~/Code/navigator
cargo run --release -p cli -- kind-up
```

What this does (look in `cli/src/devx/mod.rs` → `kind_up_steps`):

```text
kind create cluster --name navigator --config k8s/kind-config.yaml
kubectl apply -f https://raw.githubusercontent.com/kubernetes/ingress-nginx/.../deploy.yaml
kubectl --namespace ingress-nginx wait --for=condition=ready pod ...
helm upgrade --install restate-operator oci://ghcr.io/restatedev/restate-operator-helm ...
```

Expected output ends with something like:

```text
Set kubectl context to "kind-navigator"
pod/ingress-nginx-controller-... condition met
```

Takes ~60 seconds on a warm Docker daemon (longer the first time since it pulls the KIND node image).

### Quick sanity check

```bash
kubectl cluster-info --context kind-navigator
kubectl get nodes
# Expect: navigator-control-plane Ready, navigator-worker Ready
```

## 2. Build the image + deploy everything (🔧 you run)

```bash
cargo run --release -p cli -- deploy
```

What this does:

1. `docker build -t navigator-web:dev -f images/Dockerfile.web .` — two-stage build; ~2 min cold, ~30 s warm.
2. `kind load docker-image navigator-web:dev --name navigator` — pushes the image into the cluster's local registry.
3. `kubectl apply -f k8s/namespace.yaml` + every per-component subdirectory under `k8s/`.
4. `kubectl --namespace navigator rollout status deployment/navigator-web --timeout=300s`.

Expected final line:

```text
deployment "navigator-web" successfully rolled out
```

### What's now running

```bash
kubectl --namespace navigator get pods
```

Expect ~7 rows, all `Running` or `Completed`:

```text
NAME                              READY   STATUS      RESTARTS   AGE
keycloak-xxxxxxxxxx-xxxxx         1/1     Running     0          2m
fake-gcs-server-xxxxxxxxxx-xxxxx  1/1     Running     0          2m
fake-gcs-bootstrap-xxxxx          0/1     Completed   0          1m
navigator-web-xxxxxxxxxx-xxxxx    2/2     Running     0          1m
opa-xxxxxxxxxx-xxxxx              1/1     Running     0          2m
postgres-xxxxxxxxxx-xxxxx         1/1     Running     0          2m
restate-0                         1/1     Running     0          2m
```

`navigator-web` shows `2/2` because the OPA sidecar runs in the same pod. `fake-gcs-bootstrap` is the one-shot Job that
creates the `navigator` GCS bucket — `Completed` is the success state.

If any pod is stuck `Pending` or `CrashLoopBackOff`:

```bash
kubectl --namespace navigator describe pod <name>     # events at the bottom
kubectl --namespace navigator logs <name> --all-containers --tail=100
```

## 3. Grant staff the `staff` role in the DB

This is the deliberate split: Keycloak knows the staff user exists but the authz tier lives in our `persons` table.
Every person carries **exactly one** role — `client`, `staff`, or `admin` — in the `persons.role` column (see
[`docs/access-model.md`](access-model.md)). The Keycloak realm import creates staff; the `persons` row gets created on
its first login with `role = 'client'` (the safe default). To gate `/portal/admin/*`, we have to promote it to `staff`.

The cleanest way is to log in first (so the upsert happens), then update. But you can also pre-seed it. Either works:

```bash
# Option A — log in first, then grant.
# (Do this AFTER step 4 below has loaded the home page once.)
kubectl --namespace navigator exec deployment/postgres -- \
    psql -U navigator -d navigator -c \
    "UPDATE persons SET role = 'staff' WHERE email = 'staff@neonlaw.com';"

# Option B — pre-seed staff with the staff role so its first login
# inherits it via the email-match promotion path.
kubectl --namespace navigator exec deployment/postgres -- \
    psql -U navigator -d navigator -c \
    "INSERT INTO persons (name, email, oidc_subject, role) \
     VALUES ('Staff', 'staff@neonlaw.com', NULL, 'staff');"
```

Verify:

```bash
kubectl --namespace navigator exec deployment/postgres -- \
    psql -U navigator -d navigator -c "SELECT id, email, oidc_subject, role FROM persons;"
```

You should see staff with `role = staff`. After it logs in, `oidc_subject` populates with its Keycloak UUID.

## 4. Open Chrome (🔧 you do)

Five URLs to visit, in order. Each one exercises a different piece of the stack.

### 4.1 Navigator home page

<http://localhost:8080>

Verifies: nginx-ingress → `navigator-web` Service → pod → axum handler chain. Should render the home page immediately
(no auth required).

### 4.2 Start the OIDC flow

<http://localhost:8080/auth/login?return_to=/portal>

What happens behind the scenes:

1. `navigator-web` generates a PKCE verifier + a CSRF `state`.
2. Sets the `navigator_pre_auth` cookie (HMAC-signed, HttpOnly).
3. 302-redirects to Keycloak's `/realms/navigator/protocol/openid-connect/auth?...&code_challenge=...`.

Chrome will follow the 302 and land on the Keycloak login page.

### 4.3 Keycloak login

Chrome lands on Keycloak at <http://localhost:8080/keycloak/realms/navigator/protocol/openid-connect/auth?...> — the
nginx ingress routes `/keycloak/*` to the in-cluster Keycloak Service. The pod separately reaches Keycloak via cluster
DNS at `http://keycloak:8080/keycloak/...` for the backchannel `/token` exchange; Keycloak's hostname-v2 config
(`KC_HOSTNAME` + `KC_HOSTNAME_BACKCHANNEL_DYNAMIC=true`) keeps the two channels straight.

Credentials (from `k8s/overlays/kind/deps/keycloak.yaml` realm import):

| Field    | Value   |
|----------|---------|
| Username | `staff` |
| Password | `staff` |

Click "Sign In". Keycloak issues a one-time `code`, redirects back to
`http://localhost:8080/auth/callback?code=...&state=...`.

### 4.4 Callback completes, /portal renders

If you pre-seeded the staff role in step 3:

- Callback decodes id_token (`sub=<keycloak-uuid>`, `email=staff@neonlaw.com`).
- `upsert_person_from_claims` matches the seeded row by email, promotes it (stamps `oidc_subject`), reads
  `role = staff`.
- Session cookie set: `{ sub, email, person_id, role: "staff", exp, csrf_token }`.
- 302 → `/portal` — the role-aware landing. OPA allows any authenticated person here, so the dashboard renders;
  `role == "staff"` only becomes load-bearing on the `/portal/admin/*` routes below.

Now try the admin routes — each `/portal/admin/*` path hits the `staff` gate, while `/portal` and `/portal/projects`
need only an authenticated session:

- <http://localhost:8080/portal>
- <http://localhost:8080/portal/admin/people>
- <http://localhost:8080/portal/admin/entities>
- <http://localhost:8080/portal/admin/jurisdictions>
- <http://localhost:8080/portal/admin/entity-types>
- <http://localhost:8080/portal/admin/templates>
- <http://localhost:8080/portal/admin/questions>
- <http://localhost:8080/portal/projects>

All should return 200 and render their table.

### 4.5 Revoke the role, see the gate fire

```bash
kubectl --namespace navigator exec deployment/postgres -- \
    psql -U navigator -d navigator -c \
    "UPDATE persons SET role = 'client' WHERE email = 'staff@neonlaw.com';"
```

Then in Chrome:

1. Hit <http://localhost:8080/auth/logout> to clear the session.
2. Re-do <http://localhost:8080/auth/login?return_to=/portal>.
3. Log in as staff again.
4. `/portal` still loads (she is authenticated), but `/portal/admin` now returns 403.

This proves the gate is database-sourced — Keycloak hasn't changed, the token is identical, but access is gone.

## 5. Other consoles

| URL                                   | Login                                   |
| ------------------------------------- | --------------------------------------- |
| <http://localhost:30080/keycloak/>    | Keycloak admin (`admin` / `admin`)      |
| <http://localhost:30443/storage/v1/b> | fake-gcs-server HTTP API (list buckets) |

The Keycloak admin console lets you confirm the `navigator` realm, `navigator-web` client, and staff user are all live.
A `curl` against the fake-gcs-server endpoint above lists the `navigator` bucket created by the bootstrap Job.

## 6. Tail logs while you click around

```bash
# navigator-web (web container only)
kubectl --namespace navigator logs -f deployment/navigator-web -c web

# the OPA sidecar in the same pod
kubectl --namespace navigator logs -f deployment/navigator-web -c opa

# Keycloak (verbose; grep for 'event' or 'authorize')
kubectl --namespace navigator logs -f deployment/keycloak
```

## 7. Tear down

```bash
cargo run --release -p cli -- kind-down
```

Removes the entire KIND cluster. Re-run `cargo run --release -p cli -- deploy` to start fresh (it calls `kind-up` first
as a prerequisite).

## 7b. Fast loop — `web` on the host, deps in KIND

When you're actively editing the `web` crate, running `navigator deploy` on every change is too slow. Reach instead for
`navigator start-dev-server`: KIND hosts every dependency, but `cargo run -p web` runs in your shell so a `Ctrl-C` +
`cargo run` restart costs a single Rust rebuild rather than a docker build + kind load + rollout.

### Bring it up

```bash
cargo run --release -p cli -- start-dev-server
```

What this does (look in `cli/src/devx/mod.rs`):

1. `kind create cluster` (skipped if one already exists with the same name).
2. Installs nginx-ingress, then `kubectl apply` for every directory under `k8s/` **except `k8s/web/`**.
3. Waits for the Postgres, fake-gcs-server, Keycloak, OPA Deployments and the Restate StatefulSet to roll out.
4. Starts background `kubectl port-forward` processes:

   | Service         | In-cluster        | Host                                                       |
   | --------------- | ----------------- | ---------------------------------------------------------- |
   | Postgres        | `:5432`           | `localhost:15432` (5432 is often taken by a host Postgres) |
   | Restate ingress | `:8080`           | `localhost:9080` (8080 is taken by KIND's nginx)           |
   | Restate admin   | `:9070`           | `localhost:9070`                                           |
   | OPA             | `:8181`           | `localhost:8181`                                           |
   | Keycloak        | NodePort `:30080` | `localhost:30080` (kind-config mapping)                    |
   | fake-gcs-server | NodePort `:30443` | `localhost:30443` (kind-config mapping)                    |

5. Writes PIDs to `.devx/pids` and the env file to `.devx/env`.

### Run the web server locally

```bash
set -a; source .devx/env; set +a
cargo run -p web
```

The `set -a` block exports every `KEY=VALUE` line in `.devx/env` into your shell. `cargo run -p web` then binds `:3001`
with `DATABASE_URL` pointing at the in-cluster Postgres via the forwarded port, OAuth pointing at Keycloak on `:30080`,
OPA on `:8181`, fake-gcs on `:30443`.

The Keycloak realm in `k8s/overlays/kind/deps/keycloak.yaml` whitelists `http://localhost:3001/auth/callback` alongside
the existing `:8080` redirect URI, so the OIDC flow works in either deploy mode without realm edits.

### Open in Chrome

| URL                                                  | What it verifies                                |
| ---------------------------------------------------- | ----------------------------------------------- |
| <http://localhost:3001>                              | Local `cargo run -p web` → home page (no auth)  |
| <http://localhost:3001/auth/login?return_to=/portal> | OIDC flow against in-cluster Keycloak           |
| <http://localhost:30080/keycloak/>                   | Keycloak admin console (`admin` / `admin`)      |
| <http://localhost:30443/storage/v1/b>                | fake-gcs-server bucket list                     |

The OIDC login uses `staff` / `staff` (same realm as section 4.3 above). After login, `/portal` renders for any
authenticated person; the `staff` gate applies to `/portal/admin/*`, reached via the port-forward instead of an in-pod
sidecar (same policy either way).

### Hot-restart the web

Edit code, then in the same shell:

```bash
# Ctrl-C the running web, then:
cargo run -p web
```

No kubectl, no docker, no kind interaction needed — only the web binary recompiles. The cluster keeps its state across
restarts.

### Keep the deps up across sessions (the persistent fixture)

The dependency tier — the KIND cluster with Postgres, Keycloak, OPA, fake-gcs-server, and Restate — is a reusable
dev fixture, not a per-task resource. Stand it up once and **leave it running between sessions**; at handoff clean
up only the host-side `web` process, your task's `target/` artifacts, and rebuilt dev images. Creating the cluster
is the slow step, so reuse beats rebuild. Reach for full teardown (below) only when you intend to rebuild from
scratch.

- **Re-arm after a sleep or reboot.** The host-side `kubectl port-forward` processes (PIDs in `.devx/pids`) die when the
  laptop sleeps or the shell closes, even though the cluster keeps running — so `web` suddenly can't reach Postgres or
  OPA. Just re-run `start-dev-server`: it detects the existing cluster (`KIND cluster 'navigator' already exists,
  reusing`), re-applies the manifests (a no-op for unchanged pods, so **Postgres data is preserved**), kills any stale
  port-forwards, and respawns them. No cluster recreate, so it returns in seconds and rewrites `.devx/env`.
- **Bounce one wedged dependency** without touching the others:

  ```bash
  kubectl --namespace navigator rollout restart deployment/keycloak   # or opa, fake-gcs-server, postgres
  ```

  Keycloak re-imports its realm from the ConfigMap and OPA re-reads its policy bundle, so those come back clean. But
  Postgres and fake-gcs-server use `emptyDir`, so restarting **their** pod wipes the database and the bucket — after a
  `postgres` bounce, re-run the migrations and seed (see §7c). Restate has a PVC and survives a pod restart.
- **Reclaim disk without losing the fixture.** Prune images, build cache, and any leaked `postgres:17-alpine`
  testcontainers (`docker image prune -a`, `docker builder prune`); never `docker volume prune`, `down`, or `kind
  delete` for routine cleanup — those destroy the cluster and force a full rebuild. See the Resource cleanup section
  of [`agent-workflows.md`](agent-workflows.md#resource-cleanup).

### Tear down

Full teardown — only when you want a clean rebuild or to reclaim the cluster's disk, not as routine session cleanup:

```bash
cargo run --release -p cli -- down   # kills port-forwards, then `kind delete cluster`
```

### Devcontainer (optional)

If you don't want to install `kind` / `kubectl` natively, the `tools/dev/Dockerfile` bundles them with the pinned Rust
toolchain:

```bash
docker build -t navigator-devx:dev -f tools/dev/Dockerfile .
docker run --rm -it \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v "$PWD":/work -w /work --network host \
    navigator-devx:dev \
    cargo run --release -p cli -- start-dev-server
```

`--network host` is what lets the browser on the host reach the port-forwards started inside the container.

## 7c. Running the test suite

`cargo test` needs exactly one Postgres for the whole run — never one per test binary. Two ways to get it:

```bash
# Zero setup: the first run starts ONE reuse-labeled container; every
# later run, in any crate, reuses it. Reclaim it any time with:
#   docker rm -f $(docker ps -aq --filter label=org.navigator.test-postgres=shared)
cargo test --workspace

# Or point tests at an already-running Postgres (no Docker in the test
# path) — e.g. the KIND Postgres from `navigator start-dev-server`:
export TEST_DATABASE_URL=postgres://navigator:navigator@localhost:15432/navigator
cargo test --workspace
```

Each test still creates its own `test_<id>` schema, so tests run in parallel and never pollute the dev data even when
they share a server. The full rationale and the env contract are in [`test-database.md`](test-database.md).

## 8. What this verifies end-to-end

Walking through steps 1–5 demonstrates, *live*:

1. **Kubernetes deploy** of the full stack on a single laptop.
2. **OIDC Authorization Code + PKCE** against a real Keycloak.
3. **Person upsert** keyed on the OIDC `sub`, with email-match promotion for seeded rows.
4. **DB-sourced authz** — flipping `persons.role` in Postgres changes the gate decision on the next login.
5. **OPA policy decision** via the in-pod sidecar (zero-RTT localhost call).
6. **Ingress + Service + NodePort** routing through KIND's port mappings (`k8s/kind-config.yaml`).

The same three guarantees are verified statically by `web/tests/oidc_e2e.rs` — six integration tests against wiremock'd
IdP + OPA. Run them with `cargo test -p web --test oidc_e2e`.
