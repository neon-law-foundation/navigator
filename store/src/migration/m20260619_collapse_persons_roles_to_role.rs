//! Collapse `persons.roles` (jsonb array) into `persons.role` (text
//! enum) and rename `person_project_roles.role` → `participation`.
//!
//! See [`docs/access-model.md`](../../../docs/access-model.md).
//! Role decides the tier (`client`, `staff`, `admin`); participation
//! decides the per-project scope. Keeping the two on the same column
//! name in different tables silently overloads OPA's `input.session`
//! vs `input.project` reads, hence the rename.
//!
//! Every existing row lands on `role='client'`. Re-running the seed
//! loader (`User.yaml`) lifts seeded staff/admin rows back to their
//! configured tiers; the bootstrap admin OAuth callback path heals the
//! configured operator back to `admin` on next sign-in.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // 1. Add `role` with a `client` default — every existing row
        //    lands on the safe tier. Re-seeding lifts the rows
        //    that should be staff/admin via `User.yaml`.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE persons \
             ADD COLUMN role TEXT NOT NULL DEFAULT 'client' \
             CONSTRAINT persons_role_check \
             CHECK (role IN ('client', 'staff', 'admin'))"
                .to_string(),
        ))
        .await?;

        // 2. Drop the legacy column.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE persons DROP COLUMN roles".to_string(),
        ))
        .await?;

        // 3. Rename `person_project_roles.role` → `participation`.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE person_project_roles RENAME COLUMN role TO participation".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE person_project_roles RENAME COLUMN participation TO role".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE persons ADD COLUMN roles JSONB NOT NULL DEFAULT '[]'::jsonb".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "UPDATE persons SET roles = CASE \
               WHEN role = 'admin' THEN '[\"admin\"]'::jsonb \
               WHEN role = 'staff' THEN '[\"staff\"]'::jsonb \
               ELSE '[]'::jsonb \
             END"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE persons DROP CONSTRAINT persons_role_check".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE persons DROP COLUMN role".to_string(),
        ))
        .await?;

        Ok(())
    }
}
