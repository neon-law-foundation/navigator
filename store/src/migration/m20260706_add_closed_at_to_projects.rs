//! Add `closed_at` to `projects` — the clock the 10-year retention window
//! runs off.
//!
//! A matter's privileged file (its conversation log + raw payloads) is kept
//! for ten years after the matter closes, then securely destroyed — the
//! policy the client consents to in the retainer ("Your file, kept for ten
//! years"). Retention therefore needs the close *date*, not just the
//! `closed` status: `closed_at` is stamped when
//! [`crate::projects::close_for_notation`] flips a matter to `closed`, and the
//! retention sweep purges once `closed_at + 10 years` has passed. Nullable —
//! open matters have no close date, and matters closed before this migration
//! carry `NULL` until they are next touched.

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
                    .add_column(ColumnDef::new(Projects::ClosedAt).string().null().comment(
                        "RFC 3339 timestamp the matter was closed; start of the 10-year \
                         retention window. NULL while open.",
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
                    .drop_column(Projects::ClosedAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    ClosedAt,
}
