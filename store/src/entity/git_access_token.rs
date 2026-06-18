//! `git_access_tokens` — short-lived, Project-scoped Personal Access
//! Tokens a `git` CLI presents as HTTP Basic.
//!
//! A browser read of a repo rides the session cookie, but `git clone` /
//! `git push` from a CLI has no cookie — it sends HTTP Basic. `web`
//! mints one of these and the lawyer pastes it into git's credential
//! helper; `web` validates it where `/mcp` validates its bearer, so
//! there is one token-validation seam rather than a parallel password
//! store. See [`crate::git_access_tokens`] for mint/validate and
//! [the design](../../../docs/git-project-repos.md) §2.
//!
//! The plaintext is shown once at mint and never stored — only its
//! SHA-256 hex (`token_hash`). Revocation is deleting the row. A `None`
//! `project_id` scopes the token to every Project the person
//! participates in; a set `project_id` scopes it to one matter.

use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Clone / fetch — read access to the repo.
pub const SCOPE_READ: &str = "read";
/// Push — write access; a strict superset of [`SCOPE_READ`].
pub const SCOPE_WRITE: &str = "write";

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "git_access_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// FK → [`super::person`] — the identity this token authenticates as.
    pub person_id: Uuid,
    /// FK → [`super::project`] — the one matter this token may touch.
    /// `None` = every Project the person participates in.
    pub project_id: Option<Uuid>,
    /// SHA-256 hex of the token plaintext.
    pub token_hash: String,
    /// `read` or `write` — see the `SCOPE_*` constants.
    pub scope: String,
    /// RFC 3339 expiry; a token at or past this instant is rejected as
    /// if absent.
    pub expires_at: String,
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
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id"
    )]
    Project,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

crate::uuid_active_model_behavior!();
