//! Make every Project a git repository: repo identity on `projects`
//! plus the `git_access_tokens` table that backs CLI git auth.
//!
//! See [the design](../../../docs/git-project-repos.md). Each Project is
//! hosted as one append-only bare git repo, served Rust-native from
//! `web`. Two pieces land here:
//!
//! - `projects.git_default_branch` — the single ref the repo carries
//!   (`main`; the design forbids any other branch). Non-null with a
//!   `main` default so every existing row becomes a valid repo identity.
//! - `projects.git_initialized_at` — RFC 3339 timestamp set when the
//!   bare repo is first created on the volume; `NULL` means "not yet
//!   initialized" so the store can lazily create the repo on first use.
//!
//! `drive_folder_id` is left in place but **dormant** — the design keeps
//! Drive as an optional export mirror, so we do not drop the column.
//!
//! `git_access_tokens` holds the short-lived, Project-scoped Personal
//! Access Tokens a `git` CLI presents as HTTP Basic. Tokens are stored
//! **hashed** (`token_hash`); the plaintext is shown once at mint time
//! and never persisted. A `NULL` `project_id` scopes the token to every
//! Project the person participates in; a set `project_id` scopes it to
//! one matter. `scope` is `read` (clone/fetch) or `write` (push).
//! Revocation is deleting the row.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(clippy::too_many_lines)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .add_column(
                        ColumnDef::new(Projects::GitDefaultBranch)
                            .string()
                            .not_null()
                            .default("main")
                            .comment(
                                "The single git ref this Project's repo carries. Always \
                                 `main` — the design is append-only, single-branch; no \
                                 other branch is ever created.",
                            ),
                    )
                    .add_column(ColumnDef::new(Projects::GitInitializedAt).string().comment(
                        "RFC 3339 timestamp when the bare repo was first created on \
                             the volume. NULL = not yet initialized (created lazily on \
                             first git access).",
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(GitAccessTokens::Table)
                    .if_not_exists()
                    .comment(
                        "GitAccessToken — a short-lived, Project-scoped Personal Access \
                         Token a `git` CLI presents as HTTP Basic. Stored hashed; \
                         validated where /mcp validates its bearer. Revocation = row \
                         deletion. See docs/git-project-repos.md §2.",
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .comment("UUIDv7 identifier for this token (time-sortable)."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::PersonId)
                            .uuid()
                            .not_null()
                            .comment("FK → Person (`persons.id`) — the identity this token authenticates as."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::ProjectId).uuid().comment(
                            "FK → Project (`projects.id`) — the one matter this token may \
                             touch. NULL = every Project the person participates in.",
                        ),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::TokenHash)
                            .string()
                            .not_null()
                            .unique_key()
                            .comment("SHA-256 hex of the token plaintext; the plaintext is shown once at mint."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::Scope)
                            .string()
                            .not_null()
                            .comment("`read` (clone/fetch) or `write` (push, a strict superset of read)."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::ExpiresAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 expiry; a token at or past this instant is rejected as if absent."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::InsertedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp when this row was inserted."),
                    )
                    .col(
                        ColumnDef::new(GitAccessTokens::UpdatedAt)
                            .string()
                            .not_null()
                            .comment("RFC 3339 timestamp of the last update."),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_git_access_tokens_person")
                            .from(GitAccessTokens::Table, GitAccessTokens::PersonId)
                            .to(Persons::Table, Persons::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_git_access_tokens_project")
                            .from(GitAccessTokens::Table, GitAccessTokens::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_git_access_tokens_person_id")
                    .table(GitAccessTokens::Table)
                    .col(GitAccessTokens::PersonId)
                    .col(GitAccessTokens::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_git_access_tokens_person_id")
                    .table(GitAccessTokens::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(GitAccessTokens::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(Projects::GitInitializedAt)
                    .drop_column(Projects::GitDefaultBranch)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    GitDefaultBranch,
    GitInitializedAt,
}

#[derive(DeriveIden)]
enum GitAccessTokens {
    Table,
    Id,
    PersonId,
    ProjectId,
    TokenHash,
    Scope,
    ExpiresAt,
    InsertedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
