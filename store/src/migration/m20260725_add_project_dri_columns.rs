//! Add the two Directly Responsible Individual columns to `projects`:
//! `staff_dri_person_id` (the firm-side accountable person) and
//! `client_dri_person_id` (the client-side accountable person). Both are
//! **nullable** foreign keys to `persons` — a first-class project attribute,
//! distinct from the `person_project_roles` participation ledger.
//!
//! **Pure schema DDL** — `ADD COLUMN` (nullable) plus the two foreign keys,
//! and nothing else. Migrations never carry data statements (no `TRUNCATE`,
//! `DELETE`, `INSERT`, `UPDATE`): a migration is a reversible schema
//! definition, not a data operation.
//!
//! The columns are **nullable on purpose**, so this `ADD COLUMN` applies
//! cleanly to a populated `projects` table with **no backfill and no
//! pre-clearing** — legacy rows simply carry `NULL`. The "every new matter
//! has a real staff + client DRI" invariant is enforced at the create paths
//! (web matter-open, the MCP tools, the CLI), which require and role-check
//! both, rather than by a DB `NOT NULL`. No sentinel persons.

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
                    .add_column(
                        ColumnDef::new(Projects::StaffDriPersonId)
                            .uuid()
                            .null()
                            .comment(
                                "FK → Person (`persons.id`). The firm-side Directly \
                                 Responsible Individual — the single attorney/admin \
                                 accountable for this matter. Nullable: legacy rows \
                                 are NULL; every new matter gets a real DRI, enforced \
                                 at the create paths (not a DB NOT NULL, so the column \
                                 adds cleanly to a populated table). Distinct from the \
                                 `person_project_roles` participation ledger.",
                            ),
                    )
                    .add_column(
                        ColumnDef::new(Projects::ClientDriPersonId)
                            .uuid()
                            .null()
                            .comment(
                                "FK → Person (`persons.id`). The client-side Directly \
                                 Responsible Individual — the single client contact \
                                 accountable for this matter. Nullable, the mirror of \
                                 `staff_dri_person_id`.",
                            ),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_projects_staff_dri_person")
                    .from(Projects::Table, Projects::StaffDriPersonId)
                    .to(Persons::Table, Persons::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_projects_client_dri_person")
                    .from(Projects::Table, Projects::ClientDriPersonId)
                    .to(Persons::Table, Persons::Id)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_projects_client_dri_person")
                    .table(Projects::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_projects_staff_dri_person")
                    .table(Projects::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Projects::Table)
                    .drop_column(Projects::ClientDriPersonId)
                    .drop_column(Projects::StaffDriPersonId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    StaffDriPersonId,
    ClientDriPersonId,
}

#[derive(DeriveIden)]
enum Persons {
    Table,
    Id,
}
