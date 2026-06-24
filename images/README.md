# Container images

Every Dockerfile the workspace ships, in one place. The build context is always the **repo root**, so each is built with
`-f images/<file> .` (the `COPY` paths are relative to the root, not this directory). The `navigator` CLI and the
[`power-push`](../docs/cloud-operations.md) rollout path do this for you.

There are **five** Dockerfiles for **seven** images — two long-running servers, three CronJob triggers that share one
parameterized Dockerfile, and two standalone services.

## Long-running servers

- **`Dockerfile.web`** → `navigator-web`. The `web` server (axum + SeaORM + maud): site, `/portal`, `/api`, `/mcp`, and
  git smart-HTTP. Build: `navigator image`.
- **`Dockerfile.workflows-service`** → `navigator-workflows-service`. The Restate worker hosting **every** workflow
  (`Notation`, `Archives`, `Statutes`, `BillingCanary`, `DriveSync`). Build: `navigator image-workflows-service`.

## CronJob triggers — one shared, parameterized Dockerfile

All workflows run inside the single `workflows-service` worker, but each still needs a tiny entrypoint to *start* a run
by POSTing to the Restate ingress. Those are byte-identical except the crate they build, so they share one
`Dockerfile.trigger`, built with `--build-arg CRATE=<crate>`:

- `navigator-archives-trigger` — `CRATE=archives`, starts `Archives` (nightly export). Build: `navigator
  image-archives-trigger`.
- `navigator-statutes-trigger` — `CRATE=statutes`, starts `Statutes` (weekly NRS scrape). Build: `navigator
  image-statutes-trigger`.
- `navigator-billing-canary-trigger` — `CRATE=billing-workflows`, starts `BillingCanary`. Build: `navigator
  image-billing-canary-trigger`.

Adding a workflow adds a `--build-arg` row here and a `navigator` target — never a new Dockerfile, never a new always-on
service.

## Standalone services

- **`Dockerfile.git`** → `navigator-git`. The git-serving tier (same musl-static binary as `web`, distinct deploy).
  Build: `docker build -f images/Dockerfile.git .`.
- **`Dockerfile.redirect`** → `navigator-redirect`. The Cloud Run redirect service (`chat.neonlaw.com` + apex 308s).
  Build: `docker build -f images/Dockerfile.redirect .`.

The devcontainer Dockerfile at `tools/dev/Dockerfile` is **not** part of this set — it bundles the toolchain for local
development and ships no application image.
