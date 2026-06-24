//! Shared test infrastructure for every workspace crate that
//! exercises the `store` schema.
//!
//! # Why this exists
//!
//! Before the SQLite cutover, tests opened
//! `DbConfig::Sqlite { path: ":memory:" }` and got a fresh database
//! per test. SQLite is gone; the only supported backend is Postgres.
//! Spinning up a full Postgres process per `#[tokio::test]` would be
//! slow enough to push the whole workspace's `cargo test` past the
//! 60-second budget the SQLite-cutover plan sets.
//!
//! # The pattern
//!
//! There is exactly ONE Postgres server for the whole `cargo test`
//! run, resolved once per process by [`shared_postgres`]. Each test
//! then gets its own isolated namespace via `CREATE SCHEMA
//! test_<id>` plus a `search_path` override in the connection URL.
//! Migrations run inside the new schema, so two tests never collide
//! on schema mutations, foreign keys, or unique constraints — the
//! schema-per-test isolation is what lets the suite run in parallel.
//!
//! Where that one server comes from is a small hybrid (see
//! `docs/test-database.md`):
//!
//! - **`TEST_DATABASE_URL` set** → connect to that already-running
//!   Postgres and create per-test schemas on it. No container is
//!   spawned or leaked. This is the CI path (a `docker run` Postgres)
//!   and the "reuse my `navigator start-dev-server` Postgres at `localhost:15432`" path.
//! - **`TEST_DATABASE_URL` unset** → spin up ONE
//!   [`ReuseDirective::Always`] container, labeled
//!   `org.navigator.test-postgres=shared`. Every test binary in the
//!   run finds and shares that single container instead of starting
//!   its own, and later `cargo test` invocations reuse it too. This
//!   keeps `cargo test` zero-config on any laptop with Docker.
//!
//! A reuse-marked container is deliberately NOT reaped on drop, so
//! it survives between runs. Reclaim it with:
//!
//! ```sh
//! docker rm -f $(docker ps -aq \
//!   --filter label=org.navigator.test-postgres=shared)
//! ```
//!
//! # Image pinning
//!
//! Pinned by digest. testcontainers 0.27 has no `with_image_digest`
//! helper, but Docker accepts `name:tag@sha256:…` and treats the
//! digest as authoritative (the tag stays for human readability).
//! Refresh [`POSTGRES_DIGEST`] in the same PR that bumps the tag —
//! a stale digest fails the pull loudly instead of silently letting
//! a tampered image through.
//!
//! Re-fetch the live digest with:
//!
//! ```sh
//! docker buildx imagetools inspect postgres:17-alpine | grep Digest
//! ```

use std::sync::Arc;

use sea_orm::{ConnectOptions, ConnectionTrait, Database, Statement};
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt, ReuseDirective,
};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::Db;

const POSTGRES_IMAGE: &str = "postgres";
/// `17-alpine` plus the index digest at the time of the SQLite
/// cutover. Docker resolves `tag@sha256:…` by digest; the tag stays
/// for human readability in `docker ps` output.
const POSTGRES_TAG: &str =
    "17-alpine@sha256:979c4379dd698aba0b890599a6104e082035f98ef31d9b9291ec22f2b13059ca";
const POSTGRES_USER: &str = "navigator";
const POSTGRES_PASSWORD: &str = "navigator";
const POSTGRES_DB: &str = "navigator";

/// When set, point every test at this already-running Postgres and
/// create per-test schemas there instead of spawning a container. CI
/// sets it to a `docker run` Postgres; locally it can point at the
/// `navigator start-dev-server` KIND Postgres (`localhost:15432`) or any other server.
const TEST_DATABASE_URL_ENV: &str = "TEST_DATABASE_URL";

/// Stable label on the one reuse-shared container, so every test
/// binary (and every later `cargo test` run) finds the same one and
/// `docker` can prune it by selector.
const REUSE_LABEL_KEY: &str = "org.navigator.test-postgres";
const REUSE_LABEL_VALUE: &str = "shared";

/// Per-test connection-pool ceiling. With one shared server backing
/// the whole run (instead of a container per binary), unbounded pools
/// would exhaust the server's `max_connections`. Each test touches the
/// DB with one or two concurrent queries, so a small ceiling is ample
/// and keeps total connections well under the server limit.
const TEST_POOL_MAX_CONNECTIONS: u32 = 5;

struct SharedPostgres {
    /// `Some` when this process spawned the reuse-labeled container;
    /// `None` when connecting to an external `TEST_DATABASE_URL`.
    /// Held only to keep a spawned container alive for the life of the
    /// process — a reuse-marked container is not reaped on drop anyway.
    _container: Option<ContainerAsync<GenericImage>>,
    base_url: String,
}

static POSTGRES: OnceCell<Arc<SharedPostgres>> = OnceCell::const_new();

async fn shared_postgres() -> Arc<SharedPostgres> {
    POSTGRES
        .get_or_init(|| async {
            // External Postgres: skip Docker entirely. This is the CI
            // path and the "reuse my `navigator start-dev-server` Postgres" path — nothing
            // is spawned, so nothing can leak.
            if let Some(base_url) = std::env::var(TEST_DATABASE_URL_ENV)
                .ok()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
            {
                return Arc::new(SharedPostgres {
                    _container: None,
                    base_url,
                });
            }

            // Zero-config: ONE reuse-labeled container shared by every
            // test binary in this run and reused by later runs.
            // `ReuseDirective::Always` makes testcontainers find the
            // labeled container instead of starting a fresh one, and
            // never reap it on drop.
            let container = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
                .with_exposed_port(5432.tcp())
                .with_wait_for(WaitFor::message_on_stderr(
                    "database system is ready to accept connections",
                ))
                .with_env_var("POSTGRES_USER", POSTGRES_USER)
                .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
                .with_env_var("POSTGRES_DB", POSTGRES_DB)
                .with_label(REUSE_LABEL_KEY, REUSE_LABEL_VALUE)
                // Docker's default 64 MB `/dev/shm` is far too small for the
                // whole `cargo test --workspace` run hitting one shared
                // server: each parallel test schema's migration allocates
                // ~32 MB dynamic-shared-memory segments, and under the
                // machine's full test parallelism dozens land at once,
                // exhausting the segment and failing migrations with
                // `53100: No space left on device`. Give the server 1 GiB of
                // shared memory (RAM-backed, reclaimed with the container) so
                // the suite runs green at any parallelism — locally, in KIND,
                // and in CI.
                .with_shm_size(1024 * 1024 * 1024)
                .with_reuse(ReuseDirective::Always)
                .start()
                .await
                .expect("postgres container should start");
            let host = container.get_host().await.expect("postgres container host");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("postgres container port");
            let base_url = format!(
                "postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@{host}:{port}/{POSTGRES_DB}"
            );
            Arc::new(SharedPostgres {
                _container: Some(container),
                base_url,
            })
        })
        .await
        .clone()
}

fn short_id() -> String {
    // Per-process monotonic counter avoids the time-prefix collision
    // that bites when tokio runs tests in parallel — every UUIDv7
    // generated in the same millisecond shares its leading hex
    // characters. The full UUID is 32 hex chars; the last 12 carry
    // enough entropy to keep schema names distinct, but using the
    // counter alongside the timestamp is what guarantees no two
    // schemas in this process ever collide.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = Uuid::now_v7().simple().to_string();
    format!("{:08x}{}", n, &id[id.len() - 8..])
}

/// A scoped per-test Postgres handle: both the connected `Db` and
/// the raw URL the caller can hand to a subprocess (e.g. an
/// `assert_cmd`-spawned CLI binary). The schema is alive as long as
/// the underlying container is, which is the life of the process —
/// see [`shared_postgres`].
#[derive(Clone)]
pub struct Schema {
    pub db: Db,
    pub url: String,
    pub name: String,
}

/// Create a new per-test schema inside the shared Postgres
/// container, run migrations, and hand back the connected `Db` plus
/// the URL needed to talk to that same schema from a subprocess.
pub async fn schema() -> Schema {
    let shared = shared_postgres().await;
    let mut admin_opts = ConnectOptions::new(shared.base_url.clone());
    admin_opts
        .max_connections(2)
        .min_connections(0)
        .sqlx_logging(false);
    let admin = Database::connect(admin_opts)
        .await
        .expect("connect to per-process postgres");

    let name = format!("test_{}", short_id());
    admin
        .execute(Statement::from_string(
            admin.get_database_backend(),
            format!("CREATE SCHEMA \"{name}\""),
        ))
        .await
        .expect("create per-test schema");
    let _ = admin.close().await;

    let url = format!("{}?options=-c%20search_path%3D{name}", shared.base_url);
    let mut opts = ConnectOptions::new(url.clone());
    opts.max_connections(TEST_POOL_MAX_CONNECTIONS)
        .min_connections(0)
        .sqlx_logging(false);
    let db = Database::connect(opts)
        .await
        .expect("connect to per-test schema");

    crate::migrate(&db).await.expect("migrate per-test schema");
    Schema { db, url, name }
}

/// Return a fresh `Db` pointed at an empty, fully-migrated schema
/// inside the per-process shared Postgres container.
///
/// Each call creates a new `test_<uuid>` schema, runs migrations
/// inside it, and configures the connection's `search_path` so every
/// table reference resolves to the new schema. Two concurrent tests
/// therefore see independent tables without paying for a new
/// container each time.
pub async fn pg() -> Db {
    schema().await.db
}

/// Find-or-create a minimal entity (with its jurisdiction + entity type)
/// and return its id. `projects.entity_id` is `NOT NULL`, so every test
/// that inserts a project needs a pre-existing entity to open against;
/// this is the one-liner that supplies it.
pub async fn seed_entity(db: &Db) -> Uuid {
    use crate::entity::{entity as entities, entity_type, jurisdiction};
    use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};

    let jur_id = match jurisdiction::Entity::find()
        .filter(jurisdiction::Column::Code.eq("TS"))
        .one(db)
        .await
        .expect("jurisdiction lookup")
    {
        Some(j) => j.id,
        None => {
            jurisdiction::ActiveModel {
                name: ActiveValue::Set("Test State".into()),
                code: ActiveValue::Set("TS".into()),
                ..Default::default()
            }
            .insert(db)
            .await
            .expect("seed jurisdiction")
            .id
        }
    };
    let type_id = match entity_type::Entity::find()
        .filter(entity_type::Column::Name.eq("Test Org"))
        .one(db)
        .await
        .expect("entity_type lookup")
    {
        Some(t) => t.id,
        None => {
            entity_type::ActiveModel {
                name: ActiveValue::Set("Test Org".into()),
                ..Default::default()
            }
            .insert(db)
            .await
            .expect("seed entity_type")
            .id
        }
    };
    entities::ActiveModel {
        name: ActiveValue::Set(format!("Test Entity {}", Uuid::now_v7())),
        entity_type_id: ActiveValue::Set(type_id),
        jurisdiction_id: ActiveValue::Set(jur_id),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed entity")
    .id
}

/// Find-or-create a single throwaway person to satisfy a project's
/// required `staff_dri_person_id` / `client_dri_person_id` foreign keys
/// in tests. Both columns are `NOT NULL`, so every test that inserts a
/// `project::ActiveModel` needs a real `persons.id` for each side; tests
/// that don't exercise DRI semantics just point both columns at this one
/// fixture row. Idempotent (keyed on a fixed email) so repeated calls in
/// one test return the same id without a unique-violation.
pub async fn dri_person(db: &Db) -> Uuid {
    use crate::entity::person;
    use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};

    const EMAIL: &str = "dri-fixture@test.invalid";
    if let Some(existing) = person::Entity::find()
        .filter(person::Column::Email.eq(EMAIL))
        .one(db)
        .await
        .expect("dri_person lookup")
    {
        return existing.id;
    }
    person::ActiveModel {
        name: ActiveValue::Set("DRI Fixture".into()),
        email: ActiveValue::Set(EMAIL.into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed dri fixture person")
    .id
}

/// Seed one notation (with its template, person, and project) and
/// return the notation id. Shared by the helper-module tests that need
/// a matter to hang rows off (`review_documents`, `document_comments`).
pub async fn seed_notation(db: &Db) -> Uuid {
    use crate::entity::{notation, person, project, template};
    use sea_orm::{ActiveModelTrait, ActiveValue};

    let entity_id = seed_entity(db).await;

    let tmpl = template::ActiveModel {
        code: ActiveValue::Set("onboarding__estate".into()),
        title: ActiveValue::Set("Estate Plan".into()),
        respondent_type: ActiveValue::Set("person".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed template");
    let person = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed person");
    let dri = dri_person(db).await;
    let proj = project::ActiveModel {
        name: ActiveValue::Set("Libra estate plan".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(entity_id),
        staff_dri_person_id: ActiveValue::Set(Some(dri)),
        client_dri_person_id: ActiveValue::Set(Some(dri)),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed project");
    notation::ActiveModel {
        template_id: ActiveValue::Set(tmpl.id),
        person_id: ActiveValue::Set(person.id),
        entity_id: ActiveValue::Set(None),
        project_id: ActiveValue::Set(proj.id),
        state: ActiveValue::Set("BEGIN".into()),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("seed notation")
    .id
}
