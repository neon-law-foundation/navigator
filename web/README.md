# web

The product. An axum HTTP server that serves the public Neon Law site, the foundation site, the admin CRUD UI, the
OAuth/OIDC dance, the inbound-email webhook, and the JSON API — all from one binary. Uses `store` for persistence,
`views` for HTML, `workflows` for the retainer-intake state machine, and `cloud` for object storage.

## Getting started

`web` requires Postgres — there is no SQLite fallback. The KIND dev loop pairs this binary on the host with the
in-cluster Postgres that `navigator start-dev-server` provisions:

```bash
navigator start-dev-server                       # KIND cluster + Postgres + Keycloak + OPA + …
set -a; source .devx/env; set +a  # exports DATABASE_URL + SENDGRID stubs
cargo run -p web              # binds :3001 by default; PORT overrides
```

On boot the server runs migrations, applies the canonical seed, loads the bundled workshops/marketing content from
`content/`, and starts serving. The same code path runs in production against Cloud SQL; only the `DATABASE_URL` and the
`NAVIGATOR_EMAIL_BACKEND` env (`sendgrid` in prod, unset locally so dev runs use `CapturingEmail`) differ.

## What's next

For local-Kubernetes work, run dependencies in KIND and `web` on the host — see `cli` and the RUNBOOK section on "host
runs `web`, deps in cluster." Routes live in `src/lib.rs::build_router`; handlers split by surface (`api.rs`,
`admin.rs`, `oauth.rs`, `inbound_email.rs`); the `AppState` struct wires the database, sessions, OAuth config, OPA
client, workflow runtime, signature provider, and object storage into every handler that needs them.

## Authorization model

> **Role decides the tier; participation decides the scope.** Every authenticated request carries a single role —
> `client`, `staff`, or `admin` — read from `persons.role` at callback time and stamped into the session cookie. `web`
> enforces it twice: OPA (sidecar) decides the URL tier; [`web::access::visible_projects`](src/access.rs) scopes the
> rows. Per-project visibility comes from `person_project_roles`, not from the role.

Full narrative + Rego rules: [`docs/access-model.md`](../docs/access-model.md). Login flow that stamps `role` into the
session: [`docs/oidc.md`](../docs/oidc.md).
