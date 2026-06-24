//! Classify `sea_orm::DbErr` so handlers can map a uniqueness
//! collision (Postgres SQLSTATE `23505`) to the right caller-facing
//! status while leaving every other database failure as an internal
//! error.
//!
//! SeaORM 1.x ships [`DbErr::sql_err`] which already translates the
//! backend-specific SQLSTATE codes into a portable `SqlErr` enum — we
//! lean on that rather than reaching for `sqlx` directly. Lives in
//! `store/` so both `web/` (HTTP → 409) and `mcp/` (JSON-RPC → conflict
//! text) can consume it without either depending on the other.

use sea_orm::{DbErr, SqlErr};

/// `true` when the database rejected the write because it would
/// violate a UNIQUE constraint.
#[must_use]
pub fn is_unique_violation(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_)))
}

/// A staff-facing reason clause for a failed write, taken from the **actual**
/// database error rather than a guess. The usual delete failure is a
/// foreign-key block — "can't delete, it's still referenced" — but it might
/// be something else entirely; either way this surfaces what the database
/// reported. For a foreign-key violation the Postgres message names the
/// referencing table (e.g. `… on table "notations"`), so the operator sees
/// *why*, not a generic "couldn't delete". Intended to be composed into a
/// sentence by the caller (e.g. `format!("Couldn't delete this matter — {}.")`).
///
/// This is an operator/staff surface — constraint and table names are
/// schema facts, not client content, so it is safe to show here (unlike a
/// telemetry span; see the observability rule).
#[must_use]
pub fn describe_write_failure(err: &DbErr) -> String {
    match err.sql_err() {
        Some(SqlErr::ForeignKeyConstraintViolation(detail)) => {
            format!("it's still referenced by other records ({detail})")
        }
        Some(SqlErr::UniqueConstraintViolation(detail)) => {
            format!("it collides with an existing record ({detail})")
        }
        _ => err.to_string(),
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::{describe_write_failure, is_unique_violation};
    use sea_orm::{ActiveModelTrait, ActiveValue, DbErr, EntityTrait};

    /// A foreign-key block (deleting a row another row still references)
    /// is described as "still referenced", carrying the database's own
    /// detail — so a failed delete surfaces the real reason, not a guess.
    #[tokio::test]
    async fn foreign_key_block_is_described_with_the_database_detail() {
        use crate::entity::{entity as entities, project};
        let db = crate::test_support::pg().await;
        let entity_id = crate::test_support::seed_entity(&db).await;
        // A project references the entity (`projects.entity_id` FK).
        project::ActiveModel {
            name: ActiveValue::Set("Blocks its entity".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(entity_id),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("seed project");

        // Deleting the still-referenced entity must fail on the FK.
        let err = entities::Entity::delete_by_id(entity_id)
            .exec(&db)
            .await
            .expect_err("FK must block the delete");

        let msg = describe_write_failure(&err);
        assert!(
            msg.contains("still referenced by other records"),
            "expected a referenced-records description, got: {msg}"
        );
        // The Postgres detail (naming the referencing table) is carried through.
        assert!(
            msg.contains("projects"),
            "the DB detail should name the table: {msg}"
        );
    }

    /// A non-database error falls back to the error's own string rather
    /// than misclassifying it as a constraint problem.
    #[test]
    fn non_database_error_falls_back_to_the_error_text() {
        let err = DbErr::Custom("boom".into());
        let msg = describe_write_failure(&err);
        assert!(msg.contains("boom"), "got: {msg}");
    }

    /// Inserting a second `persons` row with the same email yields a
    /// `DbErr` that `is_unique_violation` recognizes.
    #[tokio::test]
    async fn detects_duplicate_email_on_persons() {
        use crate::entity::person;
        let db = crate::test_support::pg().await;
        let email = "dup@example.com";

        person::ActiveModel {
            name: ActiveValue::Set("First".into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(crate::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("first insert");

        let err = person::ActiveModel {
            name: ActiveValue::Set("Second".into()),
            email: ActiveValue::Set(email.into()),
            role: ActiveValue::Set(crate::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect_err("second insert must collide on the unique email index");

        assert!(
            is_unique_violation(&err),
            "expected a unique-constraint classification, got {err:?}"
        );
    }

    /// Non-database `DbErr` variants are not unique violations. Guards
    /// against a future refactor that accidentally widens the
    /// classifier to "any error".
    #[test]
    fn rejects_non_database_errors() {
        let err = DbErr::Custom("not a sqlx error".into());
        assert!(!is_unique_violation(&err));
    }
}
