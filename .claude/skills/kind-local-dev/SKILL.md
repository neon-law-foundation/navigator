---
name: kind-local-dev
description: >
  Local Kubernetes-in-Docker (KIND) workflow for the navigator workspace — cluster lifecycle, ingress, port-forwarding,
  the "host runs `web`, deps run in cluster" iteration pattern via the `navigator` CLI. Trigger when running any
  `navigator` orchestration subcommand (start-dev-server/down/deploy/kind-up/kind-down/e2e/logs/worktree-env), editing
  `k8s/kind-config.yaml`, debugging an in-cluster service from the host, or onboarding the cluster from a fresh machine.
  Also trigger before installing a different
  local-Kubernetes flavor — we standardize on KIND. Also trigger before proposing to run, preview, screenshot, or
  manually exercise the app, or before any action that runs on the user's machine (cluster, browser, or cloud commands)
  — those resolve to this KIND loop, and the move is to propose the commands for the user to run.
---

# KIND-based local development

Cluster manifest: `k8s/kind-config.yaml`. Service manifests:
`k8s/{namespace,postgres,gcs,keycloak,restate,opa,workflows-service,web}/`. The `navigator` CLI's orchestration
(`cli::devx`) drives both the "host runs web" developer loop and the "full stack in KIND" CI-shaped flow — there is no
Makefile.

## Modes

- **Full in-cluster** — E2E smoke tests, CI-shaped reproduction, demoing.
  `cargo run --release -p cli -- deploy && cargo run --release -p cli -- e2e`
- **Host-runs-web** (fast iteration) — Editing `web`, iterating on handlers or templates.
  `cargo run --release -p cli -- start-dev-server`, then `cargo run -p web` on the host.
- **Per-worktree** (parallel agents / Codex) — Each git worktree gets its own isolated environment on the **shared**
  deps cluster: its own Postgres database `navigator_<slug>` and host `web` port (`3001` + a stable per-slug offset),
  written to `<worktree>/.devx/env`. `cargo run -p cli -- worktree-env up` (then `down`/`status`); `--demo` runs the
  full in-cluster stack from ghcr. Idempotent; `down` drops only that database and never touches the shared deps. This
  is what Codex's per-worktree Setup/Cleanup scripts call (`worktree-env up/down --path "$CODEX_WORKTREE_PATH"`). Full
  recipe: `docs/RUNBOOK.md` §7c.

The `navigator` CLI brings the cluster up with every dependency *except* `web`, then prints env vars into
`.devx/env` that point the host-side `cargo run -p web` at the in-cluster Postgres / fake-gcs / Keycloak / Restate / OPA
/ `workflows-service`.

```bash
cargo run --release -p cli -- start-dev-server    # cluster + Operator + every dep + workflows-service
set -a; source .devx/env; set +a     # connection env vars into your shell
cargo run -p web                     # local web binds :3001, talks to in-cluster deps
cargo run --release -p cli -- down  # tear it all down
```

This is one pattern repeated: a host process reaches cluster dependencies over port-forwards — the same way the host
`web` binary reaches Postgres, Keycloak, fake-gcs, Restate, and OPA. Bringing the cluster up (`docker`, `kind`,
`kubectl`) runs on the user's machine, so when a task needs the running stack — e2e, browser screenshots, manual
verification — propose the commands for the user to run (they can prefix with `!` to run them in-session).

**Screenshots go to `/tmp`, never the repo.** Every screenshot taken while previewing or verifying `web` — browser
captures, `fantoccini` `screenshot()`, `chromedriver` grabs, ad-hoc UI shots — is written under
`/tmp/navigator-screenshots/` (`mkdir -p /tmp/navigator-screenshots` first), never to the repo root or a tracked path.
The working tree stays clean, so there is nothing to hand-delete after an iteration. The repo root's `/*.png` gitignore
rule is a backstop, not a license to write there.

## KIND cluster config

`k8s/kind-config.yaml` declares one control-plane node with extraPortMappings so host ports forward into the cluster:

| Host port | In-cluster | What it reaches |
| --- | --- | --- |
| `8080` | nginx-ingress | navigator-web behind ingress |
| `30080` | NodePort | Keycloak admin console |
| `30443` | NodePort | fake-gcs-server HTTP |

Add a new external port by editing `extraPortMappings`, then `cargo run --release -p cli -- kind-down && cargo run
--release -p cli -- kind-up`. Port mapping is set at cluster-create time — you can't add it to a running KIND cluster.

## Ingress

`nginx-ingress` is installed by `navigator kind-up` after the cluster is created. The web Deployment's Ingress
(`k8s/web/web.yaml`) routes `/` and friends to the `navigator-web` Service on port 3001. To add a new external route,
add a new `Ingress` to the relevant manifest — don't change ingress-class globally.

## Image flow

The local loop **pulls** the images CI published to ghcr — it no longer builds them on the host (the nine
`cargo run -p cli -- image*` build subcommands are gone; CI owns image builds, the `images/Dockerfile.*` stay).

- `docker pull ghcr.io/<owner>/navigator-web:<tag>` selects the host-arch variant; the resolved tag is
  `NAVIGATOR_IMAGE_TAG` if set, else the latest published `YY.MM.DD` (resolved via `devx::ghcr`, the same module
  ship uses).
- The pulled image is **retagged to `navigator-web:dev`** — the name the manifests reference — so the overlays stay
  byte-identical whether an image was historically built or is now pulled.
- `kind load docker-image navigator-web:dev --name navigator` pushes it into the cluster (KIND does **not** pull from a
  registry by default); the Deployment's `imagePullPolicy: IfNotPresent` then uses the loaded image.

`navigator deploy` does: `kind_up_steps` (idempotent) → resolve tag → `ensure_tag_published` → pull + retag + `kind
load` (both `navigator-web` and `navigator-workflows-service`) → `kubectl apply -k k8s/overlays/kind` → `kubectl rollout
status`. `start-dev-server` pulls only `workflows-service` (the host runs `web` from `cargo`). Pin a reproducible demo
with `NAVIGATOR_IMAGE_TAG=YY.MM.DD`.

## Inspecting what's running

```bash
kubectl --context kind-navigator -n navigator get pods
kubectl --context kind-navigator -n navigator logs -f deploy/navigator-web
kubectl --context kind-navigator -n navigator describe pod <pod>
kubectl --context kind-navigator -n navigator port-forward svc/postgres 5432:5432  # talk to in-cluster postgres from psql on host
```

The `--context kind-navigator` flag is the safety net against accidentally targeting a real cluster. Bake it into shell
aliases if you find yourself omitting it.

## Common gotchas

- **`navigator start-dev-server` says everything is ready but `cargo run -p web` can't reach a service.** You forgot
  `set -a; source .devx/env; set +a`. The env vars are the bridge.
- **Image change not reflected after `navigator deploy`.** The pulled image is always retagged to the fixed
  `navigator-web:dev`, so the Deployment's `image:` string never changes and Kubernetes sees "no diff" even when the
  underlying `:dev` content is newer. `kubectl rollout restart deployment/navigator-web` to force the new bits in.
- **`kind-up` fails with port `8080` in use.** Another process owns it on the host — usually a previous
  `kubectl port-forward` or a stale KIND cluster. `docker ps` then `kind delete cluster --name navigator` resolves both.
- **Pod is `CrashLoopBackOff`.** First `kubectl logs --previous`, then `kubectl describe pod` for events. Don't
  `kubectl delete pod` to "fix" it — that hides the real failure mode.

## Anti-patterns

- Editing manifests in `k8s/` to add a one-off debug flag and then forgetting to revert. Use `kubectl patch` or a
  kustomize overlay if you really need a transient change.
- Running tests against the host cluster *and* CI's KIND cluster expecting identical results — the host has whatever you
  `kind load`ed, CI starts from a clean slate.
- Using `:latest` tags. Always tag with a content hash or short sha so `kind load` + Deployment rollout are
  deterministic.
- Doing application logic in the Makefile. Makefile targets stay one or two lines wrapping shell incantations; complex
  logic moves into the `navigator` CLI's orchestration (`cli::devx`, Rust).

## Canonical sources

- KIND project (CNCF-adjacent, sigs.k8s.io): <https://kind.sigs.k8s.io/>
- KIND repository: <https://github.com/kubernetes-sigs/kind>
- Kubernetes documentation: <https://kubernetes.io/docs/>
- Kubernetes pod lifecycle / scheduling: <https://kubernetes.io/docs/concepts/workloads/pods/>
- `kubectl` cheatsheet: <https://kubernetes.io/docs/reference/kubectl/cheatsheet/>
- `nginx-ingress`: <https://github.com/kubernetes/ingress-nginx>
- CNCF landscape (find canonical projects): <https://landscape.cncf.io/>
