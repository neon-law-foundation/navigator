//! Person directory helpers.
//!
//! Today: caching the Xero `ContactID` on a person the first time they
//! are mirrored to Xero Contacts (one-way, Neon Law Navigator → Xero). The
//! matter-close invoice workflow resolves the client's contact and folds
//! the id back here so the admin people-detail page can deep-link to the
//! contact in Xero and future syncs are idempotent.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder,
};
use uuid::Uuid;

use crate::entity::person;
use crate::Db;

/// The firm-side person to designate as a matter's staff DRI when a create
/// path has no explicit opener — the self-serve intake (no staffer in the
/// room), the CLI, and AIDA's tool calls. Returns the lowest-id `admin`,
/// else the lowest-id `staff` — i.e. the firm principal in a seeded
/// install, resolved by **role**, not a hard-coded email, so a white-label
/// fork gets its own principal with no code change. `None` only on a DB
/// with no firm-side person at all (which the caller treats as an error).
///
/// # Errors
///
/// Propagates any database error.
pub async fn default_firm_dri<C: ConnectionTrait>(db: &C) -> Result<Option<Uuid>, sea_orm::DbErr> {
    if let Some(admin) = person::Entity::find()
        .filter(person::Column::Role.eq(person::Role::Admin))
        .order_by_asc(person::Column::Id)
        .one(db)
        .await?
    {
        return Ok(Some(admin.id));
    }
    Ok(person::Entity::find()
        .filter(person::Column::Role.eq(person::Role::Staff))
        .order_by_asc(person::Column::Id)
        .one(db)
        .await?
        .map(|p| p.id))
}

/// Cache the Xero `ContactID` on a person. No-op (`Ok(None)`) when the
/// person row no longer exists. Idempotent: re-setting the same id just
/// bumps `updated_at`.
///
/// # Errors
///
/// Propagates any database error.
pub async fn set_xero_contact_id(
    db: &Db,
    person_id: Uuid,
    xero_contact_id: &str,
) -> Result<Option<person::Model>, sea_orm::DbErr> {
    let Some(existing) = person::Entity::find_by_id(person_id).one(db).await? else {
        return Ok(None);
    };
    let mut active: person::ActiveModel = existing.into();
    active.xero_contact_id = ActiveValue::Set(Some(xero_contact_id.to_string()));
    Ok(Some(active.update(db).await?))
}

#[cfg(test)]
mod tests {
    use super::set_xero_contact_id;
    use crate::entity::person;
    use sea_orm::{ActiveModelTrait, ActiveValue};

    #[tokio::test]
    async fn set_xero_contact_id_caches_then_is_idempotent() {
        let db = crate::test_support::pg().await;
        let p = person::ActiveModel {
            name: ActiveValue::Set("Capricorn".into()),
            email: ActiveValue::Set("capricorn@example.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(p.xero_contact_id.is_none());

        let updated = set_xero_contact_id(&db, p.id, "xero-contact-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.xero_contact_id.as_deref(), Some("xero-contact-1"));

        // Re-set the same id — still one value, no error.
        let again = set_xero_contact_id(&db, p.id, "xero-contact-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again.xero_contact_id.as_deref(), Some("xero-contact-1"));
    }

    #[tokio::test]
    async fn set_xero_contact_id_is_noop_for_missing_person() {
        let db = crate::test_support::pg().await;
        let out = set_xero_contact_id(&db, uuid::Uuid::now_v7(), "x")
            .await
            .unwrap();
        assert!(out.is_none());
    }
}
