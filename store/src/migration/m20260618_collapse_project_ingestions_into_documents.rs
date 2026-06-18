//! Collapse `project_ingestions` into `documents`.
//!
//! Every callsite of `store::documents::ingest_bytes` wrote one
//! `documents` row and one `project_ingestions` row in lockstep
//! (1:1, no fan-out). Two tables linked only by an implicit "same
//! transaction" pairing — no FK, no immutability enforcement, no
//! second writer. The audit-trail framing didn't hold; the simpler
//! model is "a document knows where it came from."
//!
//! Adds four columns to `documents`:
//!
//! - `source` — inbound channel literal (`upload`, `drive_sync`, …),
//!   `NOT NULL`.
//! - `source_revision_id` — upstream revision id (Drive
//!   `headRevisionId`, email `Message-ID`, fax sequence number),
//!   nullable, immutable once set by the inbound workflow.
//! - `received_at` — when the inbound channel got the artifact (not
//!   when we recorded it), `NOT NULL`.
//! - `description` — optional staff-view caption.
//!
//! Backfills from `project_ingestions` by pairing the Nth document
//! per project (ordered by `inserted_at`) with the Nth ingestion
//! per project (same ordering). The 1:1 invariant guarantees this
//! pairing is correct for every row written by `ingest_bytes`.
//! Orphan documents (any that somehow lack a paired ingestion) keep
//! the column defaults — `source='upload'`, `received_at=inserted_at`.
//!
//! Also tightens `documents.project_id` to `NOT NULL`. The column
//! was created nullable for "unscoped documents" but no production
//! caller ever passed `None`; the type system was overstating
//! uncertainty that doesn't exist in the domain. The `ALTER ... SET
//! NOT NULL` will fail loudly if any orphan `NULL` rows exist —
//! that's the right behavior; the operator cleans them up and
//! re-runs.
//!
//! Finally drops `project_ingestions`. The `payload` JSON and
//! `summary` columns die with it: neither had a production writer.
//! Per-channel structured data (email headers, fax metadata) belongs
//! in per-channel tables when those channels actually ship.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // 1. Add the four new columns. `source` and `received_at`
        //    carry defaults so the NOT NULL constraint is satisfied
        //    during the add-then-backfill window; the defaults are
        //    dropped at the end.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ADD COLUMN source TEXT NOT NULL DEFAULT 'upload'".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ADD COLUMN source_revision_id TEXT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ADD COLUMN received_at TEXT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ADD COLUMN description TEXT NULL".to_string(),
        ))
        .await?;

        // 2. Universal fallback for received_at — every document gets
        //    its own inserted_at as a baseline. Backfill below
        //    overrides for paired rows.
        db.execute(Statement::from_string(
            backend,
            "UPDATE documents SET received_at = inserted_at".to_string(),
        ))
        .await?;

        // 3. Pair-and-backfill from project_ingestions. The CTE
        //    assigns the Nth document per project to the Nth
        //    ingestion per project; ingest_bytes wrote them in
        //    lockstep so the pairing is the original 1:1.
        db.execute(Statement::from_string(
            backend,
            r"WITH paired AS (
                SELECT
                    d.id AS doc_id,
                    i.source,
                    i.source_revision_id,
                    i.summary,
                    i.received_at
                FROM (
                    SELECT id, project_id,
                           row_number() OVER (
                               PARTITION BY project_id
                               ORDER BY inserted_at, id
                           ) AS rn
                    FROM documents
                    WHERE project_id IS NOT NULL
                ) d
                JOIN (
                    SELECT id, project_id, source, source_revision_id,
                           summary, received_at,
                           row_number() OVER (
                               PARTITION BY project_id
                               ORDER BY inserted_at, id
                           ) AS rn
                    FROM project_ingestions
                ) i ON i.project_id = d.project_id AND i.rn = d.rn
            )
            UPDATE documents
            SET source = paired.source,
                source_revision_id = paired.source_revision_id,
                description = paired.summary,
                received_at = paired.received_at
            FROM paired
            WHERE documents.id = paired.doc_id"
                .to_string(),
        ))
        .await?;

        // 4. Lock down: received_at NOT NULL, source loses its default
        //    so future inserts must supply one explicitly.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ALTER COLUMN received_at SET NOT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ALTER COLUMN source DROP DEFAULT".to_string(),
        ))
        .await?;

        // 5. Tighten project_id to NOT NULL. Fails loudly if any
        //    orphan NULL rows exist — that's the right behavior.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ALTER COLUMN project_id SET NOT NULL".to_string(),
        ))
        .await?;

        // 6. Drop the now-redundant table.
        db.execute(Statement::from_string(
            backend,
            "DROP TABLE project_ingestions".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let backend = db.get_database_backend();

        // Recreate project_ingestions empty — the original data merge
        // is irreversible; down() restores the schema shape, not the
        // rows that were folded into documents.
        db.execute(Statement::from_string(
            backend,
            r"CREATE TABLE project_ingestions (
                id UUID NOT NULL PRIMARY KEY,
                project_id UUID NOT NULL,
                source TEXT NOT NULL,
                summary TEXT NULL,
                payload TEXT NULL,
                source_revision_id TEXT NULL,
                received_at TEXT NOT NULL,
                inserted_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CONSTRAINT fk_project_ingestions_project
                    FOREIGN KEY (project_id) REFERENCES projects (id)
            )"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            backend,
            "CREATE INDEX idx_project_ingestions_project_id \
             ON project_ingestions (project_id, id)"
                .to_string(),
        ))
        .await?;

        // Loosen project_id back to nullable.
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE documents ALTER COLUMN project_id DROP NOT NULL".to_string(),
        ))
        .await?;

        // Drop the four collapsed columns.
        for col in &["description", "received_at", "source_revision_id", "source"] {
            db.execute(Statement::from_string(
                backend,
                format!("ALTER TABLE documents DROP COLUMN {col}"),
            ))
            .await?;
        }

        Ok(())
    }
}
