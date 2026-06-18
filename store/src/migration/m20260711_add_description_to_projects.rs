//! Add `description` to `projects` — the matter's own scope narrative,
//! distinct from the retainer's inline services line.
//!
//! "Every retainer is the firm's standing terms plus this project's
//! story." The story is the project's `description`: a free-text scope
//! narrative captured at matter-open. When a retainer is opened in the
//! same action, the description is seeded as the notation's position-0
//! custom clause (System provenance, a draft the attorney edits at the
//! `staff_review` gate) so it splices into the agreement at the
//! `{{custom_clauses}}` marker, after the firm's standing terms. Nullable
//! — a plain project create carries no description, and matters opened
//! before this migration carry `NULL`.

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
                    .add_column(ColumnDef::new(Projects::Description).text().null().comment(
                        "The matter's scope narrative ('this project's story'). Seeded as the \
                         retainer's position-0 custom clause at matter-open. NULL when absent.",
                    ))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(Projects::Description)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Description,
}
