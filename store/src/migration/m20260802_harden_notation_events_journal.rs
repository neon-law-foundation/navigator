//! Harden `notation_events` into the attributable, append-only
//! diligence journal for every notation transition.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE notation_events
                ADD COLUMN acting_person_id uuid,
                ADD COLUMN template_version_id uuid",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE notation_events ne
             SET acting_person_id = n.person_id,
                 template_version_id = n.template_id
             FROM notations n
             WHERE ne.notation_id = n.id",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE notation_events
                ALTER COLUMN acting_person_id SET NOT NULL,
                ALTER COLUMN template_version_id SET NOT NULL,
                ADD CONSTRAINT fk_notation_events_acting_person
                    FOREIGN KEY (acting_person_id) REFERENCES persons(id),
                ADD CONSTRAINT fk_notation_events_template_version
                    FOREIGN KEY (template_version_id) REFERENCES templates(id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION prevent_notation_events_mutation()
             RETURNS trigger
             LANGUAGE plpgsql
             AS $$
             BEGIN
                 RAISE EXCEPTION 'notation_events is append-only';
             END;
             $$",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TRIGGER trg_notation_events_append_only
             BEFORE UPDATE OR DELETE ON notation_events
             FOR EACH ROW EXECUTE FUNCTION prevent_notation_events_mutation()",
        )
        .await
        .map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "DROP TRIGGER IF EXISTS trg_notation_events_append_only ON notation_events",
        )
        .await?;
        db.execute_unprepared("DROP FUNCTION IF EXISTS prevent_notation_events_mutation()")
            .await?;
        db.execute_unprepared(
            "ALTER TABLE notation_events
                DROP CONSTRAINT IF EXISTS fk_notation_events_template_version,
                DROP CONSTRAINT IF EXISTS fk_notation_events_acting_person,
                DROP COLUMN IF EXISTS template_version_id,
                DROP COLUMN IF EXISTS acting_person_id",
        )
        .await
        .map(|_| ())
    }
}
