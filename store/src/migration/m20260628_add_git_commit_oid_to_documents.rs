//! Link each `documents` row to the git commit that filed it into the
//! Project's repo.
//!
//! When a document is written to a matter (inbound-email attachment,
//! portal upload, e-sign completion) it is also committed to the
//! Project's append-only repo authored as the acting person — the
//! commit log is the audit trail (see
//! [the design](../../../docs/git-project-repos.md) §7). `git_commit_oid`
//! records which commit holds this document, so the relational index
//! and the repo history reconcile, and the linkage rides the existing
//! `archives` table snapshot into the BigQuery data lake.
//!
//! Nullable: a document filed before the repo layer (or when the repo
//! root is unconfigured) still inserts — the commit is additive and
//! never blocks the durable blob+row write.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Documents::Table)
                    .add_column(ColumnDef::new(Documents::GitCommitOid).string().comment(
                        "Git commit oid (in the Project's repo) that filed this document. \
                         NULL = not committed to the repo (filed before the repo layer, or \
                         the repo root was unconfigured).",
                    ))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Documents::Table)
                    .drop_column(Documents::GitCommitOid)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Documents {
    Table,
    GitCommitOid,
}
