---
name: docker-cleanup
description: >
  Reclaim host disk space from Docker — three escalating stages (unused images → build cache → volumes), each with its
  own safety profile. Stage 1 (`docker image prune -a`) and stage 2 (`docker builder prune -a`) are non-destructive and
  reclaim the bulk of disk on a developer laptop that's been doing repeated `cargo build` + image rebuilds. Stage 3
  (`docker volume prune -a`) is destructive — it removes any volume not attached to a *running* container, which can
  nuke local Postgres data, Keycloak realm state, and KIND cluster state if the cluster is down. A Stage 0 sweep first
  removes leaked **running** `postgres:17-alpine` testcontainers (held alive by a `static` in `store::test_support`, so
  testcontainers-rs's Drop-based cleanup never fires — there is no Ryuk reaper in the Rust crate); they exhaust the
  docker0 bridge and break `docker build` with `exchange full`. Trigger when the user says "clean up docker", "prune
  docker", "reclaim disk", "free up space", "docker is using too much disk", after a long stretch of image-heavy work
  (many [[power-push]] iterations, build-cache thrash), or when a build fails with `bridge docker0 … exchange full`.
  Always print `docker system df` before and after so the user sees what was reclaimed.
---

# docker-cleanup

Reclaim host disk from Docker in three stages, ordered safest → most destructive. Always show the before/after so the
user can see what came back.

> **Docker is only half the story.** On this workspace the Rust `target/` tree (especially `target/debug/incremental`)
> is frequently a bigger disk hog than Docker. If the disk is full and you haven't measured yet, start with
> [[reclaim-disk]] — it sizes both consumers, reclaims the cargo/`target` side itself, and delegates the Docker side
> back to this skill. Use this skill directly when you already know Docker is the hog.

## When to invoke

- User says "clean up docker", "prune docker", "reclaim disk", "free up space".
- `docker system df` reports many GB reclaimable and the user wants it back.
- After a long stretch of image-heavy work — many [[power-push]] cycles, build-cache thrash, image-tag churn.
- A `docker build` / `navigator image` just failed with `adding interface veth… to bridge docker0 failed: exchange full`
  — that is leaked **running** testcontainers exhausting the docker0 bridge. Go straight to **Stage 0**; the prune
  stages will not help (they skip running containers).

## When NOT to invoke

- A KIND cluster the user cares about is **down** and stage 3 (volume prune) is on the table. Volumes for a stopped
  cluster are "unused" and will be deleted. Either bring the cluster up first (`make kind-up` /
  `navigator start-dev-server`) or skip stage 3.
- Mid-debug. If the user is investigating why a container exited, its volume may hold the only copy of the failure
  state. Don't prune until the investigation closes.

## Stage 0 — leaked testcontainers (running orphans)

Run this **first** when `docker ps` shows a swarm of `postgres:17-alpine` containers, or when a build just died with
`exchange full` on `docker0`. That error is the bridge running out of veth slots (the ceiling is ~1024). Stages 1–3
will **not** help: `image prune` and `volume prune` skip anything tied to a *running* container, and these orphans
are running. You have to remove the containers themselves.

```bash
docker ps --filter ancestor=postgres:17-alpine -q | wc -l          # how many are leaked
docker ps -a --filter ancestor=postgres:17-alpine -q \
  | xargs -r -P8 -n50 docker rm -f                                 # remove them in parallel batches
docker ps --format '{{.Names}}' | grep -E 'navigator-(control-plane|worker)'   # confirm KIND survived
```

The KIND nodes run `kindest/node`, not `postgres:17-alpine`, so the `ancestor` filter never touches them. Removing
the running orphans also **detaches their data volumes**, so the Stage 3 volume prune afterward reclaims that disk —
usually the single biggest win (one observed sweep: 1,023 containers → 250 GB of volumes freed).

### Why they leak — testcontainers-rs has no Ryuk reaper

Unlike the Java/Go testcontainers libraries, the Rust `testcontainers` crate (0.27) ships **no Ryuk sidecar** —
`grep -rni ryuk` across the crate finds nothing. Its only cleanup path is the `ContainerAsync::drop` impl, which
issues `docker rm` when the handle is dropped. But `store::test_support` holds that handle in a `static OnceCell`
(`SharedPostgres._container`), and Rust does not run destructors for `static`s at process exit — so `drop` never
fires, `rm` is never issued, and every `cargo test` binary orphans one postgres container plus its volume. Across
many `cargo test --workspace` runs they pile up until the bridge fills. The `testcontainers.reuse.enable` hint in
that file's docstring does not prevent it: the reuse feature is not enabled and the code never calls `.with_reuse()`,
so each run starts a brand-new container regardless.

The durable fix belongs in `store/src/test_support.rs` (reap the container explicitly, or wire up real reuse), not in
this skill — until it lands, this sweep is the recurring maintenance.

## Stage 1 — unused images (safe)

```bash
docker system df          # before
docker image prune -a -f  # removes every image not used by a container
docker system df          # after
```

`-a` deletes untagged *and* unused-but-tagged images. On a laptop that runs many [[power-push]] iterations, the
Artifact Registry tags (`navigator-web:<short-sha>`) accumulate and this is usually the biggest win after build cache.

Images for *running* containers are skipped automatically — the two KIND workload images stay put.

## Stage 2 — build cache (safe)

```bash
docker builder prune -a -f
```

Removes every BuildKit cache layer. Next `cargo build` inside Docker / next `docker build` rebuilds from scratch (one
slow build), but nothing persistent is lost. On this workspace the cache routinely sits at 80–100 GB after a few days
of Rust image rebuilds — biggest single reclaim.

## Stage 3 — volumes (DESTRUCTIVE — confirm first)

```bash
docker volume prune -a -f
```

Removes every volume not attached to a *running* container. This includes:

- Postgres data volumes from stopped `testcontainers` runs — on this workspace this is the bulk of it, the detached
  remains of the Stage 0 orphans. Run Stage 0 *before* Stage 3: a leaked container's volume stays attached (and so is
  skipped) until that container is removed.
- Keycloak realm data from a stopped KIND cluster.
- Anything a stopped container was holding (caches, uploads, logs).

**Always ask before running stage 3.** If the user's KIND cluster is up and they're actively using it, the volumes
attached to those running pods are safe — the prune only touches detached volumes. But if the cluster is down, the
PV-backing volumes look unused to Docker and will be nuked.

If the user confirms but is unsure what's attached, list first:

```bash
docker volume ls --filter dangling=true
```

## Reporting

After every run, print the before/after table from `docker system df` so the user can see what was reclaimed at each
stage. Example:

```text
| Stage          | Before  | After   | Reclaimed |
| Orphans (S0)   | 1023 ct | 0 ct    | unblocks bridge + detaches volumes |
| Images         | 104.7GB | 1.77GB  | ~103 GB   |
| Build cache    | 100.9GB | 0 B     | ~101 GB   |
| Volumes        | 258.2GB | 8.2GB   | ~250 GB   |
```

Reclaimed totals belong in the closing message — the user is doing this to get disk back, so lead with the number.
