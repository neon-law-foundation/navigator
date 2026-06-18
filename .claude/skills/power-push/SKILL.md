---
name: power-push
description: >
  One-shot "ship to prod" workflow — commit pending changes, then build, push, and roll out BOTH the navigator-web and
  workflows-service images together at HEAD's short SHA (always both, never one), archiving a single git bundle of HEAD
  into the GCS source bucket and reclaiming the local images afterward. Includes a pre-deploy check that the prod Secret
  carries every key the new binary's boot invariants require (a missing one — e.g. DOCUSIGN_HMAC_KEY — crash-loops the
  pod). Also covers the no-image-rebuild "push" — `kubectl rollout restart` after rotating values in the K8s Secret.
  Trigger when the user says "power-push", "ship this", "push the bundle", "deploy this", "upload bundle to gcp" after a
  commit-worthy change, OR when they've rotated a key/value in the K8s Secret and need the running pods to pick it up.
  Every project / region / domain / cluster name is read from `.env` (see [`.env.example`](../../../.env.example)) —
  nothing is hard-coded, so forks ship to their own GCP project without editing this skill.
---

# power-push

Ship a code change all the way to prod: commit → build both images → registry → GCS source bundle → secret-invariant
check → roll out both deployments together → reclaim disk. One workflow, in one session, in this order — each later step
assumes the prior step's artifact already exists. **Always ship both `navigator-web` and `workflows-service`** at one
SHA; never just one.

Also: the **no-rebuild push** for secret rotation — see the last section. Both flows count as "powering a push to prod."

## The fast path: `navigator power-push`

This whole workflow is now a deterministic subcommand of the `navigator` CLI —
[`cli/src/devx/power_push.rs`](../../../cli/src/devx/power_push.rs). Prefer it over pasting the shell blocks by hand: it
runs every step below in order, reads all config from the environment, and never commits for you (the image tag is
HEAD's short SHA, so commit first).

**Always run power-push under the Doppler `prd` config.** This is a production ship, and `prd` carries the production
values the deploy reads — `NAVIGATOR_PRIMARY_DOMAIN` (the smoke-check host), the production DocuSign keys, and the
production Restate wiring. The `dev` config is for local development and tests; shipping under it points the smoke check
and re-register step at the wrong place. This workspace is Doppler-only — there is no `.env` on disk — so inject the
config with `doppler run --project navigator --config prd --`. The `--project navigator` flag is required: this repo
carries no project-scoped Doppler config, so a bare `doppler run --config prd` cannot resolve which project to read and
errors out.

```bash
# Full ship: verify → build both → push both → bundle → secret check →
# roll out both → re-register → reclaim.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push
# Print every command, run nothing.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --dry-run
# No-rebuild push: restart both deployments after rotating a Secret value.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --restart-only
# Skip the gates — only at a SHA already verified this session.
doppler run --project navigator --config prd -- cargo run --release -p cli -- power-push --skip-verify
```

It pins `kubectl` to `${NAVIGATOR_GKE_CONTEXT}` (default `gke_<project>_<location>_<cluster>-prod`) so a stale current
context can't misdirect the ship, and the step-7b invariant check reads `web/src/config.rs` directly — so it never
drifts from the binary's real boot requirements and aborts with the exact `kubectl patch` to run if a key is missing.

The rest of this document is the **rationale and the manual fallback**: what each step does and why it's ordered this
way. Read it to understand or debug the command; run the command to ship. The archives variant and the deeper drift
diagnostics below are still operator-driven — `navigator power-push` covers the two main flows (full build + no-rebuild
restart), not those one-off shapes.

## When to invoke

- The user has a code change ready to ship and says any of:
  - "power-push", "ship this", "push the bundle"
  - "upload [bundle|image] to gcp"
  - "build and push"
- A diff is staged or recently committed and the user wants it in the registry + archived as a bundle in GCS.
- The user just changed a value in the K8s Secret (SendGrid key rotation, OIDC secret rotation, Restate token rotation,
  etc.) and asks why the change hasn't taken effect → jump to **The no-rebuild push** below.

## When NOT to invoke

- Work-in-progress changes. The image tag = HEAD short SHA, so the registry artifact gets a real commit name. Don't
  pollute the registry with throwaway tags.
- Pre-commit hooks failing. Fix the hook failure first; don't bypass.

## Configuration comes from Doppler `prd` — nothing is hard-coded

Every project / region / domain / cluster value flows through env vars — there is **no** literal GCP project ID, domain,
region, registry path, or bucket name baked into this file. This workspace is Doppler-only (no `.env` on disk); inject
the production values by running each command in this skill under `doppler run --project navigator --config prd --`. A
fork pointed at a different cloud account ships by setting the same vars in its own secret store, not by editing this
skill.

```bash
# Prefix the navigator command (and any manual shell block below) with this:
doppler run --project navigator --config prd -- <command>
```

**If a `.env` ever exists on disk it must stay gitignored.** The repo's `.gitignore` lists both `.env` and `.env.*`
(confirm with `grep -n '^\.env' .gitignore`). This workspace is Doppler-only, so there is normally no `.env` on disk to
leak, but the rule still holds for any fork that materializes one. **Never** `git add .env`, never commit it, never
paste its contents into a chat, never copy it into any tracked file; if `.env` ever appears in `git status` as a staged
or tracked change, stop and back it out — that is a credential leak in progress. The OSS-publishable template is
[`.env.example`](../../../.env.example); real secrets only ever live in `.env` (local) and the K8s Secret (prod).

| Variable | Meaning | Example |
| --- | --- | --- |
| `NAVIGATOR_GCP_PROJECT_ID` | target GCP project | `my-org-prod` |
| `NAVIGATOR_GCP_LOCATION` | region for AR + bucket + cluster | `us-west4` |
| `NAVIGATOR_GKE_CLUSTER_NAME` | cluster name (also AR repo name) | `navigator` (default) |
| `NAVIGATOR_PRIMARY_DOMAIN` | public hostname for the smoke-check curl | `example.com` |
| `NAVIGATOR_K8S_NAMESPACE` | K8s namespace for the Deployments | `navigator` (default) |
| `NAVIGATOR_GKE_OVERLAY_DIR` | private kustomize overlay path (substituted); enables §7a | `~/work/nav-overlay` |

`NAVIGATOR_K8S_NAMESPACE` is used below as `${NAVIGATOR_K8S_NAMESPACE:-navigator}` so the default applies when unset.

`NAVIGATOR_GKE_OVERLAY_DIR` is **optional but strongly recommended.**

Without it, step 7 falls back to a bare `kubectl set image`, which patches only the image field.

Any non-image change in your overlay (env var, volume mount, sidecar, resource bump) silently fails to reach prod until
something else triggers a full apply.

Forks on GitOps (Config Sync, Argo CD, Flux) can leave it unset — their controller reconciles the overlay continuously.
Forks running `kubectl apply` from a laptop should set it.

These are documented in [`.env.example`](../../../.env.example). If any required one is unset when the skill runs, fail
fast — don't substitute project-internal defaults.

```bash
: "${NAVIGATOR_GCP_PROJECT_ID:?set in .env}"
: "${NAVIGATOR_GCP_LOCATION:?set in .env}"
: "${NAVIGATOR_GKE_CLUSTER_NAME:?set in .env}"
: "${NAVIGATOR_PRIMARY_DOMAIN:?set in .env}"
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
OVERLAY="${NAVIGATOR_GKE_OVERLAY_DIR:-}"   # optional; see §7
```

### Derived names (convention, not configuration)

The workspace conventions (per [`docs/oss-install.md`](../../../docs/oss-install.md) and
[`.env.example`](../../../.env.example)) derive these from the project ID — change them only if you've intentionally
renamed the underlying resources:

| Derived name | Formula |
| --- | --- |
| Registry path | `${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}` |
| Source bucket | `gs://${NAVIGATOR_GCP_PROJECT_ID}-source/` |
| Cluster context | `gke_${NAVIGATOR_GCP_PROJECT_ID}_${NAVIGATOR_GCP_LOCATION}_${NAVIGATOR_GKE_CLUSTER_NAME}-prod` |

If your fork uses different names, override them with locals at the top of your shell session — do **not** edit this
skill.

## The full-build recipe

Run these in order. Each is one or two lines. **`.env` must be sourced first** (see the preceding section).

### 1. Pre-flight

```bash
git status                                                            # uncommitted work?
docker ps >/dev/null                                                  # daemon up?
gcloud storage ls "gs://${NAVIGATOR_GCP_PROJECT_ID}-source/" \
  --project="${NAVIGATOR_GCP_PROJECT_ID}" >/dev/null                  # ADC + bucket reachable?
kubectl config current-context                                        # on the right cluster?
```

If `gcloud storage ls` 403s, the operator's ADC needs `roles/storage.objectUser` on
`gs://${NAVIGATOR_GCP_PROJECT_ID}-source` — the same GSA used for `roles/artifactregistry.writer` on the GAR repo can
carry both.

### 2. Commit any pending change

If there's a real diff worth shipping and it isn't committed, commit it. Otherwise skip to step 3. Commit message style
and co-author trailer per `CLAUDE.md`:

```bash
git add <files>
git commit -m "$(cat <<'EOF'
<scope>: <one-line summary>

<body — the why, not the what>
EOF
)"
```

Author identity is set via your local `git config user.name` / `user.email` (don't hard-code identities in this skill).

### 3. Verify before shipping

Four checks must all exit 0 before the image is built — fmt, clippy, tests, and the **workspace-wide** markdown lint.
Run them in order; never skip the markdown step on the grounds that "only docs changed" or "I didn't touch any `.md`."
The lint runs over the whole workspace, the same command CI runs, so a regression anywhere in the tree blocks the ship.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Strict markdown lint — workspace-wide, same command as .github/workflows/ci.yml.
# This must exit 0. No per-file scoping, no excludes (`prompts/` is gitignored so CI
# doesn't see it; locally it's fine if the lint flags prompts because those files
# never reach the registry image or the prod cluster).
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes .
```

A failing test means the image you're about to ship is broken — stop and fix. A failing markdown lint means the doc
update is going out half-baked; the docs are part of the artifact, so fix the lint or revert the doc change before any
ship. Do not `git commit --no-verify`, do not bypass the step, and do not push the image while the lint is red.

### 4. Build BOTH images

**Always ship both binaries together.** `navigator-web` and `workflows-service` share the same workspace and the same
Secret; shipping them at one SHA keeps the public surface and the durable-execution worker in lockstep and avoids
version skew (new `web` submitting a step an old worker can't run). Build both — even if a given diff only touched one
crate, the cost is one extra cached `cargo` build and the rollback story stays simple (both deployments trace to one
bundle at one SHA).

```bash
cargo run --release -p cli -- image                       # navigator-web:dev
cargo run --release -p cli -- image-workflows-service     # navigator-workflows-service:dev
```

Tags are `navigator-web:dev` and `navigator-workflows-service:dev` locally; we retag on push. (The `archives`,
billing-canary, and `statutes` flows are compiled into `workflows-service`; each also has a thin trigger image —
`navigator-archives-trigger`, `navigator-billing-canary-trigger`, `navigator-statutes-trigger` — that a CronJob runs.
Ship a trigger image too only when its own crate or its CronJob changed — see "The archives variant" for the pattern.
The `navigator-git` and `navigator-redirect` images are independent standalone services, not part of this two-service
ship.)

### 5. Push BOTH to Artifact Registry

```bash
SHA=$(git rev-parse --short HEAD)
REPO="${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}"

docker tag  navigator-web:dev               "${REPO}/navigator-web:${SHA}"
docker tag  navigator-workflows-service:dev "${REPO}/navigator-workflows-service:${SHA}"
docker push "${REPO}/navigator-web:${SHA}"
docker push "${REPO}/navigator-workflows-service:${SHA}"
```

If push 401s, run `gcloud auth configure-docker "${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev"`.

### 6. Archive a git bundle to the GCS source bucket

```bash
SHA=$(git rev-parse --short HEAD)
BUNDLE="/tmp/navigator-${SHA}.bundle"

git bundle create "${BUNDLE}" --all
gcloud storage cp "${BUNDLE}" \
  "gs://${NAVIGATOR_GCP_PROJECT_ID}-source/navigator-${SHA}.bundle" \
  --project="${NAVIGATOR_GCP_PROJECT_ID}"
rm "${BUNDLE}"
```

`--all` includes every ref (branches + tags) so the bundle is a full restore point — `git clone <bundle>` works on any
machine with `gcloud` + a clean directory. Naming is `navigator-<short-sha>.bundle`; the bucket is the source-of-truth
distribution channel for this repo.

### 7. Deploy to the prod cluster

This is **four sub-steps**: sync the manifest (7a), confirm the Secret satisfies the new binary's boot invariants (7b),
bump both images and roll out together (7c), then re-register the worker with Restate so any new service is reachable
(7d).

`kubectl set image` only patches the image field. Any other change in your overlay (env var, volume mount, sidecar,
resource bump) is invisible to it. Skip 7a and the cluster keeps running on the non-image fields from the last full
apply, even though your overlay in git says otherwise.

We hit this concretely once. `NAVIGATOR_EMAIL_BACKEND=sendgrid` was added to `patches/web-env.yaml` and merged. But
every subsequent `power-push` only ran `set image`, so the prod pod kept booting without the var. Outbound email
silently fell back to the in-memory `CapturingEmail` backend; the audit table wrote `outcome="sent"`; SendGrid saw zero
requests for the day. See "Detecting manifest drift" below for the diagnostic.

#### 7a. Sync the manifest (skip only on GitOps)

If `NAVIGATOR_GKE_OVERLAY_DIR` is set, dry-run a diff first to surface drift, then apply:

```bash
if [[ -n "${OVERLAY}" ]]; then
  kubectl diff -k "${OVERLAY}" || true
  kubectl apply -k "${OVERLAY}"
fi
```

`kubectl diff` exits 1 when a diff exists — that's the signal, not a failure; the `|| true` keeps the script going.

If you're on Config Sync / Argo / Flux, your controller reconciles the overlay continuously — skip 7a and trust the
controller.

If you're applying from a laptop and **didn't** set `NAVIGATOR_GKE_OVERLAY_DIR`, accept that this push is image-only.
Any new non-image overlay fields won't reach prod until someone runs `kubectl apply -k` by hand.

#### 7b. Confirm the prod Secret satisfies the new binary's invariants

**Do this before bumping the image, every time.** `web::config::enforce_prod_invariants` runs at boot and **crash-loops
the pod** if a required key is missing — there is no `APP_ENV` escape hatch. When a commit adds a new required secret
(it lives in `web/src/config.rs`), the prod Secret must gain that key *before* the new image rolls, or the new pod
`CrashLoopBackOff`s while the old pod keeps serving (no outage, but the rollout silently never completes).

We hit this shipping the e-signature loop: the new binary required `DOCUSIGN_HMAC_KEY` (without it the
`/webhook/esignature` endpoint would skip HMAC verification and anyone could forge a `completed` callback). The prod
Secret didn't have it; the new pod crash-looped until the key was added.

Diff the keys the new binary requires against the keys the live Secret carries:

```bash
SECRET_NAME="${NAVIGATOR_WEB_SECRET_NAME:-navigator-web-secrets}"
# Required keys, scraped straight from the invariant source so this never drifts:
grep -oE '"[A-Z_]+ must be set' web/src/config.rs | grep -oE '[A-Z_]+' | sort -u > /tmp/required-keys.txt
kubectl -n "${NS}" get secret "${SECRET_NAME}" -o json \
  | jq -r '.data | keys[]' | sort > /tmp/live-keys.txt
# Names required but absent from the Secret = will crash-loop the new pod:
comm -23 /tmp/required-keys.txt /tmp/live-keys.txt
```

If that prints anything, add the missing key to the Secret first — its value never transits the chat:

```bash
KEY=$(openssl rand -hex 32)   # or the real shared secret if one already exists upstream
kubectl -n "${NS}" patch secret "${SECRET_NAME}" --type=merge \
  -p "{\"stringData\":{\"DOCUSIGN_HMAC_KEY\":\"${KEY}\"}}"
unset KEY
```

A freshly generated key satisfies the invariant and is safe: the webhook *rejects* unverified callbacks until the
upstream (e.g. DocuSign Connect) is configured with the same value. Use the real upstream value instead only if the
integration is already live. (Some invariant keys — `NAVIGATOR_OPA_URL`, `NAVIGATOR_STORAGE_BACKEND` — are deployment
env, not Secret keys; if `comm` flags one of those, the fix is step 7a, not a Secret patch.)

#### 7c. Bump BOTH images and roll out at the same time

**Always roll out both deployments together.** They envFrom the same Secret and move as a unit.

```bash
SHA=$(git rev-parse --short HEAD)
REPO="${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}"

# Fire both image bumps back-to-back, THEN wait on both — so the two rollouts
# run concurrently instead of serializing web's full rollout before workflows starts.
kubectl set image -n "${NS}" deployment/navigator-web       web="${REPO}/navigator-web:${SHA}"
kubectl set image -n "${NS}" deployment/workflows-service worker="${REPO}/navigator-workflows-service:${SHA}"
kubectl rollout status -n "${NS}" deployment/navigator-web       --timeout=300s
kubectl rollout status -n "${NS}" deployment/workflows-service --timeout=300s
```

If you only added/changed a Secret key (step 7b) without a new image at this SHA — e.g. the image tag is already current
— use `kubectl rollout restart` instead of `set image` so the pods re-read the Secret; pods cache `envFrom` at start and
never reload.

`kubectl set image` patches the deployment in place; each `rollout status` blocks until that new ReplicaSet is fully
ready or times out. If either rollout fails, roll back **that** deployment with `kubectl rollout undo` and investigate
before retrying. If the new pod lands in `CrashLoopBackOff`, read its `--previous` logs first — a boot error about
production invariants being violated means you skipped 7b's Secret check.

After a clean rollout, smoke-check the public surface. The root serves the marketing home, so grep a fixed phrase from
the `home.md` hero to confirm the page is non-empty:

```bash
curl -fsS "https://www.${NAVIGATOR_PRIMARY_DOMAIN}/" \
  | grep -ciF 'an american law firm' && echo "landing OK"   # private-mode copy
# workflows-service has no public /, so confirm the worker is 3/3 ready:
kubectl -n "${NS}" get pods -l app=workflows-service
```

If you see `HomeContent::default()` fallback copy instead of the `home.md` body, an env var like
`NAVIGATOR_MARKETING_DIR` is probably missing from `images/Dockerfile.web` (the runtime image bundles content at
`/app/content/<tree>/`, but the binary needs the env vars to know where to look). Fix `images/Dockerfile.web`, commit,
rerun power-push.

#### 7d. Re-register the worker with Restate (so a new service isn't invisible)

**Do this on every ship.** Restate Cloud routes the ingress only to *registered* services, and registration is a
snapshot of the worker's handler list at register time — rolling a new worker image does **not** re-register it. A
service or handler added since the last registration silently `404`s at the ingress (this cost two hours the day it bit
the nightly Archives email). Re-registering after the rollout makes the registered set always match the deployed worker.
It is idempotent (`force` re-runs discovery), so running it every ship is safe and cheap.

Restate-Cloud-only; KIND uses `cargo run -p cli -- restate register`. It no-ops with a warning when the admin
endpoint/credential aren't resolvable (forks not on Restate Cloud), so it never blocks a ship. The admin API requires
`Content-Type: application/json` on the POST — without it the call `415`s (the `AUTH` array below carries both headers).

```bash
WORKER_URL="${NAVIGATOR_WORKFLOWS_URL:-https://workflows.${NAVIGATOR_PRIMARY_DOMAIN}/}"
# Admin API + credential: prefer explicit env (CI / a non-expiring Restate Cloud admin
# API key); else fall back to what `restate cloud login` wrote to the CLI config.
RCFG="${HOME}/.config/restate/config.toml"
ADMIN_URL="${RESTATE_ADMIN_URL:-$(sed -n 's/^admin_base_url = "\(.*\)"/\1/p' "$RCFG" | head -1)}"
ADMIN_TOK="${RESTATE_ADMIN_TOKEN:-$(sed -n 's/^access_token = "\(.*\)"/\1/p' "$RCFG" | head -1)}"
if [[ -n "$ADMIN_URL" && -n "$ADMIN_TOK" ]]; then
  AUTH=(-H "Authorization: Bearer ${ADMIN_TOK}" -H "Content-Type: application/json")
  curl -fsS -X POST "${ADMIN_URL%/}/deployments" "${AUTH[@]}" \
    -d "{\"uri\":\"${WORKER_URL}\",\"force\":true,\"dry_run\":true}" | jq -r '.services[].name'
  curl -fsS -X POST "${ADMIN_URL%/}/deployments" "${AUTH[@]}" \
    -d "{\"uri\":\"${WORKER_URL}\",\"force\":true}" >/dev/null && echo "re-registered ${WORKER_URL}"
  curl -fsS "${ADMIN_URL%/}/services" -H "Authorization: Bearer ${ADMIN_TOK}" | jq -r '.services[].name' | sort
else
  echo "WARN: Restate auto-register skipped (no admin URL/token). On Restate Cloud, register"
  echo "      manually — see docs/durable-workflows.md 'The registration gotcha'."
fi
```

> The SSO token from `restate cloud login` expires (~24h); for unattended / CI deploys set a non-expiring Restate Cloud
> **admin-scoped API key** as `RESTATE_ADMIN_TOKEN` (with `RESTATE_ADMIN_URL`). The ingress `key_` does **not** work for
> the admin API — it is ingress-scoped (`:8080`); registration is admin-scoped (`:9070`). Full mechanism in
> [`docs/durable-workflows.md`](../../../docs/durable-workflows.md).

### 8. Reclaim disk

```bash
docker rmi navigator-web:dev navigator-workflows-service:dev 2>/dev/null || true
```

The images now live in GAR (and are running in the cluster); the local `:dev` copies are just disk weight (~100–140 MB
per build). Don't `docker system prune --all` here — that nukes layer caches that make the next `navigator image` fast.
Over many power-pushes the retagged `:<sha>` images for both binaries accumulate locally; clear the backlog occasionally
with [[docker-cleanup]] (`docker image prune -a`), not as part of a ship.

## Sequencing rationale (full-build flow)

- **Commit before image** so the image tag is a real commit SHA, not a dirty tree. The bundle name and the image tag
  both come from the same `git rev-parse --short HEAD` — they must agree.
- **Verify before image** because the image bakes the binary; a clippy or test failure caught here saves a 2-minute
  build that ships broken code.
- **GAR push before GCS bundle** so that by the time anyone restores from the bundle and inspects the manifest pointing
  at `…/navigator-web:<sha>`, the image at that tag already exists in the registry.
- **Bundle before deploy** so a restore-from-bundle scenario can reconstruct any SHA the cluster has ever served — the
  bundle is the durable record of "this code shipped." One bundle covers both binaries at the SHA.
- **Secret-invariant check before deploy** (7b) because the new image crash-loops at boot on a missing required key; a
  one-line `comm` diff catches it before the rollout silently stalls on a `CrashLoopBackOff` pod.
- **Both binaries, one SHA, concurrent rollout** so the public surface and the durable-execution worker never diverge —
  no window where new `web` submits a workflow step an old worker can't execute.
- **Re-register after the rollout** (7d) because a Restate Cloud registration is a snapshot, not a subscription: a
  service added since the last register stays invisible at the ingress (`404 service not found`) until you re-register.
  Doing it every ship makes the registered set always match the deployed worker.
- **Deploy before `docker rmi`** so the registry copy is confirmed in place AND running in the cluster before the local
  artifact is destroyed.

## The no-rebuild push — `kubectl rollout restart` after secret rotation

**Not every "push to prod" needs an image.** When you rotate a value in the K8s Secret that the deployments `envFrom` —
SendGrid key, OIDC secret, Restate token, session secret — the running pods do **not** see the new value. K8s evaluates
`envFrom: secretRef` at pod-start and never reloads. The secret object updates, the pods keep serving with stale env.

Symptom that tells you you're in this mode: the pod logs say the call succeeded, but the third-party side has no record
of it.

**Two distinct failure modes share that surface symptom** — work through both before assuming one.

**Failure mode A — stale Secret value** (this section's flow).

A key was rotated in the Secret; the pods boot-cached the old value; `kubectl rollout restart` fixes it.

We hit this once with a `SENDGRID_API_KEY` rotation. The pod logged ten "welcome email sent" attempts against the stale
key; upstream stats showed zero requests for the day.

**Failure mode B — stale env-list schema** (manifest drift; see step 7a + "Detecting manifest drift" below).

A new env var was added to the overlay but never applied to the cluster. The running container has no value for it; the
binary takes its fallback branch — possibly a no-op, a `CapturingEmail`-style dev backend, or another code path.

We hit this with `NAVIGATOR_EMAIL_BACKEND=sendgrid`. The var lived in `patches/web-env.yaml` but never landed on the
cluster, so `web::email::select_backend` returned `CapturingEmail`, `LoggingEmail` wrote `outcome="sent"` to the audit
table, and the HTTP POST to SendGrid never happened.

Restarting the pod doesn't fix this — you need step 7a or an out-of-band `kubectl apply -k` to bring the env list in.

### Recipe

```bash
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
SECRET_NAME="${NAVIGATOR_WEB_SECRET_NAME:-navigator-web-secrets}"

# 1. Confirm the secret has the value you expect, base64-decoded.
#    (Substitute the key name you rotated; example shows SendGrid.)
kubectl get secret -n "${NS}" "${SECRET_NAME}" \
  -o jsonpath='{.data.SENDGRID_API_KEY}' | base64 -d | head -c 32; echo

# 2. Restart EVERY deployment that envFrom's the Secret.
#    For Navigator that's web + the workflows worker; both
#    envFrom the same Secret so a stale value in either breaks
#    that path's outbound calls.
kubectl rollout restart -n "${NS}" \
  deployment/navigator-web \
  deployment/workflows-service

# 3. Wait for both to settle.
kubectl rollout status -n "${NS}" deployment/navigator-web      --timeout=120s
kubectl rollout status -n "${NS}" deployment/workflows-service  --timeout=120s

# 4. Verify on the third-party side. Hit your real upstream API,
#    not just the pod logs — the pod will happily 2xx against a
#    valid-but-wrong key. For SendGrid: compare credits or stats
#    before/after, or look for X-Message-Id on the next send.
```

### Detecting "I am in this trap" before restarting

Two quick diagnostics:

```bash
# How old is the running pod vs. the most recent Secret apply?
kubectl get pod -n "${NS}" -l app=navigator-web \
  -o jsonpath='{.items[0].status.containerStatuses[?(@.name=="web")].state.running.startedAt}'; echo

kubectl get secret -n "${NS}" "${SECRET_NAME}" \
  -o jsonpath='{.metadata.annotations.kubectl\.kubernetes\.io/last-applied-configuration}' \
  | head -c 400; echo
```

The annotation shows what was last `kubectl apply`-d. If the list of keys in the annotation is **shorter** than the live
`.data` map (`kubectl get secret -o json | jq .data | jq keys`), some keys were added imperatively — for example:

```bash
kubectl create secret ... --dry-run=client | kubectl apply -f -
kubectl patch <secret>
```

*And* the pod's env reflects only the keys present when it started, so anything added after that boot time is invisible
to the running process.

### Detecting manifest drift

Different problem, same symptom.

The Secret has the right values; the deployment's `env:` array is just stale — shorter than what your overlay in git
says it should be.

`kubectl rollout restart` won't help (it would recreate the pod from the same stale spec).

```bash
# 1. What env vars does the live deployment declare?
kubectl get deploy -n "${NS}" navigator-web \
  -o jsonpath='{.spec.template.spec.containers[?(@.name=="web")].env[*].name}' \
  | tr ' ' '\n' | sort > /tmp/live-env.txt

# 2. What env vars does your overlay declare?
#    Requires NAVIGATOR_GKE_OVERLAY_DIR pointing at a rendered overlay.
kubectl kustomize "${OVERLAY}" \
  | yq '. | select(.kind=="Deployment" and .metadata.name=="navigator-web")
            | .spec.template.spec.containers[]
            | select(.name=="web") | .env[].name' - \
  | sort > /tmp/overlay-env.txt

# 3. Diff. Names in overlay but not live = the drift you need to apply.
diff /tmp/overlay-env.txt /tmp/live-env.txt
```

If `diff` shows env names only in the overlay (lines prefixed `<`), the deployment is running on a stale schema and the
binary may be silently taking fallback branches for those vars. Fix with step 7a (`kubectl apply -k "${OVERLAY}"`) and
rollout.

If your fork doesn't use `yq`, the same check with `jq` against `kubectl get deploy -o json` works.

The point is to compare the **names** in the deployed env array to the **names** in the rendered overlay's env array.

## The workflows-service specifics

The full-build recipe above already ships `workflows-service` alongside `navigator-web` — that is the default, not a
variant. This section collects the worker-specific details the unified recipe glosses: its own image-tag lifecycle in
Artifact Registry, the GC-detection diagnostic, and the `deployment.yaml` env/volume patch shape for when a non-image
change must land on the worker.

Still worth a deliberate look when:

- The live deployment's image tag has been garbage-collected from Artifact Registry. Symptom: a new pod stuck in
  `ImagePullBackOff` with `not found` after any rollout, scale, or node-replacement event. The old pod keeps serving
  because it has the layers cached on its node, but the deployment is one cordon away from going dark.
- A change to `workflows-service/deployment.yaml` (env var, volume, sidecar) needs to land — same manifest-drift trap
  as web, but on the workflows side; use the strategic-merge patch below so image + env land in one rollout.

### Detecting "the tag has been GC'd from AR"

```bash
LIVE_IMAGE=$(kubectl -n "${NS}" get deploy workflows-service \
  -o jsonpath='{.spec.template.spec.containers[?(@.name=="worker")].image}')
echo "live: ${LIVE_IMAGE}"

# Does that tag still exist in AR? If this 404s, you're one node-replacement
# away from an outage — ship a fresh image now.
gcloud artifacts docker images describe "${LIVE_IMAGE}" \
  --project="${NAVIGATOR_GCP_PROJECT_ID}" 2>&1 | head
```

Worth running this same check against `navigator-web` periodically too — same failure mode, same diagnostic.

### Recipe — workflows-service only (rare)

The unified recipe already ships the worker. Reach for this standalone shape only for an out-of-band worker fix — e.g.
re-pushing a GC'd tag with no source change. The build subcommand, image name, and deployment/container names differ
from web; pre-flight (`.env` sourced) and verify (fmt + clippy + test + markdown lint) are *workspace-wide* and already
cover the worker, so don't re-run them when acting at a SHA you just verified for a web push.

```bash
# 1. Build.
cargo run --release -p cli -- image-workflows-service

# 2. Push.
SHA=$(git rev-parse --short HEAD)
IMAGE="${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}/navigator-workflows-service:${SHA}"
docker tag  navigator-workflows-service:dev "${IMAGE}"
docker push "${IMAGE}"

# 3. Bundle (skip if HEAD is already bundled from a same-SHA web push).
#    Otherwise mirror step 6 from the main recipe.

# 4. Deploy. Patch image AND any env/volume/sidecar change in ONE strategic
#    merge so the rollout cycles only once. The example below also sets
#    NAVIGATOR_EMAIL_BACKEND inline, which is the concrete drift we hit on
#    workflows-service the day we wrote this section.
kubectl -n "${NS}" patch deployment workflows-service --type=strategic --patch "$(cat <<EOF
spec:
  template:
    spec:
      containers:
        - name: worker
          image: ${IMAGE}
          env:
            - name: NAVIGATOR_EMAIL_BACKEND
              value: sendgrid
EOF
)"
kubectl -n "${NS}" rollout status deployment/workflows-service --timeout=300s

# 5. Reclaim.
docker rmi navigator-workflows-service:dev 2>/dev/null || true
```

### Smoke check

There's no public `/` to curl for workflows-service. Instead:

```bash
# Pod is up and Envoy + cloud-sql-proxy are ready (3/3).
kubectl -n "${NS}" get pods -l app=workflows-service

# Trigger a workflow that exercises the worker path you care about
# (e.g. an email-sending step) and confirm the third-party side received
# the call. For email: compare SendGrid stats / Activity before and after.
```

### When to ship both

If a single commit touches both binaries, ship them as **two sequential runs at the same SHA**: web first (it's the
public-facing surface), then workflows-service. The git bundle is uploaded once on the web run — both deployments trace
back to it. Don't try to coalesce into a single command flow; the "one binary per ship" constraint is there because
rollback granularity matters when something goes sideways.

## The archives variant

The `Archives` workflow is **hosted by `workflows-service`** — all workflows live on that one worker, so shipping
`archives/` changes is mostly a **workflows-service push** (rebuild it; the archives lib is compiled in). The only
archives-specific image is the thin nightly trigger:

- `navigator-archives-trigger` — the `trigger` binary, the `archives-trigger` CronJob. Built by
  `navigator image-archives-trigger`. There is **no** separate archives worker image and no new always-on pod.

Trigger this variant when a real diff touches `archives/` or `workflows-service/`.

```bash
# 1. Ship the standard both-binary push (workflows-service compiles in the Archives
#    workflow) — same SHA, same bundle. Then, if the trigger changed, add step 2.

# 2. If the trigger binary or its CronJob changed, ship the trigger image too.
cargo run --release -p cli -- image-archives-trigger
SHA=$(git rev-parse --short HEAD)
REPO="${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}"
docker tag  "navigator-archives-trigger:dev" "${REPO}/navigator-archives-trigger:${SHA}"
docker push "${REPO}/navigator-archives-trigger:${SHA}"
sed -i "s|navigator-archives-trigger:dev|navigator-archives-trigger:${SHA}|" examples/deploy/k8s/exports/cron-archives-trigger.yaml
kubectl --context="gke_${NAVIGATOR_GCP_PROJECT_ID}_${NAVIGATOR_GCP_LOCATION}_${NAVIGATOR_GKE_CLUSTER_NAME}-prod" \
  apply -k examples/deploy/k8s/exports/

# 3. workflows-service is already registered with Restate Cloud, so the `Archives`
#    workflow is discoverable once the new worker image rolls out — no new registration.

# 4. Trigger a run and confirm the diagnostic email arrives.
kubectl -n "${NS}" create job --from=cronjob/archives-trigger archives-trigger-manual-001

# 5. Reclaim the local trigger image.
docker rmi navigator-archives-trigger:dev 2>/dev/null || true
```

> **Cost note.** Folding `Archives` into the already-24/7 `workflows-service` adds **$0** of new always-on compute —
> the deliberate choice over a separate always-on worker (which would have been ~$25–35/mo on Autopilot for a
> once-a-night job). The only marginal cost is the nightly trigger Job (seconds) and, if `BILLING_EXPORT_TABLE` is set,
> the cost-summary BigQuery query (one small scan/night).

## What this skill is NOT

- It is **not** a substitute for a real collaboration substrate. The GCS source bundle is a backup + portability channel
  — anyone with bucket read can `gcloud storage cp` the bundle locally and `git clone` it to recover the full repo. It
  is **not** for multi-operator concurrent work; if that need arises later, add a real git remote alongside the bundle
  upload, not instead of it.
- It is **not** a partial ship. Always build, push, and roll out **both** `navigator-web` and `workflows-service` at the
  same SHA — never just one. They share a Secret and a workflow contract; shipping one alone invites version skew. The
  worker-specific details (GC detection, `deployment.yaml` patch shape) live in "The workflows-service specifics".
- It is **not** for the KIND dev loop. `navigator deploy` already bundles "build + kind load + apply" for local. This
  skill is the prod-bound flavor. See [[kind-local-dev]].

## Constraints

- **Single region per deploy.** Every artifact (image, repo, cluster pull) is in `${NAVIGATOR_GCP_LOCATION}`. Don't push
  to a different region "just in case".
- **No service-account JSON keys.** Operator ADC + Workload Identity end-to-end. If a step prompts for a key file,
  stop — something is wrong upstream.
- **One commit per ship.** If three things changed, decide upfront whether they're one bundle or three; don't ship a
  half-bundle and improvise the rest later.
- **No secrets, no real project IDs, no real domains in this skill file.** The skill is committed to the repo.
  Everything that varies per deployment flows through `.env`. If you're tempted to hard-code a value, the value either
  (a) belongs in `.env.example` with a sensible default, or (b) is a workspace convention shared by every fork — in
  which case document the convention next to its usage, but still don't bake an organization's literal identifier into
  the text.

## Related context this session surfaced

- The SendGrid → Iceberg-on-GCS → BigQuery data-lake pipeline is the next phase for outbound mail observability. The
  current `sent_emails` Postgres table is the request-side staging surface. Design draft:
  `prompts/sendgrid-events-to-iceberg.md` (gitignored).
- `/portal/admin/email-log` is intentionally not linked from the dashboard; it's reachable by URL only, because the
  table is an Iceberg staging surface, not a routine admin listing.
- See [[cloud-rest-endpoints]] for how `navigator gcp setup` talks to Google's REST APIs (the provisioning side that
  this skill assumes is already done).
- See [[postgres-in-kind]] for the dev-loop analogue (host runs `web`, in-cluster Postgres) that this skill's prod
  flow mirrors.
- The OSS-publishable walk-through that this skill complements is
  [`docs/deploy/gke-power-push-example.md`](../../../docs/deploy/gke-power-push-example.md) and
  [`docs/oss-install.md`](../../../docs/oss-install.md).
