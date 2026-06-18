//! Add `roles` to `persons` — see glossary term
//! [Person](../../../docs/glossary.md#person).
//!
//! The source of truth for what a signed-in user is allowed to do.
//! The OIDC token only carries identity (`sub`, `email`);
//! authorization happens against this column, evaluated by OPA
//! against the rego policy.
//!
//! Stored as a JSON array (`["staff", "admin"]`). The column type
//! maps to `JSONB` on Postgres and `text` on SQLite — both decode
//! cleanly to SeaORM's `JsonValue`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .add_column(
                        ColumnDef::new(Persons::Roles)
                            .json_binary()
                            .not_null()
                            .default("[]")
                            .comment(
                                "Authorization roles for the Person — JSON array of \
                                 role tokens (e.g., `[\"staff\", \"admin\"]`). \
                                 Evaluated by OPA.",
                            ),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::Roles)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Roles,
}
