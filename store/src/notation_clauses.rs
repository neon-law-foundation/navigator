//! Helpers for the `notation_clauses` table — per-notation custom prose a
//! staff member adds to a single matter's assembled document.
//!
//! Kept beside the other orchestration helpers so `web` reaches them
//! without re-importing the entity. The render-time splice lives in `web`
//! (it assembles the template body); this module owns the rows.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::notation_clause;
use crate::Db;

/// The marker in a template body where a notation's custom clauses are
/// spliced, in `position` order. A body without the marker renders
/// unchanged — clauses simply don't appear.
pub const CUSTOM_CLAUSES_MARKER: &str = "{{custom_clauses}}";

/// Splice a notation's `clauses` into a template `body` at
/// [`CUSTOM_CLAUSES_MARKER`], joined as separate markdown paragraphs in
/// order. A body without the marker is returned unchanged (clauses simply
/// don't appear), so adding the feature never disturbs a template that
/// doesn't opt in.
#[must_use]
pub fn splice(body: &str, clauses: &[notation_clause::Model]) -> String {
    if !body.contains(CUSTOM_CLAUSES_MARKER) {
        return body.to_string();
    }
    let rendered = clauses
        .iter()
        .map(|c| c.body_markdown.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    body.replace(CUSTOM_CLAUSES_MARKER, &rendered)
}

/// All clauses on a notation, in render (`position`, then `id`) order.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Vec<notation_clause::Model>, sea_orm::DbErr> {
    notation_clause::Entity::find()
        .filter(notation_clause::Column::NotationId.eq(notation_id))
        .order_by_asc(notation_clause::Column::Position)
        .order_by_asc(notation_clause::Column::Id)
        .all(db)
        .await
}

/// Append one clause to a notation at the next position, returning its id.
/// The position is `max(position) + 1` so a fresh clause always renders
/// last.
///
/// # Errors
///
/// Propagates any database error.
pub async fn append(
    db: &Db,
    notation_id: Uuid,
    body_markdown: &str,
    authored_by: Option<Uuid>,
) -> Result<Uuid, sea_orm::DbErr> {
    let next_position = for_notation(db, notation_id)
        .await?
        .last()
        .map_or(0, |c| c.position + 1);
    let row = notation_clause::ActiveModel {
        notation_id: ActiveValue::Set(notation_id),
        position: ActiveValue::Set(next_position),
        body_markdown: ActiveValue::Set(body_markdown.to_string()),
        authored_by_person_id: ActiveValue::Set(authored_by),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Replace one clause's body. Returns the updated row, or `Ok(None)` if no
/// row matched.
///
/// # Errors
///
/// Propagates any database error.
pub async fn update_body(
    db: &Db,
    id: Uuid,
    body_markdown: &str,
) -> Result<Option<notation_clause::Model>, sea_orm::DbErr> {
    let Some(row) = notation_clause::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: notation_clause::ActiveModel = row.into();
    active.body_markdown = ActiveValue::Set(body_markdown.to_string());
    Ok(Some(active.update(db).await?))
}

/// Delete one clause. Returns `true` if a row was removed.
///
/// # Errors
///
/// Propagates any database error.
pub async fn delete(db: &Db, id: Uuid) -> Result<bool, sea_orm::DbErr> {
    let res = notation_clause::Entity::delete_by_id(id).exec(db).await?;
    Ok(res.rows_affected > 0)
}

/// Move one clause one step earlier (`up`) or later (`down`) in render
/// order by swapping its `position` with its neighbour's. A no-op at the
/// ends. Returns `Ok(false)` when the clause doesn't exist or can't move.
///
/// # Errors
///
/// Propagates any database error.
pub async fn move_clause(db: &Db, id: Uuid, up: bool) -> Result<bool, sea_orm::DbErr> {
    let Some(target) = notation_clause::Entity::find_by_id(id).one(db).await? else {
        return Ok(false);
    };
    let ordered = for_notation(db, target.notation_id).await?;
    let idx = ordered
        .iter()
        .position(|c| c.id == id)
        .expect("target is in its own notation's clause list");
    let neighbour_idx = if up {
        if idx == 0 {
            return Ok(false);
        }
        idx - 1
    } else {
        if idx + 1 >= ordered.len() {
            return Ok(false);
        }
        idx + 1
    };
    let neighbour = &ordered[neighbour_idx];

    // Swap positions. Two updates; the unique render order is restored by
    // the (position, id) sort even if two rows momentarily share a value.
    let (target_pos, neighbour_pos) = (target.position, neighbour.position);
    let mut a: notation_clause::ActiveModel = target.clone().into();
    a.position = ActiveValue::Set(neighbour_pos);
    a.update(db).await?;
    let mut b: notation_clause::ActiveModel = neighbour.clone().into();
    b.position = ActiveValue::Set(target_pos);
    b.update(db).await?;
    Ok(true)
}

/// Whether a notation carries any custom clause — half of the review gate
/// (the other half is any client-sourced answer).
///
/// # Errors
///
/// Propagates any database error.
pub async fn exists_for(db: &Db, notation_id: Uuid) -> Result<bool, sea_orm::DbErr> {
    Ok(notation_clause::Entity::find()
        .filter(notation_clause::Column::NotationId.eq(notation_id))
        .one(db)
        .await?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::{append, delete, exists_for, for_notation, move_clause, splice, update_body};
    use crate::test_support::seed_notation;

    #[tokio::test]
    async fn splice_renders_clauses_at_the_marker_in_order() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        append(&db, notation_id, "Governing law is Nevada.", None)
            .await
            .unwrap();
        append(&db, notation_id, "Fees are due net 30.", None)
            .await
            .unwrap();
        let clauses = for_notation(&db, notation_id).await.unwrap();

        let body = "Engagement terms.\n\n{{custom_clauses}}\n\nSignatures.";
        let rendered = splice(body, &clauses);
        assert!(rendered.contains("Governing law is Nevada."));
        assert!(rendered.contains("Fees are due net 30."));
        assert!(!rendered.contains("{{custom_clauses}}"));
        // Order preserved, governing-law clause before the fees clause.
        let law = rendered.find("Governing law").unwrap();
        let fees = rendered.find("Fees are due").unwrap();
        assert!(law < fees);
    }

    #[test]
    fn splice_leaves_a_body_without_the_marker_unchanged() {
        let body = "No marker here.";
        assert_eq!(splice(body, &[]), body);
    }

    #[tokio::test]
    async fn append_orders_clauses_and_exists_for_reports_presence() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        assert!(!exists_for(&db, notation_id).await.unwrap());

        append(&db, notation_id, "First clause.", None)
            .await
            .unwrap();
        append(&db, notation_id, "Second clause.", None)
            .await
            .unwrap();

        let clauses = for_notation(&db, notation_id).await.unwrap();
        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0].body_markdown, "First clause.");
        assert_eq!(clauses[1].body_markdown, "Second clause.");
        assert!(clauses[0].position < clauses[1].position);
        assert!(exists_for(&db, notation_id).await.unwrap());
    }

    #[tokio::test]
    async fn move_clause_swaps_render_order() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let first = append(&db, notation_id, "First.", None).await.unwrap();
        append(&db, notation_id, "Second.", None).await.unwrap();

        // Move the first clause down; "Second." now renders first.
        assert!(move_clause(&db, first, false).await.unwrap());
        let clauses = for_notation(&db, notation_id).await.unwrap();
        assert_eq!(clauses[0].body_markdown, "Second.");
        assert_eq!(clauses[1].body_markdown, "First.");

        // Moving the now-last clause down again is a no-op.
        assert!(!move_clause(&db, first, false).await.unwrap());
    }

    #[tokio::test]
    async fn update_and_delete_round_trip() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let id = append(&db, notation_id, "Draft.", None).await.unwrap();

        let updated = update_body(&db, id, "Revised.").await.unwrap().unwrap();
        assert_eq!(updated.body_markdown, "Revised.");

        assert!(delete(&db, id).await.unwrap());
        assert!(for_notation(&db, notation_id).await.unwrap().is_empty());
    }
}
