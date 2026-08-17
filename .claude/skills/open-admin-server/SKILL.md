---
name: open-admin-server
description: >
  Bring up the local Navigator stack and land a signed-in Admin browser session on the admin console at `/app/admin`,
  driven through Claude Code for Chrome. Trigger on "Open Admin Server", "open the admin server", "log me into admin",
  "show me the admin console", or any request to look at an admin-only surface (People, Visitor analytics, Matters) on
  the local tier. Encodes the two failures that eat the first ten minutes whenever the cluster has been up since a
  previous day: a stale Rauthy pod that rejects the fixture password, and the `id_token verification failed` that
  follows recreating it. The generic browser loop is [[web-preview]]; the cluster half is [[kind-local-dev]]. This skill
  is the admin-login path specifically.
---

# Open Admin Server

Land a signed-in **Admin** session on `http://localhost:3001/app/admin` in the user's real Chrome.

## The target is `/app/admin`, not `/admin`

`ADMIN_LANDING_PATH` is `/app/admin` ([`portal/src/dioxus_app.rs`](../../../portal/src/dioxus_app.rs)). `/app` is the
mount namespace the portal reserves so future apps can hang off it, so the console hub lives inside it. The bare
`/admin/*` paths are the form handlers and record pages the hub links to — `/admin/people`, `/admin/person/{id}`,
`/admin/analytics`.

Navigating to a `/app/*` path while signed out returns `303` to `/auth/login?return_to=/app/admin`, so **one URL drives
the whole flow**: go to `/app/admin` and log in when asked.

## Credentials

The KIND Rauthy fixture ([`local-fixture.yaml`](../../../k8s/overlays/kind/rauthy/local-fixture.yaml)) seeds five role
accounts, all with password `password`:

| Email | Role | Person |
| --- | --- | --- |
| `owner@neonlaw.com` | owner | Olive Owner |
| `admin@neonlaw.com` | admin | Ada Admin |
| `lawyer@neonlaw.com` | lawyer | Lawrence Lawyer |
| `clerk@neonlaw.com` | clerk | Clara Clerk |
| `client@neonlaw.com` | client | Cleo Client |

This skill uses `admin@neonlaw.com` / `password`.

These are committed, loopback-only development fixtures in a public repository — they are test data, not credentials.
**The boundary is absolute: never type a password into a staging, production, or any non-localhost origin, and never
handle a real credential.** If the target host is not `localhost`, stop and hand the login to the user.

Authentication is OIDC; authorization is the database `persons.role`. Signing in does not create a Person — the `web`
boot seed creates Ada Admin, so the seed must have run against the same store `web` reads.

## The loop

### 1. Preconditions

`docker info` must succeed. If it fails, `open -a OrbStack` and wait — roughly 10–15 seconds.

⚠️ **`kubectl`'s current context is very often GKE production.** Never run a bare `kubectl` against this stack. Every
cluster command in this skill exports the worktree's own kubeconfig first:

```bash
export KUBECONFIG="$PWD/.devx/kubeconfig"
```

### 2. Bring up the dependency tier

Use the shared `dev up` tier, not `worktree-env`: it binds `web` to the fixed `:3001` and Rauthy to `:30080`, which is
what makes this skill's URLs stable. `dev up` reuses an existing `navigator` KIND cluster.

```bash
cargo run -p cli -- dev up
```

First run on a cold cache builds `workflows-service` in Docker and takes 10+ minutes. Build `neon` in parallel while
waiting — the two do not contend for the same target directory.

### 3. Start `web`

```bash
set -a; source .devx/env; set +a
cargo run -p neon
```

`neon` is the single binary for the consolidated site. Wait for health rather than guessing:

```bash
until curl -sf http://localhost:3001/health >/dev/null; do sleep 3; done
```

Boot applies the schema and runs the dev seed (`seed applied … 15 persons`) — that is what creates Ada Admin.

### 4. Drive Chrome and sign in

Use **Claude Code for Chrome** (`mcp__claude-in-chrome__*`), the user's real browser — not the in-app browser — whenever
the request is "open it for me".

1. `tabs_context_mcp{createIfEmpty:true}`, then `navigate` to `http://localhost:3001/app/admin`.
2. Rauthy's login form is **two-step**: it renders the email field alone. Fill it, submit, and only then does the
   password field appear. Re-read the page after the first submit — the password field gets a *new* ref.
3. Fill the password, submit.
4. Screenshot to confirm the hub: title `Neon Law | Admin`, an `Admin` heading, and three tiles — People, Visitor
   analytics, Matters.

## The two failures that always bite

Both come from a KIND cluster that has been up since a previous day. Rauthy's `data` volume is an `emptyDir`: it
survives *container* restarts inside the pod, so a pod that predates the current `users.json` keeps serving the old
bootstrap and never re-reads the fixture.

### "Invalid credentials" on a password you know is right

The user does not exist in that Rauthy's database. Confirm before acting — the log says so plainly:

```bash
export KUBECONFIG="$PWD/.devx/kubeconfig"
kubectl --namespace navigator logs -l app=rauthy --tail=40 | grep -i 'authorize\|bootstrap'
```

`POST /authorize Error: … NotFound, message: "no rows returned"` is the signature. Recreate the pod so it gets a fresh
`emptyDir` and re-bootstraps from the current fixture:

```bash
kubectl --namespace navigator delete pod -l app=rauthy
until kubectl --namespace navigator get pods -l app=rauthy --no-headers | grep -q '1/1'; do sleep 4; done
```

A healthy re-bootstrap logs `Initializing empty production database` and `Migrated 1 clients.`.

### "id_token verification failed" immediately after

Recreating Rauthy regenerates its JWKS (`Generating new JWKs`), and the running `web` still holds the old keys. The
login itself succeeded — you land on `/auth/callback?code=…` — but signature verification fails against the stale key
set. **Restart `web`**; nothing in the cluster needs touching:

```bash
pkill -f 'target/debug/neon'
set -a; source .devx/env; set +a
cargo run -p neon
```

Then navigate to `/app/admin` again. Rauthy's own SSO session is still live, so this re-authenticates silently — no
second password entry.

Order matters: recreate Rauthy **first**, restart `web` **second**. Restarting `web` before Rauthy is stable just
re-caches keys that are about to change again.

## Proving it actually worked

Compilation and a rendered page are not proof of *role*. Two checks:

- `/app/admin` renders at all. Its `admin_landing_view` server function calls `require_admin()`, which commits a real
  `403` for any non-admin, so the hub cannot render for the wrong tier.
- The audit log records the verified token, identifiers only and never client content, by design. Grep the `web` log
  for `oidc.id_token.verified`.

`/admin/people` is the strongest visual confirmation: it lists every person with Edit / Delete / Impersonate controls,
which only an admin sees.

## Cleanup

Leave the shared `dev up` tier running between sessions — it is the ordinary local stack. Close any Chrome tab this
skill opened unless the user wants it kept. Do **not** run `dev down` (it deletes the cluster) or prune Docker volumes
without explicit approval.
