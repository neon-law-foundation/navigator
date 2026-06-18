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

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::is_unique_violation;
    use sea_orm::{ActiveModelTrait, ActiveValue, DbErr};

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
