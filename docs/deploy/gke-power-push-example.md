---
name: power-push
description: >
  One-shot "ship to prod" workflow — commit pending changes, build BOTH the navigator-web and workflows-service images,
  push them to Artifact Registry tagged at HEAD's short SHA, archive a git bundle of HEAD into the source bucket,
  confirm the prod Secret satisfies the new binary's boot invariants, roll out both deployments together, re-register
  the worker with Restate, then reclaim disk. Trigger when the user says "power-push", "ship this", "push the bundle",
  "deploy this", or asks to "upload bundle to gcp" after a commit-worthy change.
---

# power-push

Ship a code change all the way to prod: commit → build both images → registry → GCS source bundle → secret-invariant
check → roll out both deployments together → re-register with Restate → reclaim disk. One workflow, in one session, in
this order, because each later step assumes the prior step's artifact exists. **Always ship both `navigator-web` and
`workflows-service`** at one SHA; never just one — they share a Secret and a workflow contract, so shipping one alone
invites version skew.

## The fast path: `navigator power-push`

This whole workflow is a deterministic subcommand of the `navigator` CLI. Prefer it over running the steps by hand: it
runs every step below in order, reads all config from the environment, and never commits for you (the image tag is
HEAD's short SHA, so commit first).

**Run it under the Doppler `prd` config.** This is a production ship, and `prd` carries the production values the deploy
reads — the smoke-check host, the production DocuSign keys, and the production Restate wiring. This workspace is
Doppler-only (there is no `.env` on disk), so inject the config with `doppler run`:

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

`--project navigator` is required because this repo carries no project-scoped Doppler config; without it `doppler run`
cannot resolve which project to read. The subcommand pins `kubectl` to `${NAVIGATOR_GKE_CONTEXT}` (default
`gke_<project>_<location>_<cluster>-prod`) so a stale current context can't misdirect the ship, and the secret-invariant
check reads `web/src/config.rs` directly — so it never drifts from the binary's real boot requirements and aborts with
the exact `kubectl patch` to run if a key is missing.

The rest of this document is the **rationale and the manual fallback**: what each step does and why it's ordered this
way. Read it to understand or debug the command; run the command to ship.

## When to invoke

- The user has a change ready to ship and says any of: "power-push", "ship this", "push the bundle", "deploy this", or
  "upload [bundle|image] to gcp" after a commit-worthy change.
- A diff is staged or recently committed and the user wants it in the registry + archived as a bundle in GCS.
- The user just rotated a value in the K8s Secret and the running pods haven't picked it up → use `--restart-only`.

## When NOT to invoke

- Work-in-progress changes. The image tag = HEAD short SHA, so the registry artifact gets a real commit name. Don't
  pollute the registry with throwaway tags.
- Pre-commit hooks failing. Fix the hook failure first; don't bypass.

## Configuration comes from the environment — nothing is hard-coded

Every project / region / domain / cluster value flows through env vars — there is no literal GCP project ID, domain,
region, registry path, or bucket name baked into the command. The production values live in Doppler `prd`; inject them
by running under `doppler run --project navigator --config prd --`. A fork pointed at a different cloud account ships by
setting the same vars in its own secret store.

| Variable | Meaning | Example |
| --- | --- | --- |
| `NAVIGATOR_GCP_PROJECT_ID` | target GCP project | `YOUR_PROJECT_ID` |
| `NAVIGATOR_GCP_LOCATION` | region for AR + bucket + cluster | `us-west4` |
| `NAVIGATOR_GKE_CLUSTER_NAME` | cluster name (also AR repo name) | `navigator` (default) |
| `NAVIGATOR_PRIMARY_DOMAIN` | public hostname for the smoke-check curl | `your-domain.example` |
| `NAVIGATOR_K8S_NAMESPACE` | K8s namespace for the Deployments | `navigator` (default) |

### Derived names (convention, not configuration)

| Derived name | Formula |
| --- | --- |
| Registry path | `${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}` |
| Source bucket | `gs://${NAVIGATOR_GCP_PROJECT_ID}-source/` |
| Cluster context | `gke_${NAVIGATOR_GCP_PROJECT_ID}_${NAVIGATOR_GCP_LOCATION}_${NAVIGATOR_GKE_CLUSTER_NAME}-prod` |

## The manual fallback recipe

Run these in order. Each is one or two lines. The production values must be injected from Doppler `prd` first (see
above).

### 1. Pre-flight

```bash
git status                                          # uncommitted work?
docker ps >/dev/null                                # daemon up?
gcloud storage ls "gs://${NAVIGATOR_GCP_PROJECT_ID}-source/" \
  --project="${NAVIGATOR_GCP_PROJECT_ID}" >/dev/null # ADC + bucket reachable?
kubectl config current-context                      # on the right cluster?
```

If `gcloud storage ls` 403s, the operator's ADC needs `roles/storage.objectUser` on
`gs://${NAVIGATOR_GCP_PROJECT_ID}-source` — the same GSA used for `roles/artifactregistry.writer` on the GAR repo can
carry both.

### 2. Commit any pending change

If there's a real diff worth shipping and it isn't committed, commit it. Otherwise skip to step 3. Co-author tag and
message style per `CLAUDE.md`:

```bash
git add <files>
git commit -m "$(cat <<'EOF'
<scope>: <one-line summary>

<body — the why, not the what>
EOF
)"
```

Push the commit to the GitHub remote (`git push origin main`) so the durable history lives in two places — the remote
and the GCS bundle from step 6. If GitHub rejects the push with a `GH007` email-privacy error, the commit carries an
email the account keeps private; set `git config user.email` to a non-private address (e.g. the
`@users.noreply.github.com` form), re-author the unpushed commits, and push again.

### 3. Verify before shipping

Four checks must all exit 0 before the images are built — fmt, clippy, tests, and the workspace-wide markdown lint:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cli --quiet -- validate --markdown-only --no-default-excludes .
```

A failing test means the image you're about to ship is broken — stop and fix. A failing markdown lint means the doc
update is going out half-baked; the docs are part of the artifact, so fix the lint or revert the doc change.

### 4. Build BOTH images

```bash
cargo run --release -p cli -- image                    # navigator-web:dev
cargo run --release -p cli -- image-workflows-service  # navigator-workflows-service:dev
```

Even if a given diff only touched one crate, build both — the cost is one extra cached `cargo` build and the rollback
story stays simple (both deployments trace to one bundle at one SHA).

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

### 6. Archive a git bundle to GCS

```bash
SHA=$(git rev-parse --short HEAD)
git bundle create "/tmp/navigator-${SHA}.bundle" --all
gcloud storage cp "/tmp/navigator-${SHA}.bundle" \
  "gs://${NAVIGATOR_GCP_PROJECT_ID}-source/navigator-${SHA}.bundle" \
  --project="${NAVIGATOR_GCP_PROJECT_ID}"
rm "/tmp/navigator-${SHA}.bundle"
```

`--all` includes every ref (branches + tags) so the bundle is a full restore point — `git clone <bundle>` works on any
machine with `gcloud` + a clean directory. Naming is `navigator-<short-sha>.bundle`. The repo also has a GitHub remote
(step 2), so the bundle is a second durable copy and a portability channel, not the only one.

### 7. Deploy to the prod cluster

This is four sub-steps: confirm the Secret satisfies the new binary's boot invariants (7a), bump both images and roll
out together (7b), then re-register the worker with Restate (7c).

#### 7a. Confirm the prod Secret satisfies the new binary's invariants

`web::config::enforce_prod_invariants` runs at boot and crash-loops the pod if a required key is missing. When a commit
adds a new required secret (it lives in `web/src/config.rs`), the prod Secret must gain that key before the new image
rolls. Diff the required keys against the live Secret:

```bash
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
SECRET_NAME="${NAVIGATOR_WEB_SECRET_NAME:-navigator-web-secrets}"
grep -oE '"[A-Z_]+ must be set' web/src/config.rs | grep -oE '[A-Z_]+' | sort -u > /tmp/required-keys.txt
kubectl -n "${NS}" get secret "${SECRET_NAME}" -o json | jq -r '.data | keys[]' | sort > /tmp/live-keys.txt
comm -23 /tmp/required-keys.txt /tmp/live-keys.txt   # names printed here will crash-loop the new pod
```

If that prints anything, add the missing key to the Secret first with `kubectl patch` — its value never transits the
chat.

#### 7b. Bump BOTH images and roll out at the same time

```bash
SHA=$(git rev-parse --short HEAD)
NS="${NAVIGATOR_K8S_NAMESPACE:-navigator}"
REPO="${NAVIGATOR_GCP_LOCATION}-docker.pkg.dev/${NAVIGATOR_GCP_PROJECT_ID}/${NAVIGATOR_GKE_CLUSTER_NAME}"

kubectl set image -n "${NS}" deployment/navigator-web    web="${REPO}/navigator-web:${SHA}"
kubectl set image -n "${NS}" deployment/workflows-service worker="${REPO}/navigator-workflows-service:${SHA}"
kubectl rollout status -n "${NS}" deployment/navigator-web    --timeout=300s
kubectl rollout status -n "${NS}" deployment/workflows-service --timeout=300s
```

If either rollout fails, roll back that deployment with `kubectl rollout undo` and investigate before retrying. After a
clean rollout, smoke-check the public surface:

```bash
curl -fsS "https://www.${NAVIGATOR_PRIMARY_DOMAIN}/" | grep -ciF 'an american law firm' && echo "landing OK"
kubectl -n "${NS}" get pods -l app=workflows-service   # worker has no public /; confirm it's ready
```

#### 7c. Re-register the worker with Restate

Restate Cloud routes the ingress only to registered services, and registration is a snapshot of the worker's handler
list at register time — rolling a new worker image does not re-register it. Re-register after every rollout so the
registered set always matches the deployed worker (it's idempotent and cheap). On KIND this is `cargo run -p cli --
restate register`; on Restate Cloud the subcommand posts to the admin API. It no-ops with a warning when the admin
endpoint/credential aren't resolvable, so it never blocks a ship.

### 8. Reclaim disk

```bash
docker rmi navigator-web:dev navigator-workflows-service:dev 2>/dev/null || true
```

The images now live in GAR (and are running in the cluster); the local `:dev` copies are just disk weight. Don't reach
for `docker system prune --all` here — it nukes the layer caches that make the next `navigator image` build fast.

## Sequencing rationale

- **Commit before image** so the image tag is a real commit SHA, not a dirty tree. The bundle name and the image tag
  both come from the same `git rev-parse --short HEAD` — they must agree.
- **Verify before image** because the image bakes the binary; a clippy or test failure caught here saves a build that
  ships broken code.
- **GAR push before GCS bundle** so that by the time anyone restores from the bundle and inspects the manifest pointing
  at `…/navigator-web:<sha>`, the image at that tag already exists in the registry.
- **Bundle before deploy** so a restore-from-bundle scenario can reconstruct any SHA the cluster has ever served. One
  bundle covers both binaries at the SHA.
- **Secret-invariant check before deploy** (7a) because the new image crash-loops at boot on a missing required key; a
  one-line `comm` diff catches it before the rollout silently stalls on a `CrashLoopBackOff` pod.
- **Both binaries, one SHA, concurrent rollout** so the public surface and the durable-execution worker never diverge —
  no window where new `web` submits a workflow step an old worker can't execute.
- **Re-register after the rollout** (7c) because a Restate Cloud registration is a snapshot, not a subscription: a
  service added since the last register stays invisible at the ingress until you re-register.
- **Deploy before `docker rmi`** so the registry copy is confirmed in place AND running in the cluster before the local
  artifact is destroyed.

## What this skill is NOT

- It is **not** a partial ship. Always build, push, and roll out **both** `navigator-web` and `workflows-service` at the
  same SHA — never just one. They share a Secret and a workflow contract; shipping one alone invites version skew.
- It is **not** for the KIND dev loop. `navigator deploy` already bundles "build + kind load + apply" for local. This
  skill is the prod-bound flavor.

## Constraints

- **Single region per deploy.** Every artifact (image, repo, cluster pull) is in `${NAVIGATOR_GCP_LOCATION}`. Don't push
  to a different region "just in case".
- **No service-account JSON keys.** Operator ADC + Workload Identity end-to-end. If a step prompts for a key file, stop
  — something is wrong upstream.
- **One commit per ship.** If three things changed, decide upfront whether they're one bundle or three; don't ship a
  half-bundle and improvise the rest later.
