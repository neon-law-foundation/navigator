//! Helpers for the `expunge_requests` table — a client's request to
//! delete one of their matter documents, awaiting attorney
//! authorization.
//!
//! A client can only *ask*: [`create`] inserts a `pending` row. A
//! staff/admin then resolves it — [`authorize`] (after running the
//! admin-gated expunge, passing the resulting audit-row id) or
//! [`deny`]. The executed expunge is always category `client_request`.
//! See [`crate::expunge_records`] and the design §9.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::expunge_request::{self, STATUS_AUTHORIZED, STATUS_DENIED, STATUS_PENDING};
use crate::Db;

/// What to record for one client deletion request.
#[derive(Debug, Clone)]
pub struct NewExpungeRequest<'a> {
    pub project_id: Uuid,
    pub document_id: Uuid,
    /// The client asking for deletion.
    pub requested_by_person_id: Uuid,
    /// Optional non-content note (the client's stated reason).
    pub note: Option<&'a str>,
}

/// Insert one `expunge_requests` row at `status = pending`, returning its
/// id. The request never deletes anything on its own — a staff/admin must
/// authorize it.
///
/// # Errors
/// Propagates any database error.
pub async fn create(db: &Db, new: &NewExpungeRequest<'_>) -> Result<Uuid, sea_orm::DbErr> {
    let row = expunge_request::ActiveModel {
        project_id: ActiveValue::Set(new.project_id),
        document_id: ActiveValue::Set(new.document_id),
        requested_by_person_id: ActiveValue::Set(new.requested_by_person_id),
        status: ActiveValue::Set(STATUS_PENDING.to_string()),
        note: ActiveValue::Set(new.note.map(String::from)),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(row.id)
}

/// Load one request by id.
///
/// # Errors
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<expunge_request::Model>, sea_orm::DbErr> {
    expunge_request::Entity::find_by_id(id).one(db).await
}

/// The pending request for a document, if any. Used to show the client
/// "deletion requested" instead of offering the control again.
///
/// # Errors
/// Propagates any database error.
pub async fn pending_for_document(
    db: &Db,
    document_id: Uuid,
) -> Result<Option<expunge_request::Model>, sea_orm::DbErr> {
    expunge_request::Entity::find()
        .filter(expunge_request::Column::DocumentId.eq(document_id))
        .filter(expunge_request::Column::Status.eq(STATUS_PENDING))
        .one(db)
        .await
}

/// Every pending request across all matters, oldest first — the staff
/// authorization queue.
///
/// # Errors
/// Propagates any database error.
pub async fn list_pending(db: &Db) -> Result<Vec<expunge_request::Model>, sea_orm::DbErr> {
    expunge_request::Entity::find()
        .filter(expunge_request::Column::Status.eq(STATUS_PENDING))
        .order_by_asc(expunge_request::Column::Id)
        .all(db)
        .await
}

/// Mark a request `authorized`, recording who resolved it and the audit
/// row id from the executed expunge. Returns the updated row, or
/// `Ok(None)` if no row matched.
///
/// # Errors
/// Propagates any database error.
pub async fn authorize(
    db: &Db,
    id: Uuid,
    resolved_by_person_id: Uuid,
    expunge_record_id: Uuid,
) -> Result<Option<expunge_request::Model>, sea_orm::DbErr> {
    let Some(row) = expunge_request::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: expunge_request::ActiveModel = row.into();
    active.status = ActiveValue::Set(STATUS_AUTHORIZED.to_string());
    active.resolved_by_person_id = ActiveValue::Set(Some(resolved_by_person_id));
    active.expunge_record_id = ActiveValue::Set(Some(expunge_record_id));
    Ok(Some(active.update(db).await?))
}

/// Mark a request `denied`, recording who resolved it. Nothing is
/// deleted. Returns the updated row, or `Ok(None)` if no row matched.
///
/// # Errors
/// Propagates any database error.
pub async fn deny(
    db: &Db,
    id: Uuid,
    resolved_by_person_id: Uuid,
) -> Result<Option<expunge_request::Model>, sea_orm::DbErr> {
    let Some(row) = expunge_request::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut active: expunge_request::ActiveModel = row.into();
    active.status = ActiveValue::Set(STATUS_DENIED.to_string());
    active.resolved_by_person_id = ActiveValue::Set(Some(resolved_by_person_id));
    Ok(Some(active.update(db).await?))
}

#[cfg(test)]
mod tests {
    use super::{
        authorize, by_id, create, deny, list_pending, pending_for_document, NewExpungeRequest,
    };
    use crate::entity::expunge_request::{STATUS_AUTHORIZED, STATUS_DENIED, STATUS_PENDING};
    use crate::entity::{blob, document, expunge_record, person, project};
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use uuid::Uuid;

    /// Seed a (person, project, document) chain and return their ids.
    async fn seed(db: &crate::Db) -> (Uuid, Uuid, Uuid) {
        let client = person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        let proj = project::ActiveModel {
            name: ActiveValue::Set("Matter".into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        let blob = blob::ActiveModel {
            storage_key: ActiveValue::Set(format!("blobs/{}", Uuid::now_v7())),
            content_type: ActiveValue::Set("application/pdf".into()),
            byte_size: ActiveValue::Set(3),
            sha256_hex: ActiveValue::Set("deadbeef".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        let doc = document::ActiveModel {
            project_id: ActiveValue::Set(proj),
            blob_id: ActiveValue::Set(blob),
            filename: ActiveValue::Set("privileged.pdf".into()),
            kind: ActiveValue::Set("unclassified".into()),
            source: ActiveValue::Set("upload".into()),
            received_at: ActiveValue::Set("2026-06-04T00:00:00Z".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id;
        (client, proj, doc)
    }

    #[tokio::test]
    async fn create_defaults_to_pending_and_is_findable() {
        let db = crate::test_support::pg().await;
        let (client, proj, doc) = seed(&db).await;

        let id = create(
            &db,
            &NewExpungeRequest {
                project_id: proj,
                document_id: doc,
                requested_by_person_id: client,
                note: Some("please remove this"),
            },
        )
        .await
        .unwrap();

        let row = by_id(&db, id).await.unwrap().unwrap();
        assert_eq!(row.status, STATUS_PENDING);
        assert_eq!(row.note.as_deref(), Some("please remove this"));
        assert!(row.resolved_by_person_id.is_none());

        assert_eq!(
            pending_for_document(&db, doc).await.unwrap().map(|r| r.id),
            Some(id)
        );
        assert_eq!(list_pending(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn authorize_links_the_audit_row_and_clears_the_queue() {
        let db = crate::test_support::pg().await;
        let (client, proj, doc) = seed(&db).await;
        let admin = person::ActiveModel {
            name: ActiveValue::Set("Nick".into()),
            email: ActiveValue::Set("nick@neonlaw.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let id = create(
            &db,
            &NewExpungeRequest {
                project_id: proj,
                document_id: doc,
                requested_by_person_id: client,
                note: None,
            },
        )
        .await
        .unwrap();
        // A standalone audit row to link.
        let record_id = expunge_record::ActiveModel {
            project_id: ActiveValue::Set(proj),
            path: ActiveValue::Set("privileged.pdf".into()),
            category: ActiveValue::Set(expunge_record::CATEGORY_CLIENT_REQUEST.into()),
            authorized_by_person_id: ActiveValue::Set(admin),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;

        let updated = authorize(&db, id, admin, record_id).await.unwrap().unwrap();
        assert_eq!(updated.status, STATUS_AUTHORIZED);
        assert_eq!(updated.resolved_by_person_id, Some(admin));
        assert_eq!(updated.expunge_record_id, Some(record_id));
        // No longer pending → off the queue and not offered to the client.
        assert!(pending_for_document(&db, doc).await.unwrap().is_none());
        assert!(list_pending(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deny_resolves_without_deleting() {
        let db = crate::test_support::pg().await;
        let (client, proj, doc) = seed(&db).await;
        let staff = person::ActiveModel {
            name: ActiveValue::Set("Staff".into()),
            email: ActiveValue::Set("staff@neonlaw.com".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let id = create(
            &db,
            &NewExpungeRequest {
                project_id: proj,
                document_id: doc,
                requested_by_person_id: client,
                note: None,
            },
        )
        .await
        .unwrap();

        let updated = deny(&db, id, staff).await.unwrap().unwrap();
        assert_eq!(updated.status, STATUS_DENIED);
        assert_eq!(updated.resolved_by_person_id, Some(staff));
        assert!(updated.expunge_record_id.is_none());
        assert!(list_pending(&db).await.unwrap().is_empty());
    }
}
