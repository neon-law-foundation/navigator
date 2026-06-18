//! Add `inserted_at` and `updated_at` to every existing table.
//!
//! Workspace convention: every row in every table carries two
//! application-set RFC 3339 timestamps. `inserted_at` is stamped
//! once on insert and never updated; `updated_at` is bumped on
//! every save. The `uuid_active_model_behavior!` macro fills both
//! in automatically — call sites don't need to remember.
//!
//! `created_at` is **not** allowed; the global convention test in
//! `store/tests/conventions.rs` asserts both rules at the schema
//! level so a future migration that adds a `created_at` column
//! fails CI.
//!
//! Both columns are added `NOT NULL` with a stub default ("the
//! moment of this migration") so existing rows have a real value;
//! the default is then dropped so future inserts must go through the
//! ActiveModelBehavior.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Every table that existed before this migration. `project_ingestions`
/// is intentionally absent — it was created in the previous migration
/// already carrying both timestamp columns.
const TABLES: &[&str] = &[
    "addresses",
    "answers",
    "blobs",
    "credentials",
    "disclosures",
    "documents",
    "entities",
    "entity_billing_profiles",
    "entity_types",
    "git_repositories",
    "invoices",
    "invoice_line_items",
    "jurisdictions",
    "letters",
    "mailrooms",
    "notations",
    "notation_events",
    "persons",
    "person_entity_roles",
    "person_project_roles",
    "projects",
    "questions",
    "relationship_logs",
    "sent_emails",
    "share_issuances",
    "templates",
];

/// Stub default for back-filling existing rows. The chosen value
/// is the migration's own birthday so the audit trail says "this
/// row existed before timestamps were a thing" without lying about
/// a more specific moment.
const STUB: &str = "2026-05-25T00:00:00Z";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        for table in TABLES {
            db.execute(Statement::from_string(
                backend,
                format!(
                    "ALTER TABLE {table} ADD COLUMN inserted_at TEXT NOT NULL DEFAULT '{STUB}'"
                ),
            ))
            .await?;
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE {table} ADD COLUMN updated_at TEXT NOT NULL DEFAULT '{STUB}'"),
            ))
            .await?;
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE {table} ALTER COLUMN inserted_at DROP DEFAULT"),
            ))
            .await?;
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE {table} ALTER COLUMN updated_at DROP DEFAULT"),
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();
        for table in TABLES {
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE {table} DROP COLUMN updated_at"),
            ))
            .await?;
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE {table} DROP COLUMN inserted_at"),
            ))
            .await?;
        }
        Ok(())
    }
}
