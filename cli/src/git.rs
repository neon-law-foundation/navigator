//! `git` subcommand — operate the per-Project git repos from the CLI.
//!
//! Two operations the firm runs by hand:
//!
//! - `token` mints a Personal Access Token for a person (the credential
//!   a `git` CLI pastes into its helper; see
//!   [the design](../../docs/git-project-repos.md) §2). The plaintext is
//!   printed once and never recoverable — only its hash is stored.
//! - `url` prints the clone URL for a Project
//!   (`<base>/projects/<id>.git`), so a lawyer can copy it.
//!
//! Token *generation* (randomness) lives here, not in `store`, which
//! stays deterministic — `store::git_access_tokens` only hashes and
//! persists what we hand it.

use chrono::{Duration, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use store::entity::{git_access_token, person};
use uuid::Uuid;

/// A fresh random PAT secret: 32 bytes (256 bits) as lowercase hex. The
/// lawyer pastes this into git's credential helper as the password.
#[must_use]
pub fn random_pat() -> String {
    let bytes: [u8; 32] = rand::random();
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// The clone URL for a Project: `<base>/projects/<id>.git`. `base` is
/// the deployment's public origin (`https://www.your-domain.example`) —
/// never hard-coded.
#[must_use]
pub fn clone_url(base: &str, project_id: Uuid) -> String {
    format!("{}/projects/{project_id}.git", base.trim_end_matches('/'))
}

/// Mint a PAT for the person identified by `email`, scoped to
/// `project_id` (`None` = every Project they participate in). Returns
/// the plaintext (to show once) and the stored row.
///
/// # Errors
/// Errors if no person has that email, the scope is unknown, or the
/// insert fails.
pub async fn mint_token(
    db: &DatabaseConnection,
    email: &str,
    project_id: Option<Uuid>,
    scope: &str,
    ttl_hours: i64,
) -> anyhow::Result<(String, git_access_token::Model)> {
    if scope != git_access_token::SCOPE_READ && scope != git_access_token::SCOPE_WRITE {
        anyhow::bail!("scope must be `read` or `write` (got `{scope}`)");
    }
    let person = person::Entity::find()
        .filter(person::Column::Email.eq(email))
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no person with email `{email}`"))?;

    let plaintext = random_pat();
    let model = store::git_access_tokens::mint(
        db,
        person.id,
        project_id,
        scope,
        &plaintext,
        Utc::now() + Duration::hours(ttl_hours),
    )
    .await?;
    Ok((plaintext, model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_pat_is_64_hex_chars_and_varies() {
        let a = random_pat();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_pat());
    }

    #[test]
    fn clone_url_joins_without_double_slash() {
        let id = Uuid::nil();
        assert_eq!(
            clone_url("https://www.example.test/", id),
            "https://www.example.test/projects/00000000-0000-0000-0000-000000000000.git"
        );
        assert_eq!(
            clone_url("https://www.example.test", id),
            "https://www.example.test/projects/00000000-0000-0000-0000-000000000000.git"
        );
    }

    #[tokio::test]
    async fn mint_token_resolves_person_and_validates() {
        use sea_orm::{ActiveModelTrait, ActiveValue};
        let db = store::test_support::pg().await;

        store::entity::person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let (plaintext, model) = mint_token(
            &db,
            "libra@example.com",
            None,
            git_access_token::SCOPE_WRITE,
            24,
        )
        .await
        .unwrap();
        assert_eq!(model.scope, git_access_token::SCOPE_WRITE);

        // The minted plaintext validates back to the same identity.
        let resolved = store::git_access_tokens::validate(&db, &plaintext, Utc::now())
            .await
            .unwrap()
            .expect("token validates");
        assert_eq!(resolved.id, model.id);

        // An unknown email is a clean error, not a panic.
        assert!(mint_token(&db, "nobody@example.com", None, "read", 24)
            .await
            .is_err());
    }
}
