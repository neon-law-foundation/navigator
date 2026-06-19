# AGENTS.md

General workspace rules, architecture invariants, and the "how to work" guide live in [`CLAUDE.md`](CLAUDE.md) and
[`docs/`](docs/) — read those first. This file only adds notes specific to the Cursor Cloud agent VM.

## Cursor Cloud specific instructions

The standard local loop is `cargo run -p cli -- start-dev-server` (KIND), documented in
[`docs/RUNBOOK.md`](docs/RUNBOOK.md). **That KIND path does not work on the Cursor Cloud VM**: the cluster create fails
because the KIND node's `systemd` cannot initialize its cgroup under the `fuse-overlayfs` Docker storage driver (it dies
with a `Structure needs cleaning` error while creating `/init.scope`). There is no `systemd` on the host and kernel
modules cannot be loaded (`modprobe` is absent). Use the standalone-container path below instead — it runs the exact
same dependency images (Postgres, Keycloak, OPA, fake-gcs) the KIND deps overlay defines.

### Docker (no systemd — start it yourself)

Docker is installed but there is no init system, so `dockerd` must be started by hand once per session (the update
script must not do this). It is configured with `fuse-overlayfs` + `containerd-snapshotter: false` (required on this
kernel) in `/etc/docker/daemon.json`:

```bash
sudo dockerd > /tmp/dockerd.log 2>&1 &   # run in a tmux session so it survives
sudo chmod 666 /var/run/docker.sock      # the shell predates the docker group membership
```

### Tests / lint / build

Standard commands (see [`docs/test-database.md`](docs/test-database.md)). Tests need one shared Postgres; point
`TEST_DATABASE_URL` at a container instead of spawning a testcontainer per binary:

```bash
docker run -d --name test-pg --shm-size=1g -e POSTGRES_USER=navigator \
  -e POSTGRES_PASSWORD=navigator -e POSTGRES_DB=navigator -p 5432:5432 \
  postgres:17-alpine -c max_connections=300
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=postgres://navigator:navigator@127.0.0.1:5432/navigator cargo test --workspace
```

### Running `web` against standalone deps

`web` calls `enforce_prod_invariants` unconditionally, so it needs the full env set even in dev. `.devx/env` (KIND) does
not exist here, so use a gitignored `.env`. **Non-obvious gotcha:** even the KIND `.devx/env` is missing three vars that
the invariants require — `SENDGRID_EVENTS_SECRET`, `SENDGRID_EVENTS_PUBLIC_KEY`, and `DOCUSIGN_HMAC_KEY` (NeonLaw
supplies these via Doppler). With no Doppler, set stub values for them in `.env` or `web` crash-loops at boot.

Bring up the four dependency containers (configs derive from the in-repo manifests —
`k8s/overlays/kind/deps/keycloak.yaml` realm JSON, `k8s/base/opa/opa.yaml` rego, fake-gcs just needs a `navigator`
bucket dir):

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

Standalone Keycloak is simpler than KIND: there is no browser-vs-cluster hostname split, so the single `KC_HOSTNAME`
value `http://localhost:30080/keycloak` serves both the frontchannel and the backchannel. `.env` then points `web` at
these: `DATABASE_URL` → `:5432`, `NAVIGATOR_STORAGE_BACKEND=gcs` + `NAVIGATOR_STORAGE_ENDPOINT=http://localhost:30443`,
`NAVIGATOR_OPA_URL=http://localhost:8181`, `OAUTH_ISSUER_URL=http://localhost:30080/keycloak/realms/navigator`,
`OAUTH_REDIRECT_URI=http://localhost:3001/auth/callback`, plus `RESTATE_BROKER_URL` to any URL (it is dialed lazily —
only a workflow dispatch needs a real broker, which the standalone path does not run). Then `cargo run -p web` listens
on `:3001`.

### Login + authz

Keycloak realm `navigator` ships one user: `staff` / `staff`. First Keycloak login prompts for a last name (the realm
import omits it). The authz tier is DB-sourced (`persons.role`), not from Keycloak — a person is created as `client` on
first login. To reach `/portal/admin/*`, pre-seed or promote the row to `staff`/`admin` (see
[`docs/RUNBOOK.md`](docs/RUNBOOK.md) §3), e.g. `UPDATE persons SET role='staff' WHERE email='staff@neonlaw.com';`
(re-login to refresh the session role).
