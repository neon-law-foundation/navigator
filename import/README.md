# import

The shared bulk contact-import engine — the one library behind every surface that turns a list of organizations and the
people who work at them into `entities`, `persons`, and the `person_entity_roles` links between them.

Mirrors the `rules` crate's shape: one library, many callers. The `cli` (`import-contacts`), the `aida_bulk_import` MCP
tool, and the `web` upload route all parse a `Payload`, run `validate` for structural diagnostics, then `apply` it
against the database. Nothing here is surface-specific. The contract is documented for humans and LLMs in
[`docs/bulk-contact-import.md`](../docs/bulk-contact-import.md).

## What it provides

- `parse(&str) -> Payload` — parse the version-1 JSON contract (`SUPPORTED_VERSION`).
- `validate(&Payload) -> Vec<Diagnostic>` — structural diagnostics (`Severity::Error` blocks the load) plus
  `canonical_url` normalization, all before any write.
- `apply(&db, &Payload) -> ImportReport` — the find-or-create load, with a per-row `RowOutcome` (`Created` / `Updated` /
  `Unchanged`) and a `summary()`.

## Idempotency

Every write is find-or-create, so an import is always safe to retry:

- People dedupe on the unique `persons.email`.
- Organizations dedupe on `(name, entity_type, jurisdiction)`.
- Links dedupe on `(person, entity, role)`.

The JSON is authoritative: a re-run overwrites `name` / `title` / `phone` from the file, but **never** a person's `role`
— promotions are sticky. Re-running an unchanged payload is a no-op that reports `unchanged`.

## Getting started

```bash
# Contract parsing + validation diagnostics + idempotent apply against a testcontainers Postgres.
cargo test -p import
```
