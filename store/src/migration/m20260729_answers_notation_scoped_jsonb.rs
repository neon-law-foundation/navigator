//! Make answers notation-scoped, role-keyed, and append-only.
//!
//! Three changes turn the flat, person-scoped answer row into one that
//! can hold two records of the same type under one Notation — the
//! data-loss bug a multi-party matter (members, trustees, directors) hits
//! today:
//!
//! - `answers.notation_id` (FK → notations, nullable) — which Notation
//!   collected this answer. Nullable because the canonical-seed fixtures
//!   (`Answer.yaml`) are person-scoped demo data with no Notation behind
//!   them; every Notation-bound write site populates it.
//! - `answers.state_name` (nullable text) — the full `<type>__<role>`
//!   questionnaire state the answer was given for (`entity__company`,
//!   `entity__subsidiary`). The respondent's answer row points at the
//!   **bare** question (`entity`), so without this the role discriminator
//!   is lost at write and two records of one type collapse at render.
//!   Null for bare/legacy/seed answers that carry no role.
//! - `answers.value` TEXT → JSONB — primitives become `{"value": …}`,
//!   singular record answers mirror the row they create/select, and
//!   aggregate (plural) answers store an array of the singular shape.
//!   Existing text values are wrapped into the primitive envelope.
//!
//! Answers are **append-only**: there is no unique constraint, re-asks and
//! corrections are new rows, and latest-per-`(notation_id, state_name)`
//! wins on read.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers ADD COLUMN notation_id UUID NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers ADD CONSTRAINT fk_answers_notation \
             FOREIGN KEY (notation_id) REFERENCES notations(id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "CREATE INDEX idx_answers_notation_id ON answers (notation_id)".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers ADD COLUMN state_name TEXT NULL".to_string(),
        ))
        .await?;

        // Wrap every existing text value into the primitive JSON envelope
        // `{"value": …}` as part of the type change so no data is lost.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers \
             ALTER COLUMN value TYPE JSONB USING jsonb_build_object('value', value)"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // Unwrap the primitive envelope back to text. Record/aggregate
        // answers (which have no plain `value` key) fall back to the JSON
        // text so the column still converts cleanly.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers \
             ALTER COLUMN value TYPE TEXT USING COALESCE(value->>'value', value::text)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers DROP COLUMN state_name".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "DROP INDEX IF EXISTS idx_answers_notation_id".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers DROP CONSTRAINT fk_answers_notation".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE answers DROP COLUMN notation_id".to_string(),
        ))
        .await?;

        Ok(())
    }
}
