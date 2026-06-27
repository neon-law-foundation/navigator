# Secrets management — Doppler

**The rule:** if Doppler is configured for this checkout, use it; otherwise fall back to `.env`.

```text
doppler configured?  ── yes ─→  doppler run -- cargo run -p web   (values injected from the `dev` config)
                     └─ no  ─→  cp .env.example .env; fill it in; cargo run -p web   (dotenvy loads .env)
```

Doppler holds the **values**; [`.env.example`](../.env.example) is the committed **contract** (every name + annotation).
Doppler is NeonLaw's operational layer *above* the env-var interface, never a code dependency — the workspace builds,
tests, and runs with no Doppler account, so OSS forks can ignore this whole page and use `.env`.

## Project and config layout

One Doppler project, `navigator`, in the **Neon Law** workplace.

| Config | Environment | Holds |
| --- | --- | --- |
| `dev` | `dev` | Shared local-dev + test secrets: third-party **sandbox** creds, GCP infra IDs |
| `dev_personal` | `dev` | Per-user branch overlay on `dev` (Doppler primitive; cannot be deleted) |
| `prd` | `prd` | Production secret values + the same GCP infra IDs |

`dev_personal` is private to your Doppler user, inherits from `dev`, and nothing references it — treat `dev` as the team
config. (The CLI refuses to delete a personal config, which is why it lingers.)

## Local development

A fresh checkout links once, then runs everything under `doppler run`:

```bash
doppler login                                    # browser auth into the Neon Law workplace
doppler setup --project navigator --config dev   # links this directory to navigator/dev
cargo run -p cli -- start-dev-server                          # writes .devx/env (KIND cluster deps)
doppler run -- cargo run -p web                  # dev secrets injected; .devx/env fills the cluster wiring
```

Local config arrives in three layers, first writer wins under `dotenvy`:

1. **`doppler run`** injects the `dev` config as real env vars — third-party sandbox creds (`DOCUSIGN_*`,
   `SENDGRID_EVENTS_*`) and GCP infra IDs (`NAVIGATOR_GCP_PROJECT_ID`, …). Highest precedence.
2. **`.devx/env`** (from `devx up`) supplies the ephemeral cluster wiring: `DATABASE_URL`, the Keycloak `OAUTH_*`,
   storage endpoints, `RESTATE_BROKER_URL`, `NAVIGATOR_OPA_URL`, host port-forwards.
3. **`.env`** (gitignored) is the fallback when not using `doppler run`. Regenerate it for tools that need a literal
   file: `doppler secrets download --no-file --format env --project navigator --config dev > .env`.

`cargo test` needs no secrets: it spins its own Postgres via `testcontainers` and routes every unconfigured vendor to
its in-process stub. Only the gated sandbox smoke tests read live creds (`DOCUSIGN_SANDBOX_*`, `XERO_SANDBOX_*`) and
self-skip when absent; run them with `doppler run -- cargo test …`.

The machine-bound deploy is no different: prefix the deploy commands in [`cloud-operations.md`](cloud-operations.md)
with `doppler run --` instead of sourcing `.env`, and the deploy-targeting vars (`NAVIGATOR_GCP_PROJECT_ID`,
`NAVIGATOR_GCP_LOCATION`, `NAVIGATOR_GKE_CLUSTER_NAME`, `NAVIGATOR_PRIMARY_DOMAIN`) are injected from the `dev` config —
e.g. `doppler run -- bash -c 'cargo run --release -p cli -- deploy'`. With Doppler configured, `.env` is optional and
fully derivable (`doppler secrets download …` above); the OSS docs keep a `source .env` path for forks that don't use
Doppler.

## Production (GCP)

Production never talks to Doppler at runtime:

```text
Doppler prd  ──render──>  GCP Secret Manager  ──CSI driver──>  navigator-web-secrets Secret  ──envFrom──>  pods
```

`prd` is the editable source of truth. A render step pushes each value into Secret Manager as a new `versions/latest`,
and the GKE Secret Manager CSI driver
([`secret-provider-class.yaml`](../examples/deploy/k8s/gke/secrets/secret-provider-class.yaml)) projects them into the
`navigator-web-secrets` Secret that `web` and `workflows-service` read via `envFrom`.

The render targets exactly these keys — keep this set **in lockstep** with `secret-provider-class.yaml`:

```text
DATABASE_URL            SESSION_SECRET          OAUTH_CLIENT_SECRET
RESTATE_BROKER_URL      RESTATE_AUTH_TOKEN      SENDGRID_API_KEY
SENDGRID_INBOUND_SECRET SENDGRID_EVENTS_SECRET  DOCUSIGN_ACCESS_TOKEN
DOCUSIGN_HMAC_KEY       XERO_TENANT_ID          XERO_CLIENT_ID
XERO_CLIENT_SECRET
```

The `NAVIGATOR_*` infra IDs in `prd` are **not** secrets — they reach the pods through the `web-env.yaml` ConfigMap, not
Secret Manager. Keep them in `prd` so the whole prod env lives in one place; the render below skips them.

### Render + rotate (run on your machine — `gcloud` is machine-bound)

Values transit a shell pipe, never the chat. To rotate, set the new value in `prd` first, then re-run this:

```bash
# NAVIGATOR_GCP_PROJECT_ID comes from Doppler (or `source .env` on an OSS fork that isn't using Doppler):
NAVIGATOR_GCP_PROJECT_ID="$(doppler secrets get NAVIGATOR_GCP_PROJECT_ID --plain --project navigator --config dev)"
PROD_KEYS="DATABASE_URL SESSION_SECRET OAUTH_CLIENT_SECRET RESTATE_BROKER_URL \
RESTATE_AUTH_TOKEN SENDGRID_API_KEY SENDGRID_INBOUND_SECRET SENDGRID_EVENTS_SECRET \
DOCUSIGN_ACCESS_TOKEN DOCUSIGN_HMAC_KEY XERO_TENANT_ID XERO_CLIENT_ID XERO_CLIENT_SECRET"
for key in $PROD_KEYS; do
  val="$(doppler secrets get "$key" --plain --project navigator --config prd 2>/dev/null)" || continue
  [ -z "$val" ] && { echo "skip $key (empty in prd)"; continue; }
  printf %s "$val" | gcloud secrets versions add "$key" \
    --project="$NAVIGATOR_GCP_PROJECT_ID" --data-file=- 2>/dev/null \
  || printf %s "$val" | gcloud secrets create "$key" \
    --project="$NAVIGATOR_GCP_PROJECT_ID" --data-file=-
done
```

Then `kubectl rollout restart` the `navigator-web` + `workflows-service` Deployments so they re-read the Secret — see
the [`cloud-operations.md`](cloud-operations.md) no-rebuild restart path. (A `navigator secrets render-prod` subcommand
could replace this shell loop later; the loop is the source of truth until then.)

## Adding a new secret

1. **Code + contract** — the binary reads it; document it in `.env.example` with the right annotations.
2. **Doppler** — add it to `dev` (and `prd` if prod needs it).
3. **Prod plumbing** — if it's a secret required in prod, add it to `secret-provider-class.yaml` (both blocks) and to
   `PROD_KEYS` above, then render. If it's a non-secret, add it to the `web-env.yaml` ConfigMap instead.
4. **Boot invariant** — if the binary hard-requires it in prod (`web::config::enforce_prod_invariants`), the prod Secret
   must carry it *before* the new image rolls or the pod crash-loops. `power-push` step 7b checks exactly this.

## One-time `prd` backfill from the live cluster

`prd` currently holds only the five shared `NAVIGATOR_*` infra IDs. The production secret *values* still live in the
hand-maintained `navigator-web-secrets` Secret. Import them once (on your machine, where `kubectl` and the values are),
after which Doppler `prd` → render is the forward path and the hand-maintained Secret can be retired:

```bash
kubectl -n navigator get secret navigator-web-secrets -o json \
  | jq -r '.data | to_entries[] | "\(.key)=\(.value|@base64d)"' > /tmp/prd.env
doppler secrets upload /tmp/prd.env --project navigator --config prd && rm -f /tmp/prd.env
```
