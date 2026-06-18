//! Bind every Notation to a Project — see glossary terms
//! [Notation](../../../docs/notation.md#notation) and
//! [Project](../../../docs/glossary.md#project).
//!
//! An Engagement is *a Notation in the context of a Project*; the
//! glossary's load-bearing rule is "every Notation belongs to
//! exactly one Project." Before this migration `notations` had no
//! `project_id` column, so the rule could only be enforced by
//! convention. This adds the FK.
//!
//! For any pre-existing notation rows (development databases), we
//! synthesize a back-fill Project per orphan so the migration is
//! non-destructive. Production has no notation rows yet. The
//! column is tightened to `NOT NULL` after the back-fill.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;
use uuid::Uuid;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .add_column(ColumnDef::new(Notations::ProjectId).uuid().comment(
                        "FK → Project (`projects.id`). Every Notation lives in \
                             exactly one Project; see docs/glossary.md#project.",
                    ))
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        let backend = db.get_database_backend();

        let orphans = db
            .query_all(Statement::from_string(
                backend,
                "SELECT id, entity_id FROM notations WHERE project_id IS NULL",
            ))
            .await?;
        for row in orphans {
            let nid: Uuid = row.try_get("", "id")?;
            let entity_id: Option<Uuid> = row.try_get("", "entity_id")?;
            let pid = Uuid::now_v7();
            db.execute(Statement::from_sql_and_values(
                backend,
                "INSERT INTO projects (id, name, status, entity_id) VALUES ($1, $2, $3, $4)",
                [
                    pid.into(),
                    format!("Backfill for notation {nid}").into(),
                    "open".into(),
                    entity_id.into(),
                ],
            ))
            .await?;
            db.execute(Statement::from_sql_and_values(
                backend,
                "UPDATE notations SET project_id = $1 WHERE id = $2",
                [pid.into(), nid.into()],
            ))
            .await?;
        }

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_notations_project")
                    .from(Notations::Table, Notations::ProjectId)
                    .to(Projects::Table, Projects::Id)
                    .to_owned(),
            )
            .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE notations ALTER COLUMN project_id SET NOT NULL".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_notations_project")
                    .table(Notations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Notations::Table)
                    .drop_column(Notations::ProjectId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Notations {
    Table,
    ProjectId,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}
