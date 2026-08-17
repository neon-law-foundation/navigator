//! `document_comments` — one comment a reader anchored to a text range
//! within a [`crate::review_documents`] draft, and every query against the
//! table.
//!
//! # This table lives in SurrealDB
//!
//! `document_comments` moved with wave five of #1093 (ENG-121), in the
//! satellite-ring slice.
//!
//! The review surface is read-only; a comment is the only thing the client
//! writes there. The anchor is a character-offset range into the document
//! text (`anchor_start`..`anchor_end`) plus the `quoted_text` it covered, so
//! the sidebar can show the quote even if the underlying draft is later
//! re-rendered. Lawyer flip `resolved` once addressed.
//!
//! # The spine row is written before its satellite
//!
//! [`create_with_communication`] writes a comment **and** its
//! `communications` spine row — the unified privileged conversation log.
//! A multi-statement Surreal query is not one transaction, so the two
//! writes are ordered rather than atomic: the spine row goes first and the
//! satellite second. A failure between them leaves an orphaned spine row
//! with no satellite, which reads as a conversation entry with no anchor —
//! the recoverable direction. The reverse would be a comment pointing at a
//! spine row that does not exist.
//!
//! `communication_id` is a bare `uuid` rather than a
//! `record<communication>` link, so every query against it is an equality
//! match.

use chrono::{DateTime, Utc};
use serde::Serialize;
use surrealdb::types::SurrealValue;
use uuid::Uuid;

use crate::surreal::{record_id, record_uuid, retry, SurrealDb};

/// The table these rows live in.
const TABLE: &str = "document_comment";

/// One anchored comment.
///
/// The application-facing shape: plain Rust types, no engine handles.
/// [`DocumentCommentRow`] is the seam that turns it into (and back out of)
/// what the SDK reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentComment {
    pub id: Uuid,
    pub review_document_id: Uuid,
    pub person_id: Uuid,
    pub anchor_start: i32,
    pub anchor_end: i32,
    pub quoted_text: String,
    pub body: String,
    pub resolved: bool,
    /// The `communications` spine row for this comment, when it was
    /// written through [`create_with_communication`]. `None` for a legacy
    /// Phase A row. Bare `uuid`, not a link — `communications` has not
    /// moved to SurrealDB yet.
    pub communication_id: Option<Uuid>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The row as the engine reads and writes it.
#[derive(SurrealValue)]
struct DocumentCommentRow {
    id: surrealdb::types::RecordId,
    review_document_id: surrealdb::types::RecordId,
    person_id: surrealdb::types::RecordId,
    anchor_start: i32,
    anchor_end: i32,
    quoted_text: String,
    body: String,
    resolved: bool,
    communication_id: Option<Uuid>,
    inserted_at: surrealdb::types::Datetime,
    updated_at: surrealdb::types::Datetime,
}

impl DocumentCommentRow {
    /// `None` when a record id is not a native UUID key — a row written by
    /// something that bypassed [`crate::surreal::record_id`].
    fn into_comment(self) -> Option<DocumentComment> {
        Some(DocumentComment {
            id: record_uuid(&self.id)?,
            review_document_id: record_uuid(&self.review_document_id)?,
            person_id: record_uuid(&self.person_id)?,
            anchor_start: self.anchor_start,
            anchor_end: self.anchor_end,
            quoted_text: self.quoted_text,
            body: self.body,
            resolved: self.resolved,
            communication_id: self.communication_id,
            inserted_at: self.inserted_at.into(),
            updated_at: self.updated_at.into(),
        })
    }
}

/// The projection every read shares, so one field list describes the row
/// and a new column cannot reach [`DocumentCommentRow`] from only one
/// query.
const SELECT: &str = "id, review_document_id, person_id, anchor_start, anchor_end, quoted_text, \
     body, resolved, communication_id, inserted_at, updated_at";

/// One new anchored comment. The anchor is a ProseMirror position range
/// plus the text it covered, captured client-side from the read-only
/// document.
#[derive(Debug, Clone)]
pub struct NewComment<'a> {
    pub review_document_id: Uuid,
    pub person_id: Uuid,
    pub anchor_start: i32,
    pub anchor_end: i32,
    pub quoted_text: &'a str,
    pub body: &'a str,
}

/// A new comment plus the spine-row fields it can't derive from the
/// satellite alone: the matter it belongs to and which way the message
/// flows. Used by [`create_with_communication`], the path the review
/// surface takes now that every comment is one entry in the matter's
/// privileged conversation log.
#[derive(Debug, Clone)]
pub struct NewLinkedComment<'a> {
    /// Matter this comment belongs to (the spine's `project_id`).
    pub project_id: Uuid,
    pub review_document_id: Uuid,
    pub person_id: Uuid,
    /// `communications` direction — `inbound` for a client's comment,
    /// `outbound` for a lawyer comment the client will read. See
    /// [`crate::communications::direction`].
    pub direction: &'a str,
    pub anchor_start: i32,
    pub anchor_end: i32,
    pub quoted_text: &'a str,
    pub body: &'a str,
}

/// The ids written by [`create_with_communication`].
#[derive(Debug, Clone, Copy)]
pub struct CreatedComment {
    pub comment_id: Uuid,
    pub communication_id: Uuid,
}

/// Errors reading or writing a document comment.
#[derive(Debug, thiserror::Error)]
pub enum DocumentCommentError {
    #[error("surreal: {0}")]
    Surreal(#[from] surrealdb::Error),
    #[error("communications: {0}")]
    Communication(#[from] crate::communications::CommunicationError),
    /// A write reported success but returned no row, or returned one this
    /// module could not read back.
    #[error("writing a document comment returned no usable row")]
    WriteReturnedNothing,
}

fn many(mut response: surrealdb::IndexedResults) -> Result<Vec<DocumentComment>, surrealdb::Error> {
    let rows: Vec<DocumentCommentRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .filter_map(DocumentCommentRow::into_comment)
        .collect())
}

/// Run a write under the shared retry policy
/// ([`crate::surreal::retry`]), mapping whatever finally comes back to
/// this module's error.
///
/// Only the mapping lives here. How long a lost race is re-run, and
/// which engine conditions count as a lost race, are one policy for the
/// whole crate.
async fn writing<F, Q>(attempt: F) -> Result<surrealdb::IndexedResults, surrealdb::Error>
where
    F: FnMut() -> Q,
    Q: std::future::IntoFuture<Output = Result<surrealdb::IndexedResults, surrealdb::Error>>,
{
    retry::writing(attempt).await
}

#[allow(clippy::too_many_arguments)]
async fn create_row(
    db: &SurrealDb,
    review_document_id: Uuid,
    person_id: Uuid,
    anchor_start: i32,
    anchor_end: i32,
    quoted_text: &str,
    body: &str,
    communication_id: Option<Uuid>,
) -> Result<Uuid, surrealdb::Error> {
    let id = Uuid::now_v7();
    let mut response = db
        .query(format!(
            "CREATE $id SET \
             review_document_id = $review_document_id, person_id = $person_id, \
             anchor_start = $anchor_start, anchor_end = $anchor_end, \
             quoted_text = $quoted_text, body = $body, resolved = false, \
             communication_id = $communication_id \
             RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind((
            "review_document_id",
            record_id(crate::review_documents::TABLE, review_document_id),
        ))
        .bind(("person_id", record_id("person", person_id)))
        .bind(("anchor_start", anchor_start))
        .bind(("anchor_end", anchor_end))
        .bind(("quoted_text", quoted_text.to_string()))
        .bind(("body", body.to_string()))
        .bind(("communication_id", communication_id))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<DocumentCommentRow> = response.take(0)?;
    Ok(row
        .and_then(DocumentCommentRow::into_comment)
        .map_or(id, |c| c.id))
}

/// Insert one comment (always `resolved = false`), returning its id.
///
/// # Errors
///
/// [`DocumentCommentError::Surreal`] if the insert fails.
pub async fn create(db: &SurrealDb, new: &NewComment<'_>) -> Result<Uuid, DocumentCommentError> {
    Ok(create_row(
        db,
        new.review_document_id,
        new.person_id,
        new.anchor_start,
        new.anchor_end,
        new.quoted_text,
        new.body,
        None,
    )
    .await?)
}

/// Create a comment **and** its `communications` spine row — see the
/// module header for why the two writes cannot share a transaction. The
/// spine carries the message (body, author, direction, when); the
/// `document_comments` satellite carries the channel-specific anchor
/// (range + quoted text) and points at the spine.
///
/// # Errors
///
/// [`DocumentCommentError::Communication`] if the spine insert fails (the
/// satellite is never attempted), or [`DocumentCommentError::Surreal`] if
/// the satellite insert fails after the spine row is already committed.
pub async fn create_with_communication(
    surreal: &SurrealDb,
    new: &NewLinkedComment<'_>,
) -> Result<CreatedComment, DocumentCommentError> {
    let now = Utc::now().to_rfc3339();
    let ingested = crate::communications::ingest(
        surreal,
        &crate::communications::IngestArgs {
            project_id: new.project_id,
            channel: crate::communications::channel::DOCUMENT_COMMENT,
            direction: new.direction,
            author_person_id: Some(new.person_id),
            counterparty: None,
            subject: None,
            body: new.body,
            source_ref: None,
            asset_id: None,
            occurred_at: &now,
        },
    )
    .await?;

    let comment_id = create_row(
        surreal,
        new.review_document_id,
        new.person_id,
        new.anchor_start,
        new.anchor_end,
        new.quoted_text,
        new.body,
        Some(ingested.communication_id),
    )
    .await?;

    Ok(CreatedComment {
        comment_id,
        communication_id: ingested.communication_id,
    })
}

/// One comment by id.
///
/// # Errors
///
/// [`DocumentCommentError::Surreal`] if the lookup fails.
pub async fn find_by_id(
    db: &SurrealDb,
    id: Uuid,
) -> Result<Option<DocumentComment>, DocumentCommentError> {
    let mut response = db
        .query(format!("SELECT {SELECT} FROM ONLY $id LIMIT 1"))
        .bind(("id", record_id(TABLE, id)))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    let row: Option<DocumentCommentRow> = response.take(0)?;
    Ok(row.and_then(DocumentCommentRow::into_comment))
}

/// All comments on a review document, oldest first.
///
/// # Errors
///
/// [`DocumentCommentError::Surreal`] if the lookup fails.
pub async fn for_review_document(
    db: &SurrealDb,
    review_document_id: Uuid,
) -> Result<Vec<DocumentComment>, DocumentCommentError> {
    let response = db
        .query(format!(
            "SELECT {SELECT} FROM {TABLE} WHERE review_document_id = $review_document \
             ORDER BY id ASC"
        ))
        .bind((
            "review_document",
            record_id(crate::review_documents::TABLE, review_document_id),
        ))
        .await
        .and_then(surrealdb::IndexedResults::check)?;
    Ok(many(response)?)
}

/// Flip the `resolved` flag on one comment. Returns the updated row, or
/// `Ok(None)` if no row matched.
///
/// # Errors
///
/// [`DocumentCommentError::Surreal`] if the write fails.
pub async fn set_resolved(
    db: &SurrealDb,
    id: Uuid,
    resolved: bool,
) -> Result<Option<DocumentComment>, DocumentCommentError> {
    let mut response = writing(|| {
        db.query(format!(
            "UPDATE $id SET resolved = $resolved, updated_at = time::now() RETURN {SELECT}"
        ))
        .bind(("id", record_id(TABLE, id)))
        .bind(("resolved", resolved))
    })
    .await?;
    let row: Option<DocumentCommentRow> = response.take(0)?;
    Ok(row.and_then(DocumentCommentRow::into_comment))
}

/// Clear `communication_id` on every comment that names one of
/// `communication_ids` — the retention sweep's link-clear step, run against
/// this satellite before the spine rows it points at are deleted.
/// `communication_id` is a bare `uuid` (see the module header), so this is
/// a plain equality match, not a record link.
///
/// # Errors
///
/// [`DocumentCommentError::Surreal`] if the write fails.
pub async fn clear_communication_links(
    db: &SurrealDb,
    communication_ids: &[Uuid],
) -> Result<(), DocumentCommentError> {
    if communication_ids.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = communication_ids.to_vec();
    writing(|| {
        db.query(format!(
            "UPDATE {TABLE} SET communication_id = NONE, updated_at = time::now() \
             WHERE communication_id IN $ids"
        ))
        .bind(("ids", ids.clone()))
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{create, create_with_communication, for_review_document, set_resolved, NewComment};
    use crate::review_documents::{self, NewReviewDocument};
    use crate::surreal::test_support::mem;
    use crate::test_support::seed_notation;

    async fn seed_review_document(surreal: &crate::surreal::SurrealDb) -> uuid::Uuid {
        let notation_id = seed_notation(surreal).await;
        review_documents::create(
            surreal,
            &NewReviewDocument {
                notation_id,
                kind: "will",
                title: "Last Will and Testament",
                body_html: "<p>I, Libra, leave everything to Taurus.</p>",
            },
        )
        .await
        .unwrap()
    }

    async fn seed_commenter(surreal: &crate::surreal::SurrealDb) -> uuid::Uuid {
        crate::persons::create(
            surreal,
            &crate::persons::NewPerson::new("Taurus", "taurus@example.com"),
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_inserts_an_unresolved_comment_readable_by_document() {
        let surreal = mem().await;
        let review_document_id = seed_review_document(&surreal).await;
        let person_id = seed_commenter(&surreal).await;

        let id = create(
            &surreal,
            &NewComment {
                review_document_id,
                person_id,
                anchor_start: 3,
                anchor_end: 8,
                quoted_text: "Libra",
                body: "Should this be my full legal name?",
            },
        )
        .await
        .unwrap();

        let rows = for_review_document(&surreal, review_document_id)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].quoted_text, "Libra");
        assert_eq!(rows[0].anchor_start, 3);
        assert!(!rows[0].resolved);
    }

    #[tokio::test]
    async fn create_with_communication_writes_and_links_both_rows() {
        use crate::communications::{channel, direction};

        let surreal = crate::surreal::test_support::mem().await;
        let review_document_id = seed_review_document(&surreal).await;
        let notation_id = review_documents::by_id(&surreal, review_document_id)
            .await
            .unwrap()
            .unwrap()
            .notation_id;
        let project_id = crate::notations::find_by_id(&surreal, notation_id)
            .await
            .unwrap()
            .unwrap()
            .project_id;
        let person_id = seed_commenter(&surreal).await;

        let created = create_with_communication(
            &surreal,
            &super::NewLinkedComment {
                project_id,
                review_document_id,
                person_id,
                direction: direction::INBOUND,
                anchor_start: 3,
                anchor_end: 8,
                quoted_text: "Libra",
                body: "Should this be my full legal name?",
            },
        )
        .await
        .unwrap();

        // The satellite carries the anchor and points at the spine.
        let comment = super::find_by_id(&surreal, created.comment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(comment.communication_id, Some(created.communication_id));
        assert_eq!(comment.quoted_text, "Libra");

        // The spine row is in the matter's conversation log.
        let thread = crate::communications::for_project(&surreal, project_id)
            .await
            .unwrap();
        assert_eq!(thread.len(), 1);
        assert_eq!(thread[0].id, created.communication_id);
        assert_eq!(thread[0].channel, channel::DOCUMENT_COMMENT);
        assert_eq!(thread[0].direction, direction::INBOUND);
        assert_eq!(thread[0].body, "Should this be my full legal name?");
        assert_eq!(thread[0].author_person_id, Some(person_id));
    }

    #[tokio::test]
    async fn set_resolved_flips_the_flag() {
        let surreal = mem().await;
        let review_document_id = seed_review_document(&surreal).await;
        let person_id = seed_commenter(&surreal).await;
        let id = create(
            &surreal,
            &NewComment {
                review_document_id,
                person_id,
                anchor_start: 0,
                anchor_end: 1,
                quoted_text: "I",
                body: "typo here",
            },
        )
        .await
        .unwrap();

        let updated = set_resolved(&surreal, id, true).await.unwrap().unwrap();
        assert!(updated.resolved);
    }
}
