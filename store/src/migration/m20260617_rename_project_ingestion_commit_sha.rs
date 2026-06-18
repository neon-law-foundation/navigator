//! Rename `project_ingestions.commit_sha` → `source_revision_id`.
//!
//! The column holds the upstream artifact's revision identifier —
//! Drive's `headRevisionId`, email's `Message-ID`, sequence numbers
//! from fax, and whatever future inbound channels supply.
//! `source_revision_id` keeps the column source-neutral across all
//! of them.
//!
//! Contract is unchanged: nullable, application-set, immutable once
//! written.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ProjectIngestions::Table)
                    .rename_column(
                        ProjectIngestions::CommitSha,
                        ProjectIngestions::SourceRevisionId,
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ProjectIngestions::Table)
                    .rename_column(
                        ProjectIngestions::SourceRevisionId,
                        ProjectIngestions::CommitSha,
                    )
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ProjectIngestions {
    Table,
    CommitSha,
    SourceRevisionId,
}
