//! Helpers for the `document_comments` table — a reader's anchored
//! comments on a [`crate::entity::review_document`].
//!
//! The review surface is read-only; comments are the only thing a client
//! writes. Kept beside the other orchestration helpers so `web` reaches
//! them without re-importing the entity.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};
use uuid::Uuid;

use crate::entity::{communication, document_comment};
use crate::Db;

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

/// Insert one comment (always `resolved = false`), returning its id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn create(db: &Db, new: &NewComment<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = document_comment::ActiveModel {
        review_document_id: ActiveValue::Set(new.review_document_id),
        person_id: ActiveValue::Set(new.person_id),
        anchor_start: ActiveValue::Set(new.anchor_start),
        anchor_end: ActiveValue::Set(new.anchor_end),
        quoted_text: ActiveValue::Set(new.quoted_text.to_string()),
        body: ActiveValue::Set(new.body.to_string()),
        resolved: ActiveValue::Set(false),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// A new comment plus the spine-row fields it can't derive from the
/// satellite alone: the matter it belongs to and which way the message
/// flows. Used by [`create_with_communication`], the path the review surface
/// takes now that every comment is one entry in the matter's privileged
/// conversation log.
#[derive(Debug, Clone)]
pub struct NewLinkedComment<'a> {
    /// Matter this comment belongs to (the spine's `project_id`).
    pub project_id: Uuid,
    pub review_document_id: Uuid,
    pub person_id: Uuid,
    /// `communications` direction — `inbound` for a client's comment,
    /// `outbound` for a staff comment the client will read. See
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

/// Create a comment **and** its `communications` spine row atomically, with
/// the satellite's `communication_id` pointing at the spine. This is how a
/// comment joins the unified conversation log: the spine carries the message
/// (body, author, direction, when), the `document_comments` satellite carries
/// the channel-specific anchor (range + quoted text).
///
/// Both inserts run in one transaction so the satellite never points at a
/// spine row that doesn't exist (and vice versa).
///
/// # Errors
///
/// Propagates any database error; the transaction rolls back on failure.
pub async fn create_with_communication(
    db: &Db,
    new: &NewLinkedComment<'_>,
) -> Result<CreatedComment, sea_orm::DbErr> {
    let now = Utc::now().to_rfc3339();
    let txn = db.begin().await?;

    let communication = communication::ActiveModel {
        project_id: ActiveValue::Set(new.project_id),
        channel: ActiveValue::Set(crate::communications::channel::DOCUMENT_COMMENT.to_string()),
        direction: ActiveValue::Set(new.direction.to_string()),
        author_person_id: ActiveValue::Set(Some(new.person_id)),
        body: ActiveValue::Set(new.body.to_string()),
        occurred_at: ActiveValue::Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    let comment = document_comment::ActiveModel {
        review_document_id: ActiveValue::Set(new.review_document_id),
        person_id: ActiveValue::Set(new.person_id),
        anchor_start: ActiveValue::Set(new.anchor_start),
        anchor_end: ActiveValue::Set(new.anchor_end),
        quoted_text: ActiveValue::Set(new.quoted_text.to_string()),
        body: ActiveValue::Set(new.body.to_string()),
        resolved: ActiveValue::Set(false),
        communication_id: ActiveValue::Set(Some(communication.id)),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    txn.commit().await?;
    Ok(CreatedComment {
        comment_id: comment.id,
        communication_id: communication.id,
    })
}

/// All comments on a review document, oldest first.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_review_document(
    db: &Db,
    review_document_id: Uuid,
) -> Result<Vec<document_comment::Model>, sea_orm::DbErr> {
    document_comment::Entity::find()
        .filter(document_comment::Column::ReviewDocumentId.eq(review_document_id))
        .order_by_asc(document_comment::Column::Id)
        .all(db)
        .await
}

/// Flip the `resolved` flag on one comment. Returns the updated row, or
/// `Ok(None)` if no row matched.
///
/// # Errors
///
/// Propagates any database error.
pub async fn set_resolved(
    db: &Db,
    id: Uuid,
    resolved: bool,
) -> Result<Option<document_comment::Model>, sea_orm::DbErr> {
    let Some(row) = document_comment::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: document_comment::ActiveModel = row.into();
    active.resolved = ActiveValue::Set(resolved);
    Ok(Some(active.update(db).await?))
}

#[cfg(test)]
mod tests {
    use super::{create, create_with_communication, for_review_document, set_resolved, NewComment};
    use crate::review_documents::{self, NewReviewDocument};
    use crate::test_support::seed_notation;
    use sea_orm::{ActiveModelTrait, ActiveValue};

    async fn seed_review_document(db: &crate::Db) -> uuid::Uuid {
        let notation_id = seed_notation(db).await;
        review_documents::create(
            db,
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

    async fn seed_commenter(db: &crate::Db) -> uuid::Uuid {
        crate::entity::person::ActiveModel {
            name: ActiveValue::Set("Taurus".into()),
            email: ActiveValue::Set("taurus@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_inserts_an_unresolved_comment_readable_by_document() {
        let db = crate::test_support::pg().await;
        let review_document_id = seed_review_document(&db).await;
        let person_id = seed_commenter(&db).await;

        let id = create(
            &db,
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

        let rows = for_review_document(&db, review_document_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].quoted_text, "Libra");
        assert_eq!(rows[0].anchor_start, 3);
        assert!(!rows[0].resolved);
    }

    #[tokio::test]
    async fn create_with_communication_writes_and_links_both_rows() {
        use crate::communications::{channel, direction};
        use sea_orm::EntityTrait;

        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let project_id = crate::entity::notation::Entity::find_by_id(notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .project_id;
        let review_document_id = crate::review_documents::create(
            &db,
            &crate::review_documents::NewReviewDocument {
                notation_id,
                kind: "will",
                title: "Last Will and Testament",
                body_html: "<p>I, Libra, leave everything to Taurus.</p>",
            },
        )
        .await
        .unwrap();
        let person_id = seed_commenter(&db).await;

        let created = create_with_communication(
            &db,
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
        let comment = crate::entity::document_comment::Entity::find_by_id(created.comment_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(comment.communication_id, Some(created.communication_id));
        assert_eq!(comment.quoted_text, "Libra");

        // The spine row is in the matter's conversation log.
        let thread = crate::communications::for_project(&db, project_id)
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
        let db = crate::test_support::pg().await;
        let review_document_id = seed_review_document(&db).await;
        let person_id = seed_commenter(&db).await;
        let id = create(
            &db,
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

        let updated = set_resolved(&db, id, true).await.unwrap().unwrap();
        assert!(updated.resolved);
    }
}
