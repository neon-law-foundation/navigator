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

const BACKFILL_LEGACY_ANSWERS_SQL: &str = "\
INSERT INTO answers (
    id,
    question_id,
    person_id,
    value,
    source,
    authored_by_person_id,
    inserted_at,
    updated_at,
    notation_id,
    state_name
)
SELECT (
        substr(md5(a.id::text || ':' || n.id::text), 1, 8) || '-' ||
        substr(md5(a.id::text || ':' || n.id::text), 9, 4) || '-' ||
        substr(md5(a.id::text || ':' || n.id::text), 13, 4) || '-' ||
        substr(md5(a.id::text || ':' || n.id::text), 17, 4) || '-' ||
        substr(md5(a.id::text || ':' || n.id::text), 21, 12)
    )::uuid,
    a.question_id,
    a.person_id,
    a.value,
    a.source,
    a.authored_by_person_id,
    a.inserted_at,
    a.updated_at,
    n.id,
    q.code
FROM answers a
JOIN notations n ON n.person_id = a.person_id
JOIN questions q ON q.id = a.question_id
WHERE a.notation_id IS NULL
ON CONFLICT (id) DO NOTHING";

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

        // Rows that predate notation-scoped answers were person-scoped,
        // so copy them onto each existing notation for that respondent.
        // Future reads stay strictly `notation_id = ...`; the copied rows
        // preserve upgrade-time renders without reintroducing fallback
        // queries that would leak answers between new matters.
        db.execute(Statement::from_string(
            backend,
            BACKFILL_LEGACY_ANSWERS_SQL.to_string(),
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
            "ALTER TABLE answers DROP CONSTRAINT IF EXISTS fk_answers_notation".to_string(),
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

#[cfg(test)]
mod tests {
    use super::BACKFILL_LEGACY_ANSWERS_SQL;
    use sea_orm::{
        ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
        Statement,
    };

    #[tokio::test]
    async fn backfill_copies_legacy_person_answers_onto_existing_notations() {
        use crate::entity::{answer, notation, project, question, template};

        let db = crate::test_support::pg().await;
        let person = crate::entity::person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra-migration@example.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("migration__retainer".into()),
            title: ActiveValue::Set("Migration Retainer".into()),
            respondent_type: ActiveValue::Set("person".into()),
            is_current: ActiveValue::Set(true),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let q = question::ActiveModel {
            code: ActiveValue::Set("client_name".into()),
            prompt: ActiveValue::Set("Client name?".into()),
            answer_type: ActiveValue::Set("string".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let dri = crate::test_support::dri_person(&db).await;
        let project = project::ActiveModel {
            name: ActiveValue::Set("Migration matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(&db).await),
            staff_dri_person_id: ActiveValue::Set(Some(dri)),
            client_dri_person_id: ActiveValue::Set(Some(dri)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let notation = notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(project.id),
            state: ActiveValue::Set("staff_review".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        answer::ActiveModel {
            question_id: ActiveValue::Set(q.id),
            person_id: ActiveValue::Set(person.id),
            notation_id: ActiveValue::Set(None),
            state_name: ActiveValue::Set(None),
            value: ActiveValue::Set(answer::primitive("Libra Prime")),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        db.execute(Statement::from_string(
            db.get_database_backend(),
            BACKFILL_LEGACY_ANSWERS_SQL.to_string(),
        ))
        .await
        .unwrap();

        let copied = answer::Entity::find()
            .filter(answer::Column::NotationId.eq(notation.id))
            .one(&db)
            .await
            .unwrap()
            .expect("legacy answer copied to notation scope");
        assert_eq!(copied.state_name.as_deref(), Some("client_name"));
        assert_eq!(answer::display_value(&copied.value), "Libra Prime");
    }
}
