//! Record governed expunges (design §9).
//!
//! The legal council's requirement: when a matter repo's history is
//! rewritten to remove privileged / sealed / lawfully-deleted material,
//! the expunge *itself* is recorded — who authorized it, when, and the
//! category — so the redaction is auditable. This module is the
//! write/read seam for that audit log; the `web::expunge` orchestrator
//! calls [`record`] after the repo rewrite + LFS deletion.

use sea_orm::{ActiveModelTrait, ActiveValue};
use uuid::Uuid;

use crate::entity::expunge_record;
use crate::Db;

/// Inputs to [`record`].
#[derive(Debug, Clone)]
pub struct NewExpunge<'a> {
    /// The matter whose repo was rewritten.
    pub project_id: Uuid,
    /// The repo path removed (metadata, not content).
    pub path: &'a str,
    /// One of the `expunge_record::CATEGORY_*` constants.
    pub category: &'a str,
    /// The admin who authorized the expunge.
    pub authorized_by_person_id: Uuid,
    /// `main` oid before the rewrite.
    pub head_before: Option<&'a str>,
    /// `main` oid after the rewrite.
    pub head_after: Option<&'a str>,
    /// Optional non-content note (e.g. a docket reference).
    pub note: Option<&'a str>,
}

/// Insert one expunge audit row, returning its id.
///
/// # Errors
/// [`sea_orm::DbErr`] if the insert fails.
pub async fn record(db: &Db, new: &NewExpunge<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = expunge_record::ActiveModel {
        project_id: ActiveValue::Set(new.project_id),
        path: ActiveValue::Set(new.path.to_string()),
        category: ActiveValue::Set(new.category.to_string()),
        authorized_by_person_id: ActiveValue::Set(new.authorized_by_person_id),
        head_before: ActiveValue::Set(new.head_before.map(String::from)),
        head_after: ActiveValue::Set(new.head_after.map(String::from)),
        note: ActiveValue::Set(new.note.map(String::from)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{expunge_record, person, project};
    use crate::test_support::pg;
    use sea_orm::EntityTrait;

    #[tokio::test]
    async fn record_persists_the_audit_row() {
        let db = pg().await;

        let admin = person::ActiveModel {
            name: ActiveValue::Set("Nick".into()),
            email: ActiveValue::Set("nick@neonlaw.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let __dri = crate::test_support::dri_person(&db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(&db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        let id = record(
            &db,
            &NewExpunge {
                project_id: proj,
                path: "privileged.pdf",
                category: expunge_record::CATEGORY_SEALING,
                authorized_by_person_id: admin,
                head_before: Some("a".repeat(40).as_str()),
                head_after: Some("b".repeat(40).as_str()),
                note: Some("sealed per docket 24-CV-1"),
            },
        )
        .await
        .unwrap();

        let row = expunge_record::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .expect("expunge row");
        assert_eq!(row.project_id, proj);
        assert_eq!(row.category, expunge_record::CATEGORY_SEALING);
        assert_eq!(row.authorized_by_person_id, admin);
        assert_eq!(row.path, "privileged.pdf");
        assert_eq!(row.note.as_deref(), Some("sealed per docket 24-CV-1"));
    }
}
