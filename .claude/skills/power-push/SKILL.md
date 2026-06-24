---
name: power-push
description: >
  One-shot "roll prod onto today's image" workflow — take the BOTH images GitHub already built and published to
  ghcr.io (navigator-web and navigator-workflows-service, tagged `YY.MM.DD` for the release date), pin both GKE
  deployments to the latest published `YY.MM.DD` tag, and roll them out together (always both, never one). Includes a
  pre-deploy check that the prod Secret carries every key the new binary's boot invariants require (a missing one — e.g.
  DOCUSIGN_HMAC_KEY — crash-loops the pod). Also covers the no-image-rebuild "push" — `kubectl rollout restart` after
  rotating values in the K8s Secret. We no longer build images locally: the daily tag flow (`deploy.yml`) builds and
  publishes them; power-push only updates the cluster. Trigger when the user says "power-push", "ship this", "deploy
  this", "roll prod onto the latest image", "update the cluster", OR when they've rotated a key/value in the K8s Secret
  and need the running pods to pick it up. Every project / region / domain / cluster / ghcr-owner value is read from the
  environment (see [`.env.example`](../../../.env.example)) — nothing is hard-coded, so forks ship to their own GCP
  project + GitHub org without editing this skill.
---

# power-push

Roll prod onto the image GitHub already built. The images are **not** built on your laptop anymore — the daily tag flow
([`deploy.yml`](../../../.github/workflows/deploy.yml)) builds both `navigator-web` and `navigator-workflows-service`,
runs the full KIND integration suite, and publishes them to **ghcr.io** tagged with the calendar release tag `YY.MM.DD`
(plus `latest`). power-push's whole job is to **take the latest published `YY.MM.DD` images and update the Google Cloud
cluster**: resolve the newest tag → confirm the Secret satisfies the new binary's boot invariants → pin both
deployments to that tag and roll them out together → re-register the worker with Restate. **Always roll both
`navigator-web` and `workflows-service`** at the same tag; never just one.

## power-push does not commit — code reaches prod through PRs

power-push deploys an image that **already exists**, so it never commits, branches, or builds. The path a code change
takes to prod is:

1. **Land the code via a PR.** Per [`CLAUDE.md`](../../../CLAUDE.md) Commit discipline, never commit on `main`: do the
   work on a topic branch and open a PR (the [`create-pr`](../create-pr/SKILL.md) skill, `/create-pr`, is the front
   door — branch → push → `gh pr create` → `gh pr merge --auto --squash`). `ci.yml` runs on the PR and GitHub
   squash-merges it once every required check is green. You don't babysit the merge.
2. **The daily tag flow builds the image.** The cron flow
   ([`release-tag.yml`](../../../.github/workflows/release-tag.yml)) cuts a `YY.MM.DD` tag at the tip of `main` at
   02:00 PST; the tag push triggers `deploy.yml`, which runs KIND integration and publishes both images to ghcr.io at
   that tag. (To ship same-day instead of waiting for 02:00, an operator can `workflow_dispatch` `release-tag.yml` or
   push a `YY.MM.DD` tag by hand.)
3. **power-push rolls the cluster onto it.** That is this skill — run it from `main` once the image you want exists in
   ghcr.io.

So power-push runs **from `main`**, after the PR has merged and the image is published. There is no "commit step" here
anymore.

Also: the **no-rebuild push** for secret rotation — see the last section. Both flows count as "powering a push to prod."

## The fast path: `navigator power-push`

This workflow is a subcommand of the `navigator` CLI —
[`cli/src/devx/power_push.rs`](../../../cli/src/devx/power_push.rs). It is **roll-only**: it resolves the latest
published `YY.MM.DD` tag from ghcr.io (or takes `--tag YY.MM.DD`), confirms the Secret invariants, pins **both**
deployments to that one tag and rolls them out together, then re-registers the worker with Restate. It builds **no**
images locally, pushes nothing to a registry, and archives no git bundle — the daily tag flow (`deploy.yml`) is the only
thing that builds and publishes images. Run it under `doppler run --project navigator --config prd --` (it reads
`NAVIGATOR_GHCR_OWNER` and the cluster/domain config from the environment); the manual `kubectl` recipe below mirrors
what the subcommand does, step for step, so use it to understand or debug a roll.

After a roll, confirm the cluster is on the tag you intended: `GET /version` reports the running build, with the
headline `release` field carrying the `YY.MM.DD` ghcr tag (`web` reads it from `NAVIGATOR_RELEASE_TAG`, set on the
deployment). An operator, CI, AIDA, or a browser can hit `https://www.${NAVIGATOR_PRIMARY_DOMAIN}/version` and read the
`release` field to verify which dated image is live.

**Run every cluster command under the Doppler `prd` config.** This is a production roll, and `prd` carries the
production values the deploy reads — `NAVIGATOR_PRIMARY_DOMAIN` (the smoke-check host), the production Restate wiring,
and the ghcr owner. The `dev` config is for local development and tests; rolling under it points the smoke check and
re-register step at the wrong place. This workspace is Doppler-only — there is no `.env` on disk — so inject the config
with `doppler run --project navigator --config prd --`. The `--project navigator` flag is required: this repo carries no
project-scoped Doppler config, so a bare `doppler run --config prd` cannot resolve which project to read and errors out.

```bash
# Prefix every manual block below with this:
doppler run --project navigator --config prd -- <command>
```

## When to invoke

- The user wants the latest GitHub-published image live in prod and says any of:
  - "power-push", "ship this", "deploy this"
  - "roll prod onto the latest image", "update the cluster", "pull today's image"
- A PR has merged, the daily tag flow has published a fresh `YY.MM.DD` image, and the user wants the cluster on it.
- The user just changed a value in the K8s Secret (SendGrid key rotation, OIDC secret rotation, Restate token rotation,
  etc.) and asks why the change hasn't taken effect → jump to **The no-rebuild push** below.

## When NOT to invoke

- The image you want isn't published yet. If `ci.yml` for the merge is still running, or the daily tag flow hasn't cut
  today's tag, there is nothing in ghcr.io to roll onto. Wait for the publish, or trigger `release-tag.yml` by hand.
- To get **un-merged** code to prod. power-push only deploys published images; un-merged code has no image. Land the PR
  first (`/create-pr`), let the tag flow build it, then power-push.

## Configuration comes from Doppler `prd` — nothing is hard-coded

Every project / region / domain / cluster / ghcr-owner value flows through env vars — there is **no** literal GCP
project ID, domain, region, registry path, or GitHub org baked into this file. This workspace is Doppler-only (no `.env`
on disk); inject the production values by running each command under `doppler run --project navigator --config prd --`.
A fork pointed at a different cloud account or GitHub org ships by setting the same vars in its own secret store, not by
editing this skill.

**If a `.env` ever exists on disk it must stay gitignored.** The repo's `.gitignore` lists both `.env` and `.env.*`
(confirm with `grep -n '^\.env' .gitignore`). **Never** `git add .env`, never commit it, never paste its contents into a
chat. The OSS-publishable template is [`.env.example`](../../../.env.example); real secrets only ever live in `.env`
(local) and the K8s Secret (prod).

| Variable | Meaning | Example |
| --- | --- | --- |
| `NAVIGATOR_GHCR_OWNER` | lowercase GitHub owner that owns the published packages | `neon-law-foundation` |
| `NAVIGATOR_GCP_PROJECT_ID` | target GCP project (for the cluster context) | `my-org-prod` |
| `NAVIGATOR_GCP_LOCATION` | region for the cluster | `us-west4` |
| `NAVIGATOR_GKE_CLUSTER_NAME` | cluster name | `navigator` (default) |
| `NAVIGATOR_PRIMARY_DOMAIN` | public hostname for the smoke-check curl | `example.com` |
| `NAVIGATOR_K8S_NAMESPACE` | K8s namespace for the Deployments | `navigator` (default) |
| `NAVIGATOR_GKE_OVERLAY_DIR` | private kustomize overlay path (substituted); enables step 3 | `~/work/nav-overlay` |

```bash
: "${NAVIGATOR_GHCR_OWNER:?set in .env}"
: "${NAVIGATOR_GCP_PROJECT_ID:?set in .env}"
: "${NAVIGATOR_GCP_LOCATION:?set in .env}"
: "${NAVIGATOR_GKE_CLUSTER_NAME:?set in .env}"
: "${NAVIGATOR_PRIMARY_DOMAIN:?set in .env}"
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
OVERLAY="${NAVIGATOR_GKE_OVERLAY_DIR:-}"   # optional; see step 3
```

`NAVIGATOR_GKE_OVERLAY_DIR` is **optional but strongly recommended.** Without it, the roll falls back to a bare
`kubectl set image`, which patches only the image field. Any non-image change in your overlay (env var, volume mount,
sidecar, resource bump) silently fails to reach prod until something else triggers a full apply. Forks on GitOps (Config
Sync, Argo CD, Flux) can leave it unset — their controller reconciles the overlay continuously. Forks running
`kubectl apply` from a laptop should set it.

### Derived names (convention, not configuration)

| Derived name | Formula |
| --- | --- |
| ghcr registry path | `ghcr.io/${NAVIGATOR_GHCR_OWNER}` |
| `navigator-web` image | `ghcr.io/${NAVIGATOR_GHCR_OWNER}/navigator-web:<TAG>` |
| `workflows-service` image | `ghcr.io/${NAVIGATOR_GHCR_OWNER}/navigator-workflows-service:<TAG>` |
| Cluster context | `gke_${NAVIGATOR_GCP_PROJECT_ID}_${NAVIGATOR_GCP_LOCATION}_${NAVIGATOR_GKE_CLUSTER_NAME}-prod` |

The image **name** is lowercase per ghcr's `${GITHUB_REPOSITORY_OWNER,,}` convention (see `deploy.yml`'s
`derive lowercase image name` step) — so `NAVIGATOR_GHCR_OWNER` must be the lowercased owner.

**The cluster pulls from ghcr.io anonymously.** The `navigator-*` packages under `ghcr.io/${NAVIGATOR_GHCR_OWNER}` are
**public**, so GKE nodes pull them without credentials — there is no imagePullSecret, no in-cluster registry credential,
and nothing to rotate. A fork that chooses to publish its packages privately would need a
`kubernetes.io/dockerconfigjson` imagePullSecret for ghcr.io referenced from both deployments' `imagePullSecrets` (a
one-time setup, not a per-ship step); a pod stuck in `ImagePullBackOff` with `401 Unauthorized` against `ghcr.io` is the
symptom of a private package without that Secret.

## The roll-the-cluster recipe

Run these in order under `doppler run --project navigator --config prd --`. No image is built; every step is a read or a
`kubectl` against the prod cluster.

### 1. Pre-flight

```bash
# Pin kubectl to the prod context so a stale current-context can't misdirect the roll.
CTX="gke_${NAVIGATOR_GCP_PROJECT_ID}_${NAVIGATOR_GCP_LOCATION}_${NAVIGATOR_GKE_CLUSTER_NAME}-prod"
kubectl config use-context "${CTX}"
kubectl get ns "${NS}" >/dev/null                  # cluster reachable + namespace exists?
git fetch --tags origin                            # so we can see the latest published YY.MM.DD tag
```

### 2. Resolve the latest published `YY.MM.DD` image tag

The release git tag **is** the image tag (`deploy.yml` tags the image with `github.ref_name`), so the newest `YY.MM.DD`
tag on `origin` names the newest published image. Resolve it once and pin **both** deployments to it so web and the
worker stay in lockstep.

```bash
REPO="ghcr.io/${NAVIGATOR_GHCR_OWNER}"
TAG=$(git ls-remote --tags --refs origin \
      | grep -oE '[0-9]{2}\.[0-9]{2}\.[0-9]{2}$' | sort | tail -1)
echo "latest release tag: ${TAG}"

# Confirm BOTH images actually exist in ghcr at that tag before rolling — the tag
# flow could still be mid-run or have failed integration (in which case nothing was
# published). `docker manifest inspect` is a registry read; it needs no local pull.
docker manifest inspect "${REPO}/navigator-web:${TAG}"               >/dev/null && echo "web @ ${TAG} ✓"
docker manifest inspect "${REPO}/navigator-workflows-service:${TAG}" >/dev/null && echo "worker @ ${TAG} ✓"
```

If either `manifest inspect` fails with `manifest unknown`, the image at that tag isn't published — the tag flow hasn't
finished or its KIND integration went red. Check the `deploy.yml` run for that tag before going further; do **not** fall
back to an older tag for only one of the two binaries (that reintroduces version skew). `:latest` also points at the
newest successful publish, but pin the explicit `YY.MM.DD` so the rolled image is recorded and reproducible.

### 3. Sync the manifest (skip only on GitOps)

`kubectl set image` (step 5) only patches the image field. Any other change in your overlay (env var, volume mount,
sidecar, resource bump) is invisible to it. We hit this concretely once: `NAVIGATOR_EMAIL_BACKEND=sendgrid` was added to
`patches/web-env.yaml` and merged, but every subsequent roll only ran `set image`, so the prod pod kept booting without
the var — outbound email silently fell back to the in-memory `CapturingEmail` backend, the audit table wrote
`outcome="sent"`, and SendGrid saw zero requests for the day. See "Detecting manifest drift" below.

If `NAVIGATOR_GKE_OVERLAY_DIR` is set, dry-run a diff first to surface drift, then apply:

```bash
if [[ -n "${OVERLAY}" ]]; then
  kubectl diff -k "${OVERLAY}" || true   # exits 1 when a diff exists — that's the signal, not a failure
  kubectl apply -k "${OVERLAY}"
fi
```

On Config Sync / Argo / Flux your controller reconciles the overlay continuously — skip this and trust the controller.
If you're applying from a laptop and **didn't** set `NAVIGATOR_GKE_OVERLAY_DIR`, accept that this roll is image-only.

### 4. Confirm the prod Secret satisfies the new binary's invariants

**Do this before bumping the image, every time.** `web::config::enforce_prod_invariants` runs at boot and **crash-loops
the pod** if a required key is missing — there is no `APP_ENV` escape hatch. When a merged change added a new required
secret (it lives in `web/src/config.rs`), the prod Secret must gain that key *before* the new image rolls, or the new
pod `CrashLoopBackOff`s while the old pod keeps serving (no outage, but the rollout silently never completes).

We hit this shipping the e-signature loop: the new binary required `DOCUSIGN_HMAC_KEY` (without it the
`/webhook/esignature` endpoint would skip HMAC verification and anyone could forge a `completed` callback). The prod
Secret didn't have it; the new pod crash-looped until the key was added.

Diff the keys the new binary requires against the keys the live Secret carries:

```bash
SECRET_NAME="${NAVIGATOR_WEB_SECRET_NAME:-navigator-web-secrets}"
# Required keys, scraped straight from the invariant source so this never drifts.
# (Check out the YY.MM.DD tag first if your working tree isn't at the shipped commit:
#  `git checkout ${TAG} -- web/src/config.rs` then restore afterward.)
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
upstream (e.g. DocuSign Connect) is configured with the same value. (Some invariant keys — `NAVIGATOR_OPA_URL`,
`NAVIGATOR_STORAGE_BACKEND` — are deployment env, not Secret keys; if `comm` flags one of those, the fix is step 3, not
a Secret patch.)

### 5. Pin BOTH images to the tag and roll out together

**Always roll out both deployments together.** They envFrom the same Secret and move as a unit, so pinning both to the
same `YY.MM.DD` keeps the public surface and the durable-execution worker from diverging (no window where new `web`
submits a workflow step an old worker can't execute).

```bash
# Fire both image bumps back-to-back, THEN wait on both — so the two rollouts
# run concurrently instead of serializing web's full rollout before workflows starts.
kubectl set image -n "${NS}" deployment/navigator-web    web="${REPO}/navigator-web:${TAG}"
kubectl set image -n "${NS}" deployment/workflows-service worker="${REPO}/navigator-workflows-service:${TAG}"
kubectl rollout status -n "${NS}" deployment/navigator-web    --timeout=300s
kubectl rollout status -n "${NS}" deployment/workflows-service --timeout=300s
```

If the live deployments are **already** at `${TAG}` and you only changed a Secret key (step 4) — use
`kubectl rollout restart` instead of `set image` so the pods re-read the Secret; pods cache `envFrom` at start and never
reload.

If either rollout fails, roll back **that** deployment with `kubectl rollout undo` (which returns it to the prior
`YY.MM.DD`) and investigate before retrying. If the new pod lands in `CrashLoopBackOff`, read its `--previous` logs
first — a boot error about production invariants being violated means you skipped step 4's Secret check.
`ImagePullBackOff` with `not found`/`401` means the tag isn't published or the cluster lacks ghcr pull access (see the
config note above).

After a clean rollout, smoke-check the public surface. The root serves the marketing home, so grep a fixed phrase from
the `home.md` hero to confirm the page is non-empty:

```bash
curl -fsS "https://www.${NAVIGATOR_PRIMARY_DOMAIN}/" \
  | grep -ciF 'an american law firm' && echo "landing OK"   # private-mode copy
# workflows-service has no public /, so confirm the worker is fully ready:
kubectl -n "${NS}" get pods -l app=workflows-service
```

If you see `HomeContent::default()` fallback copy instead of the `home.md` body, an env var like
`NAVIGATOR_MARKETING_DIR` is probably missing from `images/Dockerfile.web` — that's a Dockerfile fix that has to go
through a PR + the tag flow, not something power-push can patch on the cluster.

### 6. Re-register the worker with Restate (so a new service isn't invisible)

**Do this on every roll.** Restate Cloud routes the ingress only to *registered* services, and registration is a
snapshot of the worker's handler list at register time — rolling a new worker image does **not** re-register it. A
service or handler added since the last registration silently `404`s at the ingress (this cost two hours the day it bit
the nightly Archives email). Re-registering after the rollout makes the registered set always match the deployed worker.
It is idempotent (`force` re-runs discovery), so running it every roll is safe and cheap.

Restate-Cloud-only; KIND uses `cargo run -p cli -- restate register`. It no-ops with a warning when the admin
endpoint/credential aren't resolvable (forks not on Restate Cloud), so it never blocks a roll. The admin API requires
`Content-Type: application/json` on the POST — without it the call `415`s (the `AUTH` array below carries both headers).

```bash
WORKER_URL="${NAVIGATOR_WORKFLOWS_URL:-https://workflows.${NAVIGATOR_PRIMARY_DOMAIN}/}"
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

> The SSO token from `restate cloud login` expires (~24h); for unattended / CI rolls set a non-expiring Restate Cloud
> **admin-scoped API key** as `RESTATE_ADMIN_TOKEN` (with `RESTATE_ADMIN_URL`). The ingress `key_` does **not** work for
> the admin API — it is ingress-scoped (`:8080`); registration is admin-scoped (`:9070`). Full mechanism in
> [`docs/durable-workflows.md`](../../../docs/durable-workflows.md).

## Sequencing rationale (roll flow)

- **Resolve the tag before anything** so both deployments pin the same `YY.MM.DD` and can't diverge.
- **Confirm both manifests exist in ghcr** (step 2) before touching the cluster — a half-published tag (web present,
  worker missing because integration flaked) must abort the roll, not produce a skewed cluster.
- **Manifest sync before image bump** (step 3) because `set image` only patches the image field; non-image overlay
  changes won't reach prod otherwise.
- **Secret-invariant check before the bump** (step 4) because the new image crash-loops at boot on a missing required
  key; a one-line `comm` diff catches it before the rollout silently stalls on a `CrashLoopBackOff` pod.
- **Both binaries, one tag, concurrent rollout** so the public surface and the durable-execution worker never diverge.
- **Re-register after the rollout** (step 6) because a Restate Cloud registration is a snapshot, not a subscription.

## The no-rebuild push — `kubectl rollout restart` after secret rotation

**Not every "push to prod" needs a new image.** When you rotate a value in the K8s Secret that the deployments `envFrom`
— SendGrid key, OIDC secret, Restate token, session secret — the running pods do **not** see the new value. K8s
evaluates `envFrom: secretRef` at pod-start and never reloads. The secret object updates, the pods keep serving with
stale env.

Symptom that tells you you're in this mode: the pod logs say the call succeeded, but the third-party side has no record
of it.

**Two distinct failure modes share that surface symptom** — work through both before assuming one.

**Failure mode A — stale Secret value** (this section's flow). A key was rotated in the Secret; the pods boot-cached the
old value; `kubectl rollout restart` fixes it. We hit this once with a `SENDGRID_API_KEY` rotation: the pod logged ten
"welcome email sent" attempts against the stale key; upstream stats showed zero requests for the day.

**Failure mode B — stale env-list schema** (manifest drift; see step 3 + "Detecting manifest drift" below). A new env
var was added to the overlay but never applied to the cluster. Restarting the pod doesn't fix this — you need step 3 or
an out-of-band `kubectl apply -k` to bring the env list in. We hit this with `NAVIGATOR_EMAIL_BACKEND=sendgrid`: the var
lived in `patches/web-env.yaml` but never landed on the cluster, so `web::email::select_backend` returned
`CapturingEmail`, `LoggingEmail` wrote `outcome="sent"` to the audit table, and the POST to SendGrid never happened.

### Recipe

```bash
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
SECRET_NAME="${NAVIGATOR_WEB_SECRET_NAME:-navigator-web-secrets}"

# 1. Confirm the secret has the value you expect, base64-decoded.
kubectl get secret -n "${NS}" "${SECRET_NAME}" \
  -o jsonpath='{.data.SENDGRID_API_KEY}' | base64 -d | head -c 32; echo

# 2. Restart EVERY deployment that envFrom's the Secret (web + the workflows worker).
kubectl rollout restart -n "${NS}" \
  deployment/navigator-web \
  deployment/workflows-service

# 3. Wait for both to settle.
kubectl rollout status -n "${NS}" deployment/navigator-web      --timeout=120s
kubectl rollout status -n "${NS}" deployment/workflows-service  --timeout=120s

# 4. Verify on the third-party side. Hit your real upstream API, not just the pod
#    logs — the pod will happily 2xx against a valid-but-wrong key.
```

### Detecting "I am in this trap" before restarting

```bash
# How old is the running pod vs. the most recent Secret apply?
kubectl get pod -n "${NS}" -l app=navigator-web \
  -o jsonpath='{.items[0].status.containerStatuses[?(@.name=="web")].state.running.startedAt}'; echo
kubectl get secret -n "${NS}" "${SECRET_NAME}" \
  -o jsonpath='{.metadata.annotations.kubectl\.kubernetes\.io/last-applied-configuration}' \
  | head -c 400; echo
```

If the list of keys in the annotation is **shorter** than the live `.data` map
(`kubectl get secret -o json | jq .data | jq keys`), some keys were added imperatively, and the pod's env reflects only
the keys present when it started — anything added after that boot time is invisible to the running process.

### Detecting manifest drift

Different problem, same symptom. The Secret has the right values; the deployment's `env:` array is just stale — shorter
than what your overlay in git says it should be. `kubectl rollout restart` won't help (it recreates the pod from the
same stale spec).

```bash
# 1. What env vars does the live deployment declare?
kubectl get deploy -n "${NS}" navigator-web \
  -o jsonpath='{.spec.template.spec.containers[?(@.name=="web")].env[*].name}' \
  | tr ' ' '\n' | sort > /tmp/live-env.txt

# 2. What env vars does your overlay declare? (Requires NAVIGATOR_GKE_OVERLAY_DIR.)
kubectl kustomize "${OVERLAY}" \
  | yq '. | select(.kind=="Deployment" and .metadata.name=="navigator-web")
            | .spec.template.spec.containers[]
            | select(.name=="web") | .env[].name' - \
  | sort > /tmp/overlay-env.txt

# 3. Diff. Names in overlay but not live = the drift you need to apply.
diff /tmp/overlay-env.txt /tmp/live-env.txt
```

If `diff` shows env names only in the overlay (lines prefixed `<`), the deployment is running on a stale schema and the
binary may be silently taking fallback branches for those vars. Fix with step 3 (`kubectl apply -k "${OVERLAY}"`) and
roll. If your fork doesn't use `yq`, the same check with `jq` against `kubectl get deploy -o json` works.

## The trigger images (archives / billing-canary / statutes)

The `Archives`, billing-canary, and `statutes` workflows are **compiled into `workflows-service`**, so rolling the
worker onto the latest `YY.MM.DD` ships their code too — no separate worker image. Each also has a thin CronJob
*trigger* image (`navigator-archives-trigger`, `navigator-billing-canary-trigger`, `navigator-statutes-trigger`).
**The daily tag
flow publishes only `navigator-web` and `navigator-workflows-service`** — it does **not** build the trigger images. If a
trigger binary or its CronJob changes, that's a gap to close in `deploy.yml` (add the trigger image to the publish
matrix) so the trigger ships the same GitHub-built way; do **not** reintroduce a local `docker build` + GAR push for it
as a side channel. Until that's wired, a trigger-only change does not reach prod through power-push.

After a worker roll, you can confirm a workflow path end-to-end by firing its trigger and watching for the result:

```bash
kubectl -n "${NS}" create job --from=cronjob/archives-trigger archives-trigger-manual-001
# then confirm the diagnostic email arrives / the snapshot lands
```

## What this skill is NOT

- It is **not** an image builder. Images come from GitHub's tag flow (`deploy.yml`); power-push only updates the
  cluster. If you find yourself running `docker build` or `cargo run -p cli -- image`, you're off the path.
- It is **not** the way to get un-merged code to prod. Land it via a PR (`/create-pr`), let the tag flow build it, then
  roll.
- It is **not** a partial roll. Always pin and roll out **both** `navigator-web` and `workflows-service` at the same
  `YY.MM.DD` — never just one. They share a Secret and a workflow contract; rolling one alone invites version skew.
- It is **not** for the KIND dev loop. `navigator deploy` bundles "build + kind load + apply" for local. See
  [[kind-local-dev]].

## Constraints

- **No service-account JSON keys.** Operator ADC + Workload Identity for the cluster; ghcr pull is anonymous (public
  packages) or via a namespace imagePullSecret. If a step prompts for a key file, stop — something is wrong upstream.
- **Both binaries, one tag.** Web and the worker always roll at the same `YY.MM.DD`. Don't pin them to different dates.
- **No secrets, no real project IDs, no real domains, no real org in this skill file.** The skill is committed to the
  repo. Everything that varies per deployment flows through the environment. If you're tempted to hard-code a value, it
  either belongs in `.env.example` with a sensible default or is a workspace convention documented next to its usage.

## Related context

- [[create-pr]] — the branch → PR → auto-merge front door that lands the code power-push later deploys.
- [`deploy.yml`](../../../.github/workflows/deploy.yml) and
  [`release-tag.yml`](../../../.github/workflows/release-tag.yml) — the tag flow that builds + publishes the images, and
  the cron flow that cuts the `YY.MM.DD` tag.
- [[durable-execution]] — keeping the Restate worker alive; the re-register step (6) is its registration contract.
- [[cloud-rest-endpoints]] — how `navigator gcp setup` stands up the cluster this skill rolls onto.
- The OSS-publishable walk-through this skill complements is
  [`docs/oss-install.md`](../../../docs/oss-install.md).
