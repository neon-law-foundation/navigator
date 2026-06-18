//! Workspace timestamp convention — schema-level guards.
//!
//! Every table in the workspace must carry **`inserted_at`** and
//! **`updated_at`** columns (RFC 3339 timestamps set by the
//! `uuid_active_model_behavior!` macro on insert / update). The
//! columns are non-negotiable: a future migration that forgets one
//! breaks one of these tests.
//!
//! `created_at` is forbidden. The two names — `created_at` and
//! `inserted_at` — would silently overlap if both were allowed,
//! and the workspace picked the second.
//!
//! These tests run against a fresh per-test Postgres schema spun
//! up by `store::test_support::pg`, so every migration in
//! `store::migration::Migrator::migrations()` is applied before the
//! assertions fire.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::collections::HashSet;
use store::test_support::pg;

async fn fresh_db_columns() -> Vec<(String, HashSet<String>)> {
    let db = pg().await;

    // List every user table in the per-test schema (the search_path
    // is set in the connection URL, so `current_schema()` resolves
    // to it).
    let tables = db
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT tablename FROM pg_tables \
             WHERE schemaname = current_schema() \
             AND tablename != 'seaql_migrations' \
             ORDER BY tablename"
                .to_string(),
        ))
        .await
        .expect("pg_tables read")
        .into_iter()
        .map(|row| {
            row.try_get::<String>("", "tablename")
                .expect("tablename col")
        })
        .collect::<Vec<_>>();

    let mut out = Vec::with_capacity(tables.len());
    for table in tables {
        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT column_name FROM information_schema.columns \
                     WHERE table_schema = current_schema() \
                     AND table_name = '{table}'"
                ),
            ))
            .await
            .expect("information_schema.columns read");
        let cols: HashSet<String> = rows
            .into_iter()
            .map(|r| r.try_get::<String>("", "column_name").expect("col name"))
            .collect();
        out.push((table, cols));
    }
    out
}

#[tokio::test]
async fn every_table_has_inserted_at_and_updated_at() {
    let tables = fresh_db_columns().await;
    assert!(
        !tables.is_empty(),
        "no user tables found; migrations may not have run"
    );
    let mut missing: Vec<String> = Vec::new();
    for (table, cols) in &tables {
        if !cols.contains("inserted_at") {
            missing.push(format!("{table}.inserted_at"));
        }
        if !cols.contains("updated_at") {
            missing.push(format!("{table}.updated_at"));
        }
    }
    assert!(
        missing.is_empty(),
        "workspace convention: every table must carry inserted_at + updated_at. \
         Missing columns: {missing:?}"
    );
}

#[tokio::test]
async fn no_table_has_a_created_at_column() {
    let tables = fresh_db_columns().await;
    let mut offenders: Vec<String> = Vec::new();
    for (table, cols) in &tables {
        if cols.contains("created_at") {
            offenders.push(table.clone());
        }
    }
    assert!(
        offenders.is_empty(),
        "workspace convention: `created_at` is forbidden — use `inserted_at`. \
         Offending tables: {offenders:?}"
    );
}

#[tokio::test]
async fn documents_project_id_is_not_null() {
    let db = pg().await;
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT is_nullable FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = 'documents' \
             AND column_name = 'project_id'"
                .to_string(),
        ))
        .await
        .expect("information_schema query")
        .expect("documents.project_id row");
    let is_nullable: String = row.try_get("", "is_nullable").expect("is_nullable col");
    assert_eq!(
        is_nullable, "NO",
        "documents.project_id must be NOT NULL — every document belongs to a project"
    );
}

#[tokio::test]
async fn documents_carries_inbound_channel_provenance() {
    let db = pg().await;
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT column_name, is_nullable FROM information_schema.columns \
             WHERE table_schema = current_schema() \
             AND table_name = 'documents'"
                .to_string(),
        ))
        .await
        .expect("information_schema query");
    let mut by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for r in rows {
        let n: String = r.try_get("", "column_name").expect("column_name col");
        let nullable: String = r.try_get("", "is_nullable").expect("is_nullable col");
        by_name.insert(n, nullable);
    }

    // The four provenance columns must exist with the right nullability:
    // source NOT NULL, received_at NOT NULL, source_revision_id NULL,
    // description NULL.
    assert_eq!(
        by_name.get("source").map(String::as_str),
        Some("NO"),
        "documents.source must be NOT NULL"
    );
    assert_eq!(
        by_name.get("received_at").map(String::as_str),
        Some("NO"),
        "documents.received_at must be NOT NULL"
    );
    assert_eq!(
        by_name.get("source_revision_id").map(String::as_str),
        Some("YES"),
        "documents.source_revision_id must be nullable — populated by the inbound workflow"
    );
    assert_eq!(
        by_name.get("description").map(String::as_str),
        Some("YES"),
        "documents.description must be nullable — optional staff caption"
    );
}
