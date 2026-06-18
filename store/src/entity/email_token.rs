//! `email_tokens` — single-use, expiring tokens emailed to a person to
//! prove control of their address.
//!
//! One table, two [`purpose`](Model::purpose) values:
//! [`PURPOSE_PASSWORD_RESET`] (the link in a "reset your password"
//! email) and [`PURPOSE_EMAIL_CONFIRM`] (the link in a "confirm your
//! email" email). Both share the same mechanics — the plaintext is
//! shown once in the emailed URL and never stored, only its SHA-256 hex
//! (`token_hash`); `used_at` enforces single use; `expires_at` bounds
//! the window. See [`crate::email_tokens`] for mint/validate/consume.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// The token in a "reset your password" email. Claiming it lets the
/// holder set a new password in Identity Platform for the account.
pub const PURPOSE_PASSWORD_RESET: &str = "password_reset";
/// The token in a "confirm your email" email. Claiming it flips
/// `emailVerified` in Identity Platform for the account.
pub const PURPOSE_EMAIL_CONFIRM: &str = "email_confirm";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "email_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::person`] — whose account this token acts on.
    pub person_id: Uuid,
    /// Snapshot of the recipient address at mint time, for audit.
    pub email: String,
    /// [`PURPOSE_PASSWORD_RESET`] or [`PURPOSE_EMAIL_CONFIRM`].
    pub purpose: String,
    /// SHA-256 hex of the token plaintext; the plaintext lives only in
    /// the emailed link.
    pub token_hash: String,
    /// RFC 3339 expiry; a token at or past this instant is rejected as
    /// if absent.
    pub expires_at: String,
    /// RFC 3339 timestamp the token was claimed. `None` = unused; set =
    /// spent (single-use).
    pub used_at: Option<String>,
    pub inserted_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

crate::uuid_active_model_behavior!();
