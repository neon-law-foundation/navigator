---
name: rust-sea-orm
description: >
  SeaORM 1.x entity, ActiveModel, transaction, and migration patterns for `web`. Trigger when adding or modifying an
  entity, writing a `sea-orm-migration` migration, opening a transaction, choosing a column type, or wiring
  `DatabaseConnection` into a new crate. Also trigger before pulling in a different ORM (Diesel, sqlx-bare) — we
  standardize on SeaORM (sqlx underneath, runtime-tokio-rustls).
---

# SeaORM 1.x patterns in the navigator workspace

Driver is `sqlx` with `runtime-tokio-rustls`. Only `sqlx-postgres` is enabled — the SQLite backend was removed in the SQLite cutover. Tests spin up a real Postgres via `testcontainers` (`store::test_support::pg`).

## Entities

- One module per table, named for the table singular (`person`, `template`, `question`). The `Entity`, `Model`, `ActiveModel`, `Column`, `Relation` items live inside.
- Generate from schema with `sea-orm-cli generate entity` for the first cut, then commit the file and edit it by hand. Don't regenerate over hand edits — `migrate` is the source of truth, the entity is the typed surface.
- Add `#[sea_orm(unique)]`, `#[sea_orm(indexed)]`, and FK declarations directly on the `Column` enum. Document non-obvious column semantics with a one-line `///` doc.
- `Model` is the read shape, `ActiveModel` is the write shape. Construct an `ActiveModel` with `..Default::default()` and only `Set(value)` the fields you mean to write — the rest stay `NotSet` and the DB default applies.

## Reads

- Prefer `Entity::find_by_id(id).one(&db).await?` for primary-key lookups.
- `Entity::find().filter(Column::Foo.eq(value)).one(&db).await?` for one row.
- `…all(&db).await?` for the full set; **never** for paginated user-facing data — use `paginator(&db, page_size).fetch_page(n)`.
- For joins, prefer `find_related` / `find_with_related` over hand-written `JoinType::LeftJoin`. Hand joins go through `QuerySelect::join` when the relationship isn't expressible as a `Relation`.

## Writes + transactions

- Single-row insert/update: `ActiveModel::insert(&db)` / `.update(&db)`.
- Multi-row mutation, anything that must be atomic: wrap in `db.transaction::<_, _, DbErr>(|txn| Box::pin(async move { … })).await?`.
- `transaction` rolls back on `Err` and on panic; commit happens implicitly on `Ok`.
- Don't `await` long external IO inside a transaction — hold the txn for as little wall-clock as possible.

## Migrations (`sea-orm-migration`)

- Migrations live in `web/src/migrator/`, one file per migration, named `m<YYYYMMDD>_<HHMMSS>_<slug>.rs`.
- `MigrationName::name(&self)` returns the file basename. The struct's name and the registration order in `Migrator::migrations()` are how SeaORM tracks state.
- Always provide a real `down()` — the test suite runs `Migrator::down` against a fresh DB. If a step is genuinely irreversible (data deletion), make `down()` an error with an explanation.
- Use `Table::create` / `Table::alter` / `Index::create` builders — never raw SQL via `manager.get_connection().execute_unprepared(...)` unless the DDL truly can't be expressed in the builder (then comment why).
- Both backends share migration code. Avoid backend-specific SQL; prefer types that round-trip (`ColumnType::Text`, `ColumnType::Integer`, `ColumnType::TimestampWithTimeZone`).

## Column type conventions

| Domain | SeaORM type | Notes |
|---|---|---|
| Identifier | `ColumnType::Uuid` or `ColumnType::Integer` autoincrement | UUID for externally-visible IDs, integer for opaque internal. |
| Timestamp | `ColumnType::TimestampWithTimeZone` | Always tz-aware. Store UTC, render in caller's zone. |
| Money | `ColumnType::Decimal(Some((18, 2)))` | Never float for currency. |
| Enum-like string | `ColumnType::String(Some(32))` + a Rust enum with `#[derive(EnumIter, DeriveActiveEnum)]` | Compile-checked at the entity layer. |

## Connection

- One `DatabaseConnection` (alias of `Arc<DatabaseConnectionImpl>`) per process, cloned freely. Don't open a new connection per request.
- Pool size is set on the `ConnectOptions` builder before `Database::connect`. Postgres handles concurrent writers — tune `.max_connections(N)` based on workload, not driver constraints.
- `DATABASE_URL` is the only selector. `store::DbConfig::from_env` returns `Err(MissingDatabaseUrl)` if unset; there is no fallback. The KIND `navigator start-dev-server` env file sets it for host-side `cargo run -p web`.

## Testing

- Tests use `store::test_support::pg()` — one Postgres container per `cargo test` binary, fresh `CREATE SCHEMA test_<id>` per test. Migrations apply automatically inside that schema. Never share DB state across tests; the schema-per-test gives you isolation without the container churn cost.
- Enable container reuse in `~/.testcontainers.properties` (`testcontainers.reuse.enable = true`) so iterative `cargo test` doesn't pay the cold-start cost.

## Anti-patterns

- Reaching for raw `sqlx::query!` because "SeaORM doesn't support X" — usually it does via `Statement::from_string`. If it really doesn't, add a typed helper, don't sprinkle raw SQL across handlers.
- Returning `Vec<Model>` from a paginated endpoint without a `LIMIT`.
- Mixing schema changes into a feature commit — migrations stand alone so a `down` is testable in isolation.

## Canonical sources

- SeaORM docs: <https://docs.rs/sea-orm>
- SeaORM site (tutorials, recipes): <https://www.sea-ql.org/SeaORM/>
- SeaORM repository: <https://github.com/SeaQL/sea-orm>
- `sea-orm-migration` docs: <https://docs.rs/sea-orm-migration>
- sqlx (underlying driver): <https://github.com/launchbadge/sqlx>
- Postgres docs (authoritative for prod behavior): <https://www.postgresql.org/docs/current/>
