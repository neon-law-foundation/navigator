//! Drop the `projects.git_default_branch` column.
//!
//! Every Project repo is append-only and single-branch — the ref is
//! *always* `main`, enforced by the bare repo's `pre-receive` hook and
//! `receive.denyNonFastForwards` / `denyDeletes` config, never by a
//! per-row value. The column could therefore only ever hold `"main"`, so
//! it carried no information and invited code that branched on a choice
//! that does not exist. The single source of truth for the ref name is the
//! `repos::DEFAULT_BRANCH` constant; the DB no longer mirrors it.
//!
//! `git_initialized_at` stays — that column carries real per-row state
//! (the lazy bare-repo init timestamp). Only `git_default_branch` goes.
//!
//! `down` re-adds the column `NOT NULL DEFAULT 'main'`, matching its
//! original shape from `m20260627_add_git_repo_to_projects`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(Projects::GitDefaultBranch)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    GitDefaultBranch,
}
