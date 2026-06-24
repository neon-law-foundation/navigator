//! Insert helper for the `filings` table — the durable record of one
//! outbound compliance submission.
//!
//! Called by the workflow worker inside a submission step's `ctx.run`
//! (`mailroom_send`, `certified_mail`, `e_filing`, `filing__*`). Kept
//! here, beside the other orchestration helpers, so `web` / the worker
//! reach it without re-importing the entity.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::filing;
use crate::Db;

/// What to record for one submission. `submitted_at` is stamped by the
/// caller (the worker stamps it inside the journaled step so a replay
/// reuses the same timestamp).
#[derive(Debug, Clone)]
pub struct NewFiling<'a> {
    pub notation_id: Uuid,
    /// Submission step kind / state prefix (`mailroom_send`, …).
    pub kind: &'a str,
    /// Recipient office or party.
    pub office: &'a str,
    /// Human-readable summary of what was submitted.
    pub summary: &'a str,
    /// Provider/office tracking reference, when known at submit time.
    pub reference: Option<&'a str>,
    /// RFC 3339 timestamp the submission fired.
    pub submitted_at: &'a str,
}

/// Insert one `filings` row, returning its id.
///
/// # Errors
///
/// Propagates any database error.
pub async fn record(db: &Db, new: &NewFiling<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = filing::ActiveModel {
        notation_id: ActiveValue::Set(new.notation_id),
        kind: ActiveValue::Set(new.kind.to_string()),
        office: ActiveValue::Set(new.office.to_string()),
        reference: ActiveValue::Set(new.reference.map(str::to_string)),
        summary: ActiveValue::Set(new.summary.to_string()),
        submitted_at: ActiveValue::Set(new.submitted_at.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// All filings recorded for a notation, oldest first.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Vec<filing::Model>, sea_orm::DbErr> {
    filing::Entity::find()
        .filter(filing::Column::NotationId.eq(notation_id))
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::{for_notation, record, NewFiling};
    use crate::entity::{notation, person, project, template};
    use sea_orm::{ActiveModelTrait, ActiveValue};

    async fn seed_notation(db: &crate::Db) -> uuid::Uuid {
        let tmpl = template::ActiveModel {
            code: ActiveValue::Set("annual_report__nevada".into()),
            title: ActiveValue::Set("NV Annual Report".into()),
            respondent_type: ActiveValue::Set("entity".into()),
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
        let __dri = crate::test_support::dri_person(db).await;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
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
        .id
    }

    #[tokio::test]
    async fn record_inserts_a_filing_row_readable_by_notation() {
        let db = crate::test_support::pg().await;
        let notation_id = seed_notation(&db).await;
        let id = record(
            &db,
            &NewFiling {
                notation_id,
                kind: "mailroom_send",
                office: "Nevada Secretary of State",
                summary: "Annual report mailed",
                reference: None,
                submitted_at: "2026-06-01T00:00:00Z",
            },
        )
        .await
        .unwrap();

        let rows = for_notation(&db, notation_id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].kind, "mailroom_send");
        assert_eq!(rows[0].office, "Nevada Secretary of State");
        assert!(rows[0].reference.is_none());
        assert!(!rows[0].inserted_at.is_empty());
    }
}
