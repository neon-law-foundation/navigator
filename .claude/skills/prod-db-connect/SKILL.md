---
name: prod-db-connect
description: Connect to the production Cloud SQL (Postgres) database for ad-hoc inspection or a targeted data fix, via cloud-sql-proxy with IAM service-account impersonation. Trigger on "connect to prod db", "check or fix a value in prod", "query the production database", "update a prod row", or when a code/seed change must reach already-existing prod rows (the canonical seed is idempotent and will NOT update them). Project, instance, SA, and secret names are discovered from .env, gcloud, and Secret Manager — nothing hard-coded. This is PRODUCTION: read first, log and get approval for every write, then revoke the temporary IAM grant.
version: 0.1.0
---

# Connect to the production database

Prod is **Cloud SQL for Postgres**, reached over **cloud-sql-proxy** with **IAM auth** (no password). The pods connect
as their Workload-Identity SAs; you do the same by impersonating the **owning** SA that holds the tables.

> PRODUCTION, machine-bound. **Before ANY write (INSERT/UPDATE/DELETE/DDL): write the exact SQL to a timestamped
> file under `/tmp/navigator-prod-sql/`, show the user its path + contents, and wait for explicit approval.** A
> general "fix prod" ask is not standing approval for a specific statement — re-confirm each time. `SELECT`s are
> exempt. Scope every write with a guard on the old value, wrap in a transaction, verify, then clean up (Step 4).

## Gotchas that bite first

- **Tables are owned by the web SA, not you.** As a human IAM user (even project Owner) every app table reads as
  `relation "<t>" does not exist` and `information_schema` looks empty — you lack `public` schema `USAGE`. Only
  `pg_tables` shows them. Fix: connect *as* the owning SA via `--impersonate-service-account`.
- **The canonical seed is idempotent** (`store/src/seed.rs`): re-seeding only inserts missing rows, never updates
  existing ones. Changing a live value means a targeted `UPDATE`.
- **Raw SQL skips `updated_at`** (set by SeaORM, not a trigger) — set `updated_at = now()` yourself.
- **Data-only fix needs no power-push** — pods read the table live. Redeploy only when *code* changed.

## Recipe

```bash
# 1. Discover (env + gcloud; nothing hard-coded)
PROJECT=$(grep -E '^NAVIGATOR_GCP_PROJECT_ID=' .env 2>/dev/null | cut -d= -f2)
PROJECT=${PROJECT:-$(gcloud config get-value project)}
INSTANCE=$(gcloud sql instances list --project "$PROJECT" --format='value(name)' | head -1)
CONN=$(gcloud sql instances describe "$INSTANCE" --project "$PROJECT" --format='value(connectionName)')
OWNER_SA=$(gcloud sql users list --instance "$INSTANCE" --project "$PROJECT" \
            --filter='type=CLOUD_IAM_SERVICE_ACCOUNT' --format='value(name)' | grep -i web)  # web-sa@<project>.iam
SA_EMAIL="${OWNER_SA}.gserviceaccount.com"; ME=$(gcloud config get-value account)

# 2. Grant yourself impersonation on the web SA (TEMPORARY — revoked in step 5)
gcloud iam service-accounts add-iam-policy-binding "$SA_EMAIL" \
  --member="user:$ME" --role="roles/iam.serviceAccountTokenCreator"
until gcloud auth print-access-token --impersonate-service-account="$SA_EMAIL" >/dev/null 2>&1; do sleep 10; done

# 3. Proxy as the owning SA, then psql with the SA username + empty password
cloud-sql-proxy --auto-iam-authn --impersonate-service-account="$SA_EMAIL" --port 5433 "$CONN" &
PGPASSWORD="" psql "host=127.0.0.1 port=5433 user=${OWNER_SA} dbname=navigator sslmode=disable"

# 4. SELECT freely. For a write: log SQL to /tmp, get approval, THEN run with a guard + transaction:
#    psql "..." -v ON_ERROR_STOP=1 -f /tmp/navigator-prod-sql/<utc>.sql

# 5. Clean up — always
pkill -f "cloud-sql-proxy.*$INSTANCE"
gcloud iam service-accounts remove-iam-policy-binding "$SA_EMAIL" \
  --member="user:$ME" --role="roles/iam.serviceAccountTokenCreator"
```

Prefer an app seam (migration, CLI, admin route) over raw SQL when one exists. Never set a built-in user's password as a
shortcut — impersonation leaves no standing secret. Keep the proxy off the KIND port (`15432`).
