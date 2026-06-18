//! Project (matter) lifecycle helpers.
//!
//! A matter's `status` walks `open` → `closed` → `archived`
//! (`entity::project::Model::status`). Opening is done at retainer
//! intake; this module owns the *close* — flipping a matter to `closed`
//! when the firm signs its closing letter. Archival (the Drive cold
//! store) is a separate downstream step and is left untouched here.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::{notation, project};
use crate::Db;

/// The notation id of the person's **sole open matter**, for auto-routing an
/// inbound message to a matter without manual triage. Returns `Some` only
/// when the person is the client (`notations.person_id`) on exactly one
/// matter whose project is still `open`; `None` when they have none, or more
/// than one (the ambiguous case — fall back to manual `@link`).
///
/// This is the seam the email loop uses so a known client's reply lands on
/// their matter's conversation log on its own.
///
/// # Errors
///
/// Propagates any database error.
pub async fn sole_open_matter_for_person(
    db: &Db,
    person_id: Uuid,
) -> Result<Option<Uuid>, sea_orm::DbErr> {
    let notations = notation::Entity::find()
        .filter(notation::Column::PersonId.eq(person_id))
        .all(db)
        .await?;

    let mut open: Vec<Uuid> = Vec::new();
    for n in notations {
        if let Some(p) = project::Entity::find_by_id(n.project_id).one(db).await? {
            if p.status == "open" {
                open.push(n.id);
            }
        }
    }
    Ok((open.len() == 1).then(|| open[0]))
}

/// Flip the matter that `notation_id` belongs to from `open` to
/// `closed`. Returns the closed project's id, or `None` if the notation
/// (or its project) no longer exists.
///
/// Idempotent and monotonic: a matter already `closed` or `archived` is
/// left as-is — re-running never re-opens it, and a replay of the
/// firm-signature side effect is a no-op. `inserted_at`/`updated_at` are
/// maintained by the entity's active-model behavior.
pub async fn close_for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Option<Uuid>, sea_orm::DbErr> {
    let Some(n) = notation::Entity::find_by_id(notation_id).one(db).await? else {
        return Ok(None);
    };
    let Some(p) = project::Entity::find_by_id(n.project_id).one(db).await? else {
        return Ok(None);
    };
    let project_id = p.id;
    // Monotonic: don't walk backwards out of `archived`, and don't
    // churn an already-`closed` row.
    if p.status == "closed" || p.status == "archived" {
        return Ok(Some(project_id));
    }
    let mut active: project::ActiveModel = p.into();
    active.status = ActiveValue::Set("closed".into());
    // Stamp the close date — the start of the 10-year retention window.
    active.closed_at = ActiveValue::Set(Some(chrono::Utc::now().to_rfc3339()));
    active.update(db).await?;
    Ok(Some(project_id))
}

#[cfg(test)]
mod tests {
    use super::{close_for_notation, sole_open_matter_for_person};
    use crate::entity::{notation, person, project, template};
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};

    async fn seed_open_matter(db: &crate::Db) -> (uuid::Uuid, uuid::Uuid) {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("closing__letter".into()),
            title: ActiveValue::Set("Closing Letter".into()),
            respondent_type: ActiveValue::Set("person_and_entity".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let person = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let proj = project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let notation_id = notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        (notation_id, proj.id)
    }

    #[tokio::test]
    async fn close_for_notation_flips_open_to_closed() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;

        let closed = close_for_notation(&db, notation_id).await.unwrap();
        assert_eq!(closed, Some(project_id));

        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "closed");
    }

    #[tokio::test]
    async fn close_for_notation_is_idempotent_and_does_not_unarchive() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;

        // First close: open -> closed.
        close_for_notation(&db, notation_id).await.unwrap();
        // Manually archive, then re-run: must stay archived (monotonic).
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: project::ActiveModel = row.into();
        active.status = ActiveValue::Set("archived".into());
        active.update(&db).await.unwrap();

        let again = close_for_notation(&db, notation_id).await.unwrap();
        assert_eq!(again, Some(project_id));
        let row = project::Entity::find_by_id(project_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.status, "archived",
            "close must not walk back from archived"
        );
    }

    /// Open one more matter for `person_id` so a person can have several.
    async fn seed_open_matter_for(db: &crate::Db, person_id: uuid::Uuid) -> uuid::Uuid {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set(format!("onboarding__{}", uuid::Uuid::now_v7())),
            title: ActiveValue::Set("Matter".into()),
            respondent_type: ActiveValue::Set("person".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let proj = project::ActiveModel {
            name: ActiveValue::Set("another matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
            template_id: ActiveValue::Set(tmpl.id),
            person_id: ActiveValue::Set(person_id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(proj.id),
            state: ActiveValue::Set("BEGIN".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn sole_open_matter_routes_only_when_unambiguous() {
        let db = crate::test_support::pg().await;
        let (notation_id, project_id) = seed_open_matter(&db).await;
        let person_id = notation::Entity::find_by_id(notation_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .person_id;

        // Exactly one open matter → routes to it.
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            Some(notation_id)
        );

        // Close it → no open matter → no routing.
        close_for_notation(&db, notation_id).await.unwrap();
        let _ = project_id;
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            None
        );

        // Two open matters → ambiguous → no routing (manual @link instead).
        let a = seed_open_matter_for(&db, person_id).await;
        let _b = seed_open_matter_for(&db, person_id).await;
        assert_eq!(
            sole_open_matter_for_person(&db, person_id).await.unwrap(),
            None,
            "two open matters must not auto-route"
        );
        let _ = a;
    }

    #[tokio::test]
    async fn close_for_notation_returns_none_for_unknown_notation() {
        let db = crate::test_support::pg().await;
        let missing = close_for_notation(&db, uuid::Uuid::from_u128(9999))
            .await
            .unwrap();
        assert_eq!(missing, None);
    }
}
