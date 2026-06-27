---
name: postgres-in-kind
description: >
  Running Postgres inside the KIND cluster — manifest layout, connection URL, secrets, migrations, host access. Trigger
  when editing `k8s/postgres/`, choosing a Postgres version, opening `psql` against the in-cluster DB, debugging
  connection errors from a host-side `cargo run -p web`, or before deciding to swap Postgres for another database (don't
  — production is also Cloud SQL for Postgres).
---

# Postgres in the KIND cluster

`k8s/postgres/` ships a single-replica Postgres Deployment with an `emptyDir` volume. It is a **dev-only** stand-in for Cloud SQL for Postgres in production — the connection URL is the only configuration difference between dev and prod, and that's deliberate (see the workspace `CLAUDE.md` → "Cloud: GCP only").

## Why a Deployment + emptyDir, not a StatefulSet + PVC

- `emptyDir` is wiped on pod restart. That's a feature here: every developer starts from the same blank state.
- Persistence in dev would invite "works on my machine" drift between developers.
- For production-shape persistence, run Postgres outside the cluster (Cloud SQL) and point `DATABASE_URL` at it. We don't operate a stateful Postgres in-cluster — KIND is for app iteration, not data engineering.

If you genuinely need persistence across `cargo run --release -p cli -- kind-down`, run `pg_dump` from the host (see "Talking to it from the host" below) and check the dump into `store/seeds/`.

## Connection URL

In-cluster pods reach Postgres at `postgres.navigator.svc.cluster.local:5432` (the Service DNS in the `navigator` namespace). The host reaches it via `kubectl port-forward` or via the `.devx/env` file `cargo run --release -p cli -- start-dev-server` writes (port-forwarded to `127.0.0.1:15432` to avoid colliding with a host-side Postgres install on the standard `5432`):

```
DATABASE_URL=postgres://navigator:navigator@127.0.0.1:15432/navigator
```

`DATABASE_URL` is the only thing `store::DbConfig::from_env` reads — Postgres is the only supported backend (the SQLite fallback was removed in the cutover). See [[rust-sea-orm]] for the selector.

This one server hosts more than the `navigator` database: `cargo run -p cli -- worktree-env up` creates a per-worktree `navigator_<slug>` database on the **same** `:15432` server (created + migrated via `store`), so parallel worktrees stay isolated without a second Postgres. `worktree-env down` runs `DROP DATABASE … WITH (FORCE)`. See [[kind-local-dev]] and `docs/RUNBOOK.md` §7c.

## Secrets

- Dev credentials (`navigator` / `navigator`) live in `k8s/postgres/postgres.yaml` as a plain Secret. **This is fine because the cluster is local-only**; never copy this pattern into a manifest that targets a real cluster.
- Production credentials come from GCP Secret Manager via the Cloud SQL Auth Proxy or workload identity — they never appear in a YAML in this repo.

## Migrations

Migrations run on `web` boot via `sea-orm-migration` (`Migrator::up(&db, None).await?`). There is no separate migration Job in KIND — the web Deployment's startup probe waits until migrations finish, then opens the readiness gate.

When iterating on a new migration:

```bash
cargo run --release -p cli -- start-dev-server   # bring up postgres (no web)
cargo run -p web                    # binds :3001, runs migrations on startup
# edit migration, save
cargo run -p web                    # runs the new migration against the same DB
```

To start fresh: `kubectl --context kind-navigator -n navigator rollout restart deploy/postgres` (the emptyDir is wiped) then `cargo run -p web` again.

## Talking to it from the host

```bash
# Port-forward + psql
kubectl --context kind-navigator -n navigator port-forward svc/postgres 5432:5432 &
psql "postgres://navigator:navigator@127.0.0.1:5432/navigator"

# One-shot query
PGPASSWORD=navigator psql -h 127.0.0.1 -U navigator -d navigator -c '\dt'

# Dump for a fixture
PGPASSWORD=navigator pg_dump -h 127.0.0.1 -U navigator -d navigator -F p > store/seeds/dump.sql
```

## Health

The Pod's readiness probe is `pg_isready -U navigator -d navigator`. If it stays Not-Ready, `kubectl logs pod/postgres-…` is the first stop — usually the volume mounted in the wrong place or an init script syntax error.

## Anti-patterns

- Hardcoding `postgres://localhost:5432/…` in Rust. Read `DATABASE_URL` from env; the rest is plumbing.
- Adding a `StatefulSet` for dev — see "Why a Deployment + emptyDir" above.
- Running `psql` inside the pod (`kubectl exec`) for "quick checks" — fine for one query, painful for anything iterative. Port-forward and use a real psql session.
- Assuming `cargo test` always needs Docker. By default `store::test_support` spawns one reuse-labeled container shared by the whole run, but setting `TEST_DATABASE_URL` (e.g. at this KIND Postgres, `localhost:15432`) makes tests connect there and skip Docker entirely. See `docs/test-database.md`.

## Canonical sources

- Postgres documentation: <https://www.postgresql.org/docs/current/>
- Postgres on Docker Hub (image tags + env vars): <https://hub.docker.com/_/postgres>
- Postgres in Kubernetes (CNCF blog): <https://www.cncf.io/blog/>
- CloudNativePG (the operator if you ever need real persistence in-cluster): <https://cloudnative-pg.io/> · <https://github.com/cloudnative-pg/cloudnative-pg>
- Cloud SQL for Postgres (production target): <https://cloud.google.com/sql/docs/postgres>
- sqlx Postgres driver notes: <https://docs.rs/sqlx>
