---
name: kind-local-dev
description: >
  Local Kubernetes-in-Docker (KIND) workflow for the navigator workspace — cluster lifecycle, ingress, port-forwarding,
  the "host runs `web`, deps run in cluster" iteration pattern via the `navigator` CLI. Trigger when running any
  `navigator` orchestration subcommand (start-dev-server/down/deploy/kind-up/kind-down/e2e/logs/worktree-env), editing
  `k8s/kind-config.yaml`, debugging an in-cluster service from the host, or onboarding the cluster from a fresh machine.
  Also trigger before installing a different local-Kubernetes flavor — we standardize on KIND. Also trigger before
  proposing to run, preview, screenshot, or manually exercise the app, or before any action that runs on the user's
  machine (cluster, browser, or cloud commands) — those resolve to this KIND loop, and the move is to propose the
  commands for the user to run.
---

# KIND-based local development

The `navigator` CLI (`cli::devx`) drives both the "host runs `web`" developer loop and the "full stack in KIND"
CI-shaped flow — there is no Makefile. Everything factual — the cluster config and `extraPortMappings`, the port-forward
table, the ghcr pull/retag/`kind load` image flow, the per-worktree environment, and the teardown — lives in
[`docs/RUNBOOK.md`](../../../docs/RUNBOOK.md); read it and keep it, not this skill, authoritative. In-cluster Postgres
specifics are in [`docs/RUNBOOK.md`](../../../docs/RUNBOOK.md) §7b and
[`docs/test-database.md`](../../../docs/test-database.md).

## How to treat it (the load-bearing rules)

- **Any run/preview/screenshot resolves to this KIND loop, and you propose the commands.** When a task needs the running
  stack — e2e, browser screenshots, manual verification, "open the design page" — bringing the cluster up (`docker`,
  `kind`, `kubectl`) runs on the user's machine, so propose the exact commands for the user to run (they prefix with `!`
  to run in-session). The standard loop is `start-dev-server`, then source `.devx/env`, then `cargo run -p web` (binds
  `:3001`).
- **We standardize on KIND.** Before installing another local-Kubernetes flavor (minikube, k3d, Docker Desktop k8s),
  stop — the manifests, port mappings, and `cli::devx` orchestration all assume KIND.
- **In-cluster Postgres is ephemeral by design.** Postgres and fake-gcs-server use `emptyDir`, so restarting their pod
  wipes the database and bucket — every developer starts from the same blank state and "works on my machine" drift can't
  accrue. Persistence in dev is a non-goal; production shape is Cloud SQL, pointed at via `DATABASE_URL`.
- **`web` reaches in-cluster Postgres over a host port-forward.** A host-side `cargo run -p web` can't reach Postgres
  until `start-dev-server` has written `.devx/env` and you have `set -a; source .devx/env; set +a` — the port-forward
  (`127.0.0.1:15432`, off the standard `5432` to dodge a host Postgres) plus that env block is the bridge. "Ready
  cluster but `web` can't connect" is almost always the un-sourced env.
- **Screenshots go to `/tmp`, never the repo** — `/tmp/navigator-screenshots/` (`mkdir -p` first). The working tree
  stays clean.

## Anti-patterns

- Editing `k8s/` manifests for a one-off debug flag and forgetting to revert — use `kubectl patch` or a kustomize
  overlay for a transient change.
- Expecting the host cluster and CI's KIND cluster to behave identically — the host has whatever you `kind load`ed, CI
  starts clean.
- Using `:latest` image tags — tag with a content hash or `YY.MM.DD` so `kind load` + rollout are deterministic.
- `kubectl delete pod` to "fix" a `CrashLoopBackOff` — that hides the failure mode; read `kubectl logs --previous` then
  `kubectl describe pod` instead.

## Boundaries

- The browser half of the local loop (drive Chrome, screenshot, verify a UI change): [[web-preview]].
- OPA authz, Keycloak/OIDC, and Restate durable execution each have their own skill — this one owns the cluster, not the
  service.
