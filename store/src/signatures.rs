//! `signatures` reads/writes.
//!
//! The e-signature request id used to sit inline on
//! `notations.signature_request_id`; it now lives in the `signatures`
//! table, correlated back from the provider by `(provider, provider_id)`.
//! These helpers are the seam every caller (the retainer walk that sends,
//! the webhook that resolves and stamps, the admin/status reads) goes
//! through so the correlation key stays in one place.

use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::signature::{self, SignatureProvider};
use crate::Db;

/// Record the provider's request id for a Notation when the envelope is
/// created. Idempotent on `(provider, provider_id)`: re-recording the same
/// envelope returns the existing row rather than inserting a duplicate. The
/// insert is a single atomic `ON CONFLICT DO UPDATE … RETURNING`, so two
/// concurrent callers for one envelope can't race a check-then-insert.
pub async fn record_request(
    db: &Db,
    notation_id: Uuid,
    provider: SignatureProvider,
    provider_id: &str,
) -> Result<signature::Model, sea_orm::DbErr> {
    let now = chrono::Utc::now().to_rfc3339();
    let row = signature::ActiveModel {
        id: ActiveValue::Set(Uuid::now_v7()),
        notation_id: ActiveValue::Set(notation_id),
        provider: ActiveValue::Set(provider),
        provider_id: ActiveValue::Set(provider_id.to_string()),
        inserted_at: ActiveValue::Set(now.clone()),
        updated_at: ActiveValue::Set(now),
        ..Default::default()
    };
    signature::Entity::insert(row)
        .on_conflict(
            // Self-update on the conflict target so RETURNING yields the
            // already-recorded row instead of erroring on DO NOTHING.
            OnConflict::columns([signature::Column::Provider, signature::Column::ProviderId])
                .update_column(signature::Column::ProviderId)
                .to_owned(),
        )
        .exec_with_returning(db)
        .await
}

/// The signature row for `(provider, provider_id)`, if any — the webhook's
/// correlation lookup.
pub async fn by_provider(
    db: &Db,
    provider: SignatureProvider,
    provider_id: &str,
) -> Result<Option<signature::Model>, sea_orm::DbErr> {
    signature::Entity::find()
        .filter(signature::Column::Provider.eq(provider))
        .filter(signature::Column::ProviderId.eq(provider_id))
        .one(db)
        .await
}

/// The provider request id sent for a Notation, if one has been sent. The
/// notation-scoped read that replaces `notation.signature_request_id`;
/// returns the most recently recorded envelope for the notation.
pub async fn request_id_for_notation(
    db: &Db,
    notation_id: Uuid,
) -> Result<Option<String>, sea_orm::DbErr> {
    Ok(signature::Entity::find()
        .filter(signature::Column::NotationId.eq(notation_id))
        .order_by_desc(signature::Column::InsertedAt)
        .one(db)
        .await?
        .map(|s| s.provider_id))
}

/// Stamp `signed_at` on the signature for `(provider, provider_id)` when the
/// provider reports completion. A no-op (returns `false`) for an unknown
/// envelope — the callback may arrive for one we never tracked, or twice.
pub async fn stamp_signed(
    db: &Db,
    provider: SignatureProvider,
    provider_id: &str,
    signed_at: &str,
) -> Result<bool, sea_orm::DbErr> {
    let Some(row) = by_provider(db, provider, provider_id).await? else {
        return Ok(false);
    };
    let mut active: signature::ActiveModel = row.into();
    active.signed_at = ActiveValue::Set(Some(signed_at.to_string()));
    active.update(db).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{pg, seed_notation};

    #[tokio::test]
    async fn record_request_is_idempotent_on_provider_and_id() {
        let db = pg().await;
        let notation_id = seed_notation(&db).await;
        let first = record_request(&db, notation_id, SignatureProvider::DocuSign, "env-1")
            .await
            .unwrap();
        let again = record_request(&db, notation_id, SignatureProvider::DocuSign, "env-1")
            .await
            .unwrap();
        assert_eq!(first.id, again.id, "same envelope must not double-insert");
        assert_eq!(
            request_id_for_notation(&db, notation_id).await.unwrap(),
            Some("env-1".to_string())
        );
    }

    #[tokio::test]
    async fn by_provider_resolves_and_stamp_marks_signed() {
        let db = pg().await;
        let notation_id = seed_notation(&db).await;
        record_request(&db, notation_id, SignatureProvider::DocuSign, "env-9")
            .await
            .unwrap();
        let resolved = by_provider(&db, SignatureProvider::DocuSign, "env-9")
            .await
            .unwrap()
            .expect("envelope resolves to its row");
        assert_eq!(resolved.notation_id, notation_id);
        assert!(resolved.signed_at.is_none());

        let stamped = stamp_signed(
            &db,
            SignatureProvider::DocuSign,
            "env-9",
            "2026-06-30T00:00:00Z",
        )
        .await
        .unwrap();
        assert!(stamped);
        let after = by_provider(&db, SignatureProvider::DocuSign, "env-9")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.signed_at.as_deref(), Some("2026-06-30T00:00:00Z"));

        // An unknown envelope is a no-op, not an error.
        assert!(!stamp_signed(&db, SignatureProvider::DocuSign, "nope", "x")
            .await
            .unwrap());
    }
}
