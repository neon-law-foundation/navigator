# AGENTS.md

General workspace rules, architecture invariants, and the "how to work" guide live in [`CLAUDE.md`](CLAUDE.md) and
[`docs/`](docs/) — read those first. This file only adds notes specific to the Cursor Cloud agent VM.

## Cursor Cloud specific instructions

A committed [`.cursor/environment.json`](.cursor/environment.json) + [`.cursor/Dockerfile`](.cursor/Dockerfile) define
the agent base image, so build + lint + test work out of the box. The image bakes the pinned Rust 1.95.0 toolchain
(rustfmt + clippy), the native build deps (`build-essential`, `pkg-config`, `libssl-dev`, `libpq-dev`,
`protobuf-compiler`), and a local PostgreSQL seeded with a superuser role/db `navigator` (password `navigator`). On each
boot `install` runs `cargo fetch` and `start` runs `sudo service postgresql start`; `TEST_DATABASE_URL` is preset in the
image. Editing the Dockerfile triggers an image rebuild on the *next* agent — it does not change a running agent.

So the standard gate (see [`CLAUDE.md`](CLAUDE.md) and [`docs/test-database.md`](docs/test-database.md)) runs directly:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # TEST_DATABASE_URL already targets the baked local Postgres
```

`cargo test` creates a per-run `test_<id>` schema against that one server, so there is no per-binary testcontainer.

### Running the full `web` app end-to-end (extra setup)

`web` calls `enforce_prod_invariants` unconditionally and needs OIDC (Keycloak), OPA, and a GCS-compatible store on top
of Postgres. The documented KIND loop (`cargo run -p cli -- start-dev-server`, see [`docs/RUNBOOK.md`](docs/RUNBOOK.md))
**does not work on this VM**: the KIND node's `systemd` cannot init its cgroup under the `fuse-overlayfs` storage driver
(`Structure needs cleaning`), and there is no host `systemd`/`modprobe`. Run the same dependency images as standalone
containers instead. **Docker is not in the base image**, so install it first (Docker engine, then
`/etc/docker/daemon.json` with `storage-driver: fuse-overlayfs` + `containerd-snapshotter: false`, `iptables-legacy`),
and start the daemon by hand (no systemd):

```bash
sudo dockerd > /tmp/dockerd.log 2>&1 &   # run in a tmux session so it survives
sudo chmod 666 /var/run/docker.sock      # the shell predates the docker group membership
```

Then bring up the four deps (configs derive from the in-repo manifests — `k8s/overlays/kind/deps/keycloak.yaml` realm
JSON, `k8s/base/opa/opa.yaml` rego, fake-gcs just needs a `navigator` bucket dir). The local Postgres baked into the
image already serves `web` too; point `DATABASE_URL` at it:

```bash
# OPA: write k8s/base/opa/opa.yaml's `navigator.rego` to ./opa/navigator.rego, then:
docker run -d --name nav-opa -p 8181:8181 -v "$PWD/opa":/policies:ro \
  openpolicyagent/opa:latest run --server --addr=:8181 --watch /policies/navigator.rego

# fake-gcs: a top-level subdir under /data becomes a bucket
mkdir -p ./fakegcs/navigator
docker run -d --name nav-fakegcs -p 30443:4443 -v "$PWD/fakegcs":/data \
  fsouza/fake-gcs-server:latest -scheme http -port 4443 -public-host localhost:30443

# Keycloak: write keycloak.yaml's realm JSON to ./keycloak/navigator-realm.json, then:
docker run -d --name nav-keycloak -p 30080:8080 \
  -e KEYCLOAK_ADMIN=admin -e KEYCLOAK_ADMIN_PASSWORD=admin -e KC_HTTP_ENABLED=true \
  -e KC_HOSTNAME_STRICT=false -e KC_HOSTNAME=http://localhost:30080/keycloak \
  -e KC_HOSTNAME_BACKCHANNEL_DYNAMIC=true -e KC_HTTP_RELATIVE_PATH=/keycloak \
  -v "$PWD/keycloak":/opt/keycloak/data/import:ro \
  quay.io/keycloak/keycloak:25.0 start-dev --import-realm
```

**Non-obvious `web` boot gotcha:** the invariants require three vars that even the KIND `.devx/env` omits (NeonLaw ships
them via Doppler) — `SENDGRID_EVENTS_SECRET`, `SENDGRID_EVENTS_PUBLIC_KEY`, and `DOCUSIGN_HMAC_KEY`. With no Doppler,
set stub values (plus `SESSION_SECRET` ≥ 32 bytes, `SENDGRID_API_KEY`, `SENDGRID_INBOUND_SECRET`) in a gitignored `.env`
or `web` crash-loops at boot. Standalone Keycloak is simpler than KIND: there is no browser-vs-cluster hostname split,
so a single `KC_HOSTNAME` of `http://localhost:30080/keycloak` serves both the frontchannel and the backchannel. The
`.env` points `web` at these: `DATABASE_URL=postgres://navigator:navigator@127.0.0.1:5432/navigator`,
`NAVIGATOR_STORAGE_BACKEND=gcs` + `NAVIGATOR_STORAGE_ENDPOINT=http://localhost:30443`,
`NAVIGATOR_OPA_URL=http://localhost:8181`, `OAUTH_ISSUER_URL=http://localhost:30080/keycloak/realms/navigator`,
`OAUTH_REDIRECT_URI=http://localhost:3001/auth/callback`, and `RESTATE_BROKER_URL` to any URL (dialed lazily — only a
workflow dispatch needs a real broker, which this path does not run). Then `cargo run -p web` listens on `:3001`.

### Login + authz

Keycloak realm `navigator` ships one user: `staff` / `staff`. First Keycloak login prompts for a last name (the realm
import omits it). The authz tier is DB-sourced (`persons.role`), not from Keycloak — a person is created as `client` on
first login. To reach `/portal/admin/*`, pre-seed or promote the row to `staff`/`admin` (see
[`docs/RUNBOOK.md`](docs/RUNBOOK.md) §3), e.g. `UPDATE persons SET role='staff' WHERE email='staff@neonlaw.com';`
(re-login to refresh the session role).
