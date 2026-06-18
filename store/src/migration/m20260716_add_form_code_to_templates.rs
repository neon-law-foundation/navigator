//! Add `form_code` to `templates`.
//!
//! A template whose rendered artifact is a *filled government form*
//! (rather than a Typst-rendered document) declares the form it fills
//! via `form: <form_code>` in its markdown frontmatter — e.g.
//! `onboarding__nest` binds `nv_sos__llc_formation`. The seed loader
//! persists that binding here so the workflow walker can pick the
//! AcroForm rendering path at staff-approve time without re-parsing
//! frontmatter (the body blob is stored frontmatter-stripped).
//!
//! Nullable: most templates render via Typst and carry no form.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Templates::Table)
                    .add_column(ColumnDef::new(Templates::FormCode).string().null().comment(
                        "forms-registry code of the government form this \
                                 template fills (e.g. `nv_sos__llc_formation`); \
                                 NULL for Typst-rendered templates.",
                    ))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Templates::Table)
                    .drop_column(Templates::FormCode)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Templates {
    Table,
    FormCode,
}
