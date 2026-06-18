//! Add `oidc_subject` to `persons` — see glossary term
//! [Person](../../../docs/glossary.md#person).
//!
//! The stable identifier returned by the IdP's `sub` claim. We keep
//! Keycloak / Google as the source of truth for identity, but every
//! other profile attribute lives in our own `persons` row. This
//! column is what links the two.
//!
//! Nullable because seeded persons (from `store/seeds/Person.yaml`)
//! don't have an IdP entry yet; unique because each subject maps
//! to exactly one Person.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite rejects `ALTER TABLE ADD COLUMN … UNIQUE`. Add the
        // column first and create the unique index second so this
        // works on both SQLite and Postgres.
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .add_column(
                        ColumnDef::new(Persons::OidcSubject)
                            .string()
                            .comment("OIDC `sub` claim linking this Person to an IdP identity."),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_persons_oidc_subject_unique")
                    .table(Persons::Table)
                    .col(Persons::OidcSubject)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_persons_oidc_subject_unique")
                    .table(Persons::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Persons::Table)
                    .drop_column(Persons::OidcSubject)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    OidcSubject,
}
