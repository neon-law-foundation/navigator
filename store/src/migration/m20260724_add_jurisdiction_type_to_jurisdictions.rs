//! Add `jurisdiction_type` to the `jurisdictions` table.
//!
//! The canonical seed (`store/seeds/Jurisdiction.yaml`) has always
//! carried a `jurisdiction_type` (`state` | `country`) for every row, but
//! the table lacked the column, so the field was silently dropped on
//! insert. This reconciles the schema with the seed: every US state and
//! DC is a `state`; the federal sovereigns (`United States`, `Germany`)
//! are `country`.
//!
//! Existing rows are backfilled to `state` (the overwhelming majority);
//! the known `country` rows are then corrected by code so a re-seed is
//! not required to fix already-present data.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Jurisdictions::Table)
                    .add_column(
                        ColumnDef::new(Jurisdictions::JurisdictionType)
                            .string()
                            .not_null()
                            .default("state")
                            .comment(
                                "Kind of jurisdiction — `state` (US state or DC) or \
                                 `country` (federal sovereign).",
                            ),
                    )
                    .to_owned(),
            )
            .await?;

        // Correct the federal sovereigns already present in prod; fresh
        // databases get the right value from the seed regardless.
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE jurisdictions SET jurisdiction_type = 'country' \
                 WHERE code IN ('US', 'GMBH')",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Jurisdictions::Table)
                    .drop_column(Jurisdictions::JurisdictionType)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Jurisdictions {
    Table,
    JurisdictionType,
}
