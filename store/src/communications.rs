//! `store::communications` — write + read side of the project-scoped,
//! attorney-client privileged conversation log.
//!
//! [`ingest`] is the one write entry point, the analogue of
//! [`crate::documents::ingest_bytes`]: it maps a message from any channel
//! into a [`communication`](crate::entity::communication) spine row. It is
//! idempotent on `(channel, source_ref)` — a re-delivered email or
//! re-ingested source returns the existing row instead of duplicating, so
//! callers can replay safely.
//!
//! [`for_project`] is the read side: the whole thread for one matter,
//! oldest→newest, the way the conversation view renders it.
//!
//! Channel-specific fidelity (a comment's anchor, an email's headers) lives
//! in satellites that FK back to the row this returns — see the design in
//! `prompts/project-communications-ingestion.md`. Privilege is enforced one
//! layer up (`web::access::can_see_project`); this module never widens a
//! query past `project_id`.

use std::sync::Arc;

use chrono::{DateTime, Months, Utc};
use cloud::{StorageError, StorageService};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};
use uuid::Uuid;

use crate::entity::{blob, communication, document, document_comment};
use crate::Db;

/// How long a matter's privileged conversation log is kept after the matter
/// closes, then securely destroyed. Firm policy — the client consents to it
/// in the retainer ("Your file, kept for ten years"). Exceeds the NV RPC
/// file-retention floor, so it is the controlling number.
pub const RETENTION_YEARS: u32 = 10;

/// Channel literals written to `communications.channel`. Centralized so the
/// ingest seams, the thread view, and tests agree on the same strings — a
/// mismatch is a silent "message vanished from the thread" bug. SMS is here
/// already so wiring it up later is a caller change, not a schema change.
pub mod channel {
    pub const DOCUMENT_COMMENT: &str = "document_comment";
    pub const EMAIL_INBOUND: &str = "email_inbound";
    pub const EMAIL_OUTBOUND: &str = "email_outbound";
    pub const PORTAL_MESSAGE: &str = "portal_message";
    pub const SMS_INBOUND: &str = "sms_inbound";
    pub const SMS_OUTBOUND: &str = "sms_outbound";
}

/// Direction literals written to `communications.direction`.
pub mod direction {
    /// From the client to the firm.
    pub const INBOUND: &str = "inbound";
    /// From the firm to the client.
    pub const OUTBOUND: &str = "outbound";
    /// A firm-internal note — never shown to the client.
    pub const INTERNAL: &str = "internal";
}

/// Inputs to [`ingest`]. Held in a struct (not a positional list) so future
/// fields don't break callers, mirroring [`crate::documents::IngestArgs`].
#[derive(Debug, Clone)]
pub struct IngestArgs<'a> {
    /// Matter this message belongs to.
    pub project_id: Uuid,
    /// Channel literal — one of [`channel`].
    pub channel: &'a str,
    /// Direction literal — one of [`direction`].
    pub direction: &'a str,
    /// Author, when we have a `persons` row for them.
    pub author_person_id: Option<Uuid>,
    /// Email/name of the other party when there is no `persons` row.
    pub counterparty: Option<&'a str>,
    /// Optional subject line.
    pub subject: Option<&'a str>,
    /// Normalized message text.
    pub body: &'a str,
    /// External id for idempotent ingest (`Message-ID`, comment id, SMS id).
    /// When `Some`, a second ingest with the same `(channel, source_ref)`
    /// returns the existing row.
    pub source_ref: Option<&'a str>,
    /// Raw payload blob, when one was archived (verbatim `.eml`, …).
    pub blob_id: Option<Uuid>,
    /// When the message actually happened (RFC 3339). Distinct from the
    /// insert time, which the row stamps itself.
    pub occurred_at: &'a str,
}

/// What [`ingest`] resolved to.
#[derive(Debug, Clone, Copy)]
pub struct Ingested {
    pub communication_id: Uuid,
    /// `true` when an existing row with the same `(channel, source_ref)`
    /// was returned instead of inserting — the caller replayed a source.
    pub deduped: bool,
}

/// Ingest one message into the conversation log. Idempotent on
/// `(channel, source_ref)` when `source_ref` is `Some`.
///
/// # Errors
///
/// Propagates any database error.
pub async fn ingest(db: &Db, args: &IngestArgs<'_>) -> Result<Ingested, sea_orm::DbErr> {
    if let Some(source_ref) = args.source_ref {
        let existing = communication::Entity::find()
            .filter(communication::Column::Channel.eq(args.channel))
            .filter(communication::Column::SourceRef.eq(source_ref))
            .one(db)
            .await?;
        if let Some(row) = existing {
            return Ok(Ingested {
                communication_id: row.id,
                deduped: true,
            });
        }
    }

    let row = communication::ActiveModel {
        project_id: ActiveValue::Set(args.project_id),
        channel: ActiveValue::Set(args.channel.to_string()),
        direction: ActiveValue::Set(args.direction.to_string()),
        author_person_id: ActiveValue::Set(args.author_person_id),
        counterparty: ActiveValue::Set(args.counterparty.map(String::from)),
        subject: ActiveValue::Set(args.subject.map(String::from)),
        body: ActiveValue::Set(args.body.to_string()),
        source_ref: ActiveValue::Set(args.source_ref.map(String::from)),
        blob_id: ActiveValue::Set(args.blob_id),
        occurred_at: ActiveValue::Set(args.occurred_at.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(Ingested {
        communication_id: row.id,
        deduped: false,
    })
}

/// The whole conversation for one matter, oldest→newest — the order the
/// thread view renders. This is the **firm** view: every row, internal notes
/// included. Whether the caller may read this `project_id` at all is the
/// access layer's job (`web::access::can_see_project`); a client gets
/// [`for_project_client_visible`] instead.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_project(
    db: &Db,
    project_id: Uuid,
) -> Result<Vec<communication::Model>, sea_orm::DbErr> {
    communication::Entity::find()
        .filter(communication::Column::ProjectId.eq(project_id))
        .order_by_asc(communication::Column::OccurredAt)
        .order_by_asc(communication::Column::Id)
        .all(db)
        .await
}

/// The conversation as a **client** may see it: every row except firm-internal
/// notes (`direction = internal`). Internal notes are firm work product; a
/// client must never read one, so the exclusion is enforced in the query, not
/// left to the template. The firm sees [`for_project`].
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_project_client_visible(
    db: &Db,
    project_id: Uuid,
) -> Result<Vec<communication::Model>, sea_orm::DbErr> {
    communication::Entity::find()
        .filter(communication::Column::ProjectId.eq(project_id))
        .filter(communication::Column::Direction.ne(direction::INTERNAL))
        .order_by_asc(communication::Column::OccurredAt)
        .order_by_asc(communication::Column::Id)
        .all(db)
        .await
}

/// Errors from a retention purge — the DB deletes and the storage-object
/// deletes can each fail.
#[derive(Debug, thiserror::Error)]
pub enum PurgeError {
    #[error("database: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
}

/// What a purge removed — returned for the caller to log / report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub communications_deleted: u64,
    pub blobs_deleted: u64,
}

/// Permanently delete one matter's privileged conversation log and the raw
/// payloads only it referenced — the end-of-retention destruction the
/// retainer promises. Matter-scoped and logged.
///
/// The DB deletes run in one transaction: the comment satellite's
/// `communication_id` FK is cleared first (the Phase A review-surface row
/// itself is governed by the separate expunge machinery, not this sweep), the
/// spine rows are deleted, and any blob now referenced by no document and no
/// remaining communication is deleted too. Storage objects are removed after
/// the transaction commits, so a crash leaves at worst an orphaned object (a
/// later sweep reclaims it) — never a row pointing at bytes that are gone.
///
/// # Errors
///
/// [`PurgeError`] on a database or storage failure.
pub async fn purge_for_project(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    project_id: Uuid,
) -> Result<PurgeReport, PurgeError> {
    let comms = for_project(db, project_id).await?;
    if comms.is_empty() {
        return Ok(PurgeReport::default());
    }
    let comm_ids: Vec<Uuid> = comms.iter().map(|c| c.id).collect();
    let mut blob_ids: Vec<Uuid> = comms.iter().filter_map(|c| c.blob_id).collect();
    blob_ids.sort();
    blob_ids.dedup();

    let txn = db.begin().await?;

    // Clear the comment satellite's link so the spine delete is FK-legal.
    document_comment::Entity::update_many()
        .col_expr(
            document_comment::Column::CommunicationId,
            sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
        )
        .filter(document_comment::Column::CommunicationId.is_in(comm_ids.clone()))
        .exec(&txn)
        .await?;

    let deleted = communication::Entity::delete_many()
        .filter(communication::Column::ProjectId.eq(project_id))
        .exec(&txn)
        .await?;

    // Delete each referenced blob whose last referent we just removed. A blob
    // is content-addressed and may be shared, so check both lanes before
    // deleting; collect the storage keys to remove after commit.
    let mut storage_keys: Vec<String> = Vec::new();
    for bid in blob_ids {
        let referenced_by_doc = document::Entity::find()
            .filter(document::Column::BlobId.eq(bid))
            .one(&txn)
            .await?
            .is_some();
        let referenced_by_comm = communication::Entity::find()
            .filter(communication::Column::BlobId.eq(bid))
            .one(&txn)
            .await?
            .is_some();
        if !referenced_by_doc && !referenced_by_comm {
            if let Some(b) = blob::Entity::find_by_id(bid).one(&txn).await? {
                storage_keys.push(b.storage_key);
                blob::Entity::delete_by_id(bid).exec(&txn).await?;
            }
        }
    }

    txn.commit().await?;

    // Storage objects last — orphaned bytes are recoverable; a dangling row is
    // not.
    for key in &storage_keys {
        storage.delete(key).await?;
    }

    let report = PurgeReport {
        communications_deleted: deleted.rows_affected,
        blobs_deleted: storage_keys.len() as u64,
    };
    tracing::info!(
        %project_id,
        communications = report.communications_deleted,
        blobs = report.blobs_deleted,
        "purged matter conversation log at end of retention",
    );
    Ok(report)
}

/// Purge every matter whose retention window has elapsed: a closed matter
/// (`projects.closed_at` set) is destroyed once `closed_at + retention_years`
/// has passed `now`. `now` is passed in so the sweep is deterministic in
/// tests and replayable in a durable workflow. `retention_years` is normally
/// [`RETENTION_YEARS`].
///
/// # Errors
///
/// [`PurgeError`] on the first database or storage failure.
pub async fn purge_expired_matters(
    db: &Db,
    storage: &Arc<dyn StorageService>,
    now: DateTime<Utc>,
    retention_years: u32,
) -> Result<PurgeReport, PurgeError> {
    use crate::entity::project;

    let closed_matters = project::Entity::find()
        .filter(project::Column::ClosedAt.is_not_null())
        .all(db)
        .await?;

    let mut total = PurgeReport::default();
    for p in closed_matters {
        let Some(stamp) = p.closed_at.as_deref() else {
            continue;
        };
        let Ok(closed_time) = DateTime::parse_from_rfc3339(stamp) else {
            tracing::warn!(project_id = %p.id, closed_at = stamp, "unparseable closed_at; skipping retention");
            continue;
        };
        let due = closed_time
            .with_timezone(&Utc)
            .checked_add_months(Months::new(retention_years * 12));
        if due.is_some_and(|due| now >= due) {
            let r = purge_for_project(db, storage, p.id).await?;
            total.communications_deleted += r.communications_deleted;
            total.blobs_deleted += r.blobs_deleted;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::{
        channel, direction, for_project, for_project_client_visible, ingest, purge_expired_matters,
        purge_for_project, IngestArgs, RETENTION_YEARS,
    };
    use chrono::{DateTime, Utc};
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn seed_project(db: &crate::Db) -> Uuid {
        let id = Uuid::now_v7();
        crate::entity::project::ActiveModel {
            id: ActiveValue::Set(id),
            name: ActiveValue::Set("Test Matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        id
    }

    fn args<'a>(project_id: Uuid, ch: &'a str, dir: &'a str, body: &'a str) -> IngestArgs<'a> {
        IngestArgs {
            project_id,
            channel: ch,
            direction: dir,
            author_person_id: None,
            counterparty: None,
            subject: None,
            body,
            source_ref: None,
            blob_id: None,
            occurred_at: "2026-06-08T10:00:00Z",
        }
    }

    #[tokio::test]
    async fn ingest_inserts_a_spine_row() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        let out = ingest(
            &db,
            &args(
                project_id,
                channel::DOCUMENT_COMMENT,
                direction::INBOUND,
                "Should this be my full legal name?",
            ),
        )
        .await
        .unwrap();
        assert!(!out.deduped);

        let rows = for_project(&db, project_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, out.communication_id);
        assert_eq!(rows[0].channel, channel::DOCUMENT_COMMENT);
        assert_eq!(rows[0].direction, direction::INBOUND);
        assert_eq!(rows[0].body, "Should this be my full legal name?");
        assert!(rows[0].source_ref.is_none());
    }

    #[tokio::test]
    async fn ingest_dedupes_on_channel_and_source_ref() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        let mut a = args(project_id, channel::EMAIL_INBOUND, direction::INBOUND, "hi");
        a.source_ref = Some("msg-abc@mail.example.com");

        let first = ingest(&db, &a).await.unwrap();
        assert!(!first.deduped);

        // Same Message-ID re-delivered (SendGrid retry): no duplicate row.
        let second = ingest(&db, &a).await.unwrap();
        assert!(second.deduped);
        assert_eq!(second.communication_id, first.communication_id);

        assert_eq!(for_project(&db, project_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn same_source_ref_different_channel_is_distinct() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        let mut inbound = args(project_id, channel::EMAIL_INBOUND, direction::INBOUND, "q");
        inbound.source_ref = Some("ref-1");
        let mut outbound = args(
            project_id,
            channel::EMAIL_OUTBOUND,
            direction::OUTBOUND,
            "a",
        );
        outbound.source_ref = Some("ref-1");

        let a = ingest(&db, &inbound).await.unwrap();
        let b = ingest(&db, &outbound).await.unwrap();
        assert_ne!(a.communication_id, b.communication_id);
        assert_eq!(for_project(&db, project_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn null_source_ref_never_dedupes() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        // Two comments with no external id are distinct events.
        ingest(
            &db,
            &args(
                project_id,
                channel::DOCUMENT_COMMENT,
                direction::INBOUND,
                "one",
            ),
        )
        .await
        .unwrap();
        ingest(
            &db,
            &args(
                project_id,
                channel::DOCUMENT_COMMENT,
                direction::INBOUND,
                "two",
            ),
        )
        .await
        .unwrap();

        assert_eq!(for_project(&db, project_id).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn client_visible_excludes_internal_firm_notes() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        // A client-facing inbound message and a firm-internal note.
        ingest(
            &db,
            &args(
                project_id,
                channel::EMAIL_INBOUND,
                direction::INBOUND,
                "client question",
            ),
        )
        .await
        .unwrap();
        ingest(
            &db,
            &args(
                project_id,
                channel::PORTAL_MESSAGE,
                direction::INTERNAL,
                "FIRM EYES ONLY — strategy note",
            ),
        )
        .await
        .unwrap();

        // The firm sees both; the client sees only the non-internal row, and
        // never the internal note's body.
        assert_eq!(for_project(&db, project_id).await.unwrap().len(), 2);
        let client_view = for_project_client_visible(&db, project_id).await.unwrap();
        assert_eq!(client_view.len(), 1);
        assert_eq!(client_view[0].body, "client question");
        assert!(
            client_view
                .iter()
                .all(|c| c.direction != direction::INTERNAL),
            "a client must never read a firm-internal note"
        );
    }

    #[tokio::test]
    async fn for_project_returns_thread_oldest_first() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db).await;

        let mut later = args(
            project_id,
            channel::EMAIL_OUTBOUND,
            direction::OUTBOUND,
            "reply",
        );
        later.occurred_at = "2026-06-08T12:00:00Z";
        let mut earlier = args(
            project_id,
            channel::EMAIL_INBOUND,
            direction::INBOUND,
            "question",
        );
        earlier.occurred_at = "2026-06-08T09:00:00Z";

        ingest(&db, &later).await.unwrap();
        ingest(&db, &earlier).await.unwrap();

        let rows = for_project(&db, project_id).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].body, "question");
        assert_eq!(rows[1].body, "reply");
    }

    async fn fs_storage() -> Arc<dyn cloud::StorageService> {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        Arc::new(cloud::FsStorage::new(root).await.unwrap())
    }

    #[tokio::test]
    async fn purge_for_project_deletes_the_log_and_its_raw_payload() {
        let db = crate::test_support::pg().await;
        let storage = fs_storage().await;
        let project_id = seed_project(&db).await;

        // A raw .eml payload stored as a content-addressed blob, referenced by
        // an inbound-email communication.
        let storage_key = "blobs/deadbeef".to_string();
        storage
            .put(&storage_key, b"raw rfc5322 bytes", "message/rfc822")
            .await
            .unwrap();
        let blob_id = Uuid::now_v7();
        crate::entity::blob::ActiveModel {
            id: ActiveValue::Set(blob_id),
            storage_key: ActiveValue::Set(storage_key.clone()),
            content_type: ActiveValue::Set("message/rfc822".into()),
            byte_size: ActiveValue::Set(17),
            sha256_hex: ActiveValue::Set("deadbeef".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut with_blob = args(
            project_id,
            channel::EMAIL_INBOUND,
            direction::INBOUND,
            "raw email body",
        );
        with_blob.blob_id = Some(blob_id);
        ingest(&db, &with_blob).await.unwrap();
        ingest(
            &db,
            &args(
                project_id,
                channel::PORTAL_MESSAGE,
                direction::INTERNAL,
                "a note",
            ),
        )
        .await
        .unwrap();

        let report = purge_for_project(&db, &storage, project_id).await.unwrap();
        assert_eq!(report.communications_deleted, 2);
        assert_eq!(report.blobs_deleted, 1);

        // The conversation log is gone, the blob row is gone, and the raw
        // payload is gone from storage — the row and its bytes share a fate.
        assert!(for_project(&db, project_id).await.unwrap().is_empty());
        assert!(crate::entity::blob::Entity::find_by_id(blob_id)
            .one(&db)
            .await
            .unwrap()
            .is_none());
        assert!(storage.get(&storage_key).await.is_err());
    }

    #[tokio::test]
    async fn purge_expired_matters_only_purges_past_the_retention_window() {
        let db = crate::test_support::pg().await;
        let storage = fs_storage().await;

        // now, an old close (11 years ago → expired), a recent close (kept).
        let now: DateTime<Utc> = "2026-06-08T00:00:00Z".parse().unwrap();
        let old_project = seed_project(&db).await;
        let recent_project = seed_project(&db).await;
        set_closed_at(&db, old_project, "2015-01-01T00:00:00Z").await;
        set_closed_at(&db, recent_project, "2025-12-01T00:00:00Z").await;

        ingest(
            &db,
            &args(
                old_project,
                channel::EMAIL_INBOUND,
                direction::INBOUND,
                "old",
            ),
        )
        .await
        .unwrap();
        ingest(
            &db,
            &args(
                recent_project,
                channel::EMAIL_INBOUND,
                direction::INBOUND,
                "recent",
            ),
        )
        .await
        .unwrap();

        let report = purge_expired_matters(&db, &storage, now, RETENTION_YEARS)
            .await
            .unwrap();
        assert_eq!(report.communications_deleted, 1, "only the expired matter");

        // The 11-year-old matter's log is destroyed; the recent one is kept.
        assert!(for_project(&db, old_project).await.unwrap().is_empty());
        assert_eq!(for_project(&db, recent_project).await.unwrap().len(), 1);
    }

    async fn set_closed_at(db: &crate::Db, project_id: Uuid, when: &str) {
        let p = crate::entity::project::Entity::find_by_id(project_id)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let mut active: crate::entity::project::ActiveModel = p.into();
        active.status = ActiveValue::Set("closed".into());
        active.closed_at = ActiveValue::Set(Some(when.to_string()));
        active.update(db).await.unwrap();
    }
}
