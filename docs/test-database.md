# Test database — one Postgres for the whole run

One quotable sentence:

> **`cargo test` uses one Postgres — an external `TEST_DATABASE_URL` if you set one, otherwise a single
> reuse-labeled container shared by every test binary — never one container per binary.**

## The problem this fixes

`cargo test --workspace` runs ~50 test binaries. The old `store::test_support` started **one Postgres container per
binary** and held the handle in a process-lifetime `static`, so a full run tried to start dozens of `postgres:17-alpine`
containers at once and then **leaked** them (testcontainers-rs has no Ryuk reaper, so the `Drop` never fired). That
exhausted the Docker daemon — `WaitContainer(StartupTimeout)`, `bridge docker0 … exchange full` — and filled the disk
that [`agent-workflows.md`](agent-workflows.md) covers under maintenance cleanup.

The schema-per-test isolation was never the problem: within a binary, all tests already shared one container and each
test got `CREATE SCHEMA test_<id>` + a `search_path` override. The waste was purely that **every binary started its own
server**.

## The decision

We keep two test tiers and only change tier 1's Postgres provisioning:

- **Tier 1 — `cargo test` (unit + BDD).** Postgres is the only real dependency; everything else is trait-stubbed
  in-process (OPA → `PolicyClient::passthrough`, Keycloak → `wiremock`, Restate → `InMemoryRuntime`, GCS → `FsStorage`,
  SendGrid/DocuSign → `wiremock`, Xero → `StubBillingProvider`, agent router → `NullRouter`). This is what keeps the
  suite fast and runnable on any laptop.
- **Tier 2 — KIND e2e (`navigator start-dev-server`, `navigator e2e`, the per-PR browser suite).** The full real
  stack — Keycloak, OPA, Restate, fake-gcs — runs as in-cluster containers. This is where auth, authz, and durable
  workflows are exercised end to end.

We deliberately do **not** pull Keycloak/OPA/OTel/Restate into `cargo test`. The trait seams are the contract boundary;
real sidecars in unit tests would add wall-clock and flakiness, not coverage, and would raise the floor for a first-time
contributor from "Docker" to "a full KIND cluster."

For tier 1's one Postgres we picked the **hybrid (C-lean)**: honor `TEST_DATABASE_URL` if set, otherwise spawn one
reuse-labeled container. This keeps `cargo test` zero-config locally, gives CI a clean external server, and lets a
contributor who already ran `navigator start-dev-server` point tests at the KIND Postgres — one mechanism, three
backends.

### Reusing the KIND Postgres, or a dedicated container

Either, your choice — that is the point of the env seam. Nothing is auto-wired to the KIND Postgres (so even a bare
`cargo test` never depends on `navigator start-dev-server`), but `TEST_DATABASE_URL` can point at it when you want one
Postgres for both dev-run and tests. Schema-per-test isolation means tests create their own `test_<id>` schemas and
never pollute the dev data, even when they share that server.

## The env contract

- **`TEST_DATABASE_URL` unset or empty** → `store::test_support` spawns ONE `ReuseDirective::Always` container, labeled
  `org.navigator.test-postgres=shared`; every binary in the run and every later run finds and reuses it.
- **`TEST_DATABASE_URL` set** (a CI `docker run` server, or `…@localhost:15432` from `navigator start-dev-server`) →
  connect to that server and create per-test schemas there. No container is spawned, so nothing leaks.

In both cases each test still gets `CREATE SCHEMA test_<id>` + a `search_path` override (unchanged), so tests run in
parallel safely.

## What changed

- **`store/src/test_support.rs`** — `shared_postgres()` first reads `TEST_DATABASE_URL`; if absent it spawns a single
  `ReuseDirective::Always` container (stable label, not reaped on drop) instead of a fresh one per binary.
  `SharedPostgres::_container` became `Option<…>` (`None` on the external path). Per-test SeaORM pools are capped
  (`max_connections = 5`, admin = 2) because one server now backs the whole run.
- **`Cargo.toml`** — `testcontainers` gains the `reusable-containers` feature (the `ReuseDirective` API).
- **`.github/workflows/ci.yml`** — the `rust`, `coverage`, and `cucumber-bdd` jobs each `docker run` one Postgres (with
  `-c max_connections=300`, since all binaries share it) and export
  `TEST_DATABASE_URL=postgres://navigator:navigator@127.0.0.1:5432/navigator`. The old "prime testcontainers Postgres
  image" steps are gone — CI no longer touches testcontainers at all. The KIND `e2e` job (Stage 3) is unchanged; its
  cli-interop step keeps its own throwaway Postgres.

## How a contributor runs tests

- **Zero setup** (any laptop with Docker): `cargo test -p store` — the first run starts one shared container; it
  persists and every later run, in any crate, reuses it. Reclaim it any time with:

  ```sh
  docker rm -f $(docker ps -aq --filter label=org.navigator.test-postgres=shared)
  ```

- **Bring your own Postgres** (a CI-style `docker run` server, or any reachable Postgres) — no testcontainers in the
  test path:

  ```sh
  export TEST_DATABASE_URL=postgres://navigator:navigator@127.0.0.1:5432/navigator
  cargo test
  ```

- **Reuse the KIND Postgres from `navigator start-dev-server`** — one Postgres for both your dev-run and your tests:

  ```sh
  export TEST_DATABASE_URL=postgres://navigator:navigator@localhost:15432/navigator
  cargo test
  ```

  Schema-per-test isolation means the suite creates its own `test_<id>` schemas and never pollutes the dev data, even
  though it shares the server. **Caveat:** this path depends on `navigator start-dev-server` being live and the
  `localhost:15432` port-forward being up — if the cluster is down or the forward has dropped, tests hang or fail on
  connect. That is the trade-off for sharing one server; the zero-config default above has no such dependency. Unset
  `TEST_DATABASE_URL` to fall back to the self-contained container.

## How CI runs them

Each test job starts one `docker run` Postgres, waits for `pg_isready`, exports `TEST_DATABASE_URL`, and then runs
`cargo test`, `cargo llvm-cov`, and the cucumber suite against it. No testcontainers, no per-binary proliferation, no
leak. The full real stack is still exercised by the KIND `e2e` job, which now runs the full browser + accessibility
suite on every PR.

## Cold-start note

On a cold zero-config `cargo test --workspace` (no `TEST_DATABASE_URL`, no pre-existing shared container), the first few
binaries can race to create the labeled container before one wins and the rest reuse it — so a cold run may briefly
create a small handful of containers rather than exactly one. They are reuse-marked (not leaked), every later run
settles to the single shared one, and CI avoids the race entirely by setting `TEST_DATABASE_URL`. To guarantee exactly
one locally, set `TEST_DATABASE_URL` (point it at any Postgres, including `navigator start-dev-server`'s).
