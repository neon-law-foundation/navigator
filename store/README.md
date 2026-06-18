# store

Data layer for the Navigator CRM. Owns the SeaORM entities, the migration history, the canonical seed loader, and the
`DbConfig` that resolves `DATABASE_URL` into a Postgres connection. The only crate that touches the schema; `web`,
`cli`, and `mcp` all consume it.

## Getting started

```bash
cargo test -p store
```

The suite spins up a per-binary Postgres container via `testcontainers` (see `src/test_support.rs`) and gives each test
a fresh `CREATE SCHEMA test_<id>`. **Docker is required** for any `cargo test` invocation in this workspace.

Set `testcontainers.reuse.enable = true` in `~/.testcontainers.properties` to keep the Postgres container alive between
iterative `cargo test` runs — without it, every invocation pays the cold-start cost (~3s).

To use the crate, depend on it and reach for `store::connect`, `store::migrate`, `store::seed::seed_canonical`, and
`store::entity::*`. There's nothing HTTP-related here — that's intentional, and it's why `cli` and `mcp` can stay lean.

Downstream crates that want the `pg()` helper enable the `test-support` feature in their dev-deps:

```toml
[dev-dependencies]
store = { workspace = true, features = ["test-support"] }
```

## What's next

When you add a new table, add the migration under `src/migration/m<date>_*.rs` and register it in `migration/mod.rs`,
then mirror the shape with an entity under `src/entity/`. Existing migration timestamps are an ordered history, not an
attempt at calendar accuracy — bump the date on the new one and you're done.

## Authorization columns

> **Role decides the tier; participation decides the scope.** Two columns answer "who can see what" — and they live in
> two different tables on purpose.

- `persons.role` is `TEXT` with `CHECK (role IN ('client','staff','admin'))`, modeled as a SeaORM `ActiveEnum` in
  [`src/entity/person.rs`](src/entity/person.rs). One row per person, **one role per row**.
- `person_project_roles` is the many-to-many between `persons` and `projects`; `participation` is free-form text
  (`attorney`, `paralegal`, `client`, `co_counsel`, …) so new matter-side roles arrive without a migration.
- `admin` bypasses project-scoping (sees every project); `client` and `staff` see only projects with a matching
  `person_project_roles` row.

Full narrative: [`../docs/access-model.md`](../docs/access-model.md).
