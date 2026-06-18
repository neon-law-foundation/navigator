//! Helpers for the `review_documents` table — the attorney-reviewed
//! drafts a client reads and comments on before signing.
//!
//! Kept beside the other orchestration helpers so `web` and the
//! generation workflow reach them without re-importing the entity. The
//! generation workflow inserts a draft (`status = draft`); an attorney
//! advances it to `pending_review`; the client's sign-off advances it to
//! `approved`.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait,
};
use uuid::Uuid;

use crate::entity::review_document::STATUS_DRAFT;
use crate::entity::{notation, review_document};
use crate::Db;

/// What to record for one reviewable draft. `status` defaults to
/// [`STATUS_DRAFT`] via [`create`] — a freshly generated draft is never
/// visible to the client until an attorney advances it.
#[derive(Debug, Clone)]
pub struct NewReviewDocument<'a> {
    pub notation_id: Uuid,
    /// Document kind within the matter (`will`, `trust`, …).
    pub kind: &'a str,
    /// Human-readable title shown to the client.
    pub title: &'a str,
    /// Attorney-reviewed draft body as sanitized HTML.
    pub body_html: &'a str,
}

/// Insert one `review_documents` row at `status = draft`, returning its
/// id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn create(db: &Db, new: &NewReviewDocument<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = review_document::ActiveModel {
        notation_id: ActiveValue::Set(new.notation_id),
        kind: ActiveValue::Set(new.kind.to_string()),
        title: ActiveValue::Set(new.title.to_string()),
        body_html: ActiveValue::Set(new.body_html.to_string()),
        status: ActiveValue::Set(STATUS_DRAFT.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Load one review document by id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<review_document::Model>, sea_orm::DbErr> {
    review_document::Entity::find_by_id(id).one(db).await
}

/// All review documents for a notation, oldest first.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Vec<review_document::Model>, sea_orm::DbErr> {
    review_document::Entity::find()
        .filter(review_document::Column::NotationId.eq(notation_id))
        .order_by_asc(review_document::Column::Id)
        .all(db)
        .await
}

/// All client-visible review documents for a project — those whose
/// notation belongs to the project and whose status has been advanced
/// past `draft`. Joined in one query so the matter page can list a
/// client's documents-to-review without N+1 lookups.
///
/// # Errors
///
/// Propagates any database error.
pub async fn client_visible_for_project(
    db: &Db,
    project_id: Uuid,
) -> Result<Vec<review_document::Model>, sea_orm::DbErr> {
    review_document::Entity::find()
        .join(
            sea_orm::JoinType::InnerJoin,
            review_document::Relation::Notation.def(),
        )
        .filter(notation::Column::ProjectId.eq(project_id))
        .filter(review_document::Column::Status.ne(STATUS_DRAFT))
        .order_by_asc(review_document::Column::Id)
        .all(db)
        .await
}

/// Move a review document to a new `status`. Returns the updated row.
///
/// # Errors
///
/// Propagates any database error; returns `Ok(None)` if no row matched.
pub async fn set_status(
    db: &Db,
    id: Uuid,
    status: &str,
) -> Result<Option<review_document::Model>, sea_orm::DbErr> {
    let Some(row) = review_document::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: review_document::ActiveModel = row.into();
    active.status = ActiveValue::Set(status.to_string());
    Ok(Some(active.update(db).await?))
}

#[cfg(test)]
mod tests {
    use super::{by_id, create, for_notation, set_status, NewReviewDocument};
    use crate::entity::review_document::{STATUS_DRAFT, STATUS_PENDING_REVIEW};
    use crate::test_support::seed_notation;

    #[tokio::test]
    async fn create_defaults_to_draft_and_is_readable_by_notation() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;

        let id = create(
            &db,
            &NewReviewDocument {
                notation_id,
                kind: "will",
                title: "Last Will and Testament",
                body_html: "<h1>Will</h1><p>I, Libra…</p>",
            },
        )
        .await
        .unwrap();

        let row = by_id(&db, id).await.unwrap().unwrap();
        assert_eq!(row.kind, "will");
        assert_eq!(row.status, STATUS_DRAFT);
        assert!(row.body_html.contains("Libra"));

        let all = for_notation(&db, notation_id).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
    }

    #[tokio::test]
    async fn client_visible_for_project_hides_drafts() {
        use crate::entity::notation;
        use sea_orm::EntityTrait;

        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let project_id = notation::Entity::find_by_id(notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .project_id;

        let hidden = create(
            &db,
            &NewReviewDocument {
                notation_id,
                kind: "trust",
                title: "Trust (draft)",
                body_html: "<p>x</p>",
            },
        )
        .await
        .unwrap();
        let shown = create(
            &db,
            &NewReviewDocument {
                notation_id,
                kind: "will",
                title: "Will (ready)",
                body_html: "<p>y</p>",
            },
        )
        .await
        .unwrap();
        super::set_status(&db, shown, STATUS_PENDING_REVIEW)
            .await
            .unwrap();

        let visible = super::client_visible_for_project(&db, project_id)
            .await
            .unwrap();
        let ids: Vec<_> = visible.iter().map(|d| d.id).collect();
        assert!(ids.contains(&shown));
        assert!(!ids.contains(&hidden));
    }

    #[tokio::test]
    async fn set_status_advances_the_draft() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let id = create(
            &db,
            &NewReviewDocument {
                notation_id,
                kind: "trust",
                title: "Revocable Living Trust",
                body_html: "<p>Trust</p>",
            },
        )
        .await
        .unwrap();

        let updated = set_status(&db, id, STATUS_PENDING_REVIEW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, STATUS_PENDING_REVIEW);
    }
}
