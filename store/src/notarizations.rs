//! `notarizations` reads/writes — the notary counterpart to
//! [`crate::signatures`]. A Notation's document sent for remote online
//! notarization records a row here, correlated back from the provider by
//! `(provider, provider_id)`.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::notarization;
use crate::entity::signature::SignatureProvider;
use crate::Db;

/// Record the provider's request id for a Notation's notarization.
/// Idempotent on `(provider, provider_id)`.
pub async fn record_request(
    db: &Db,
    notation_id: Uuid,
    provider: SignatureProvider,
    provider_id: &str,
) -> Result<notarization::Model, sea_orm::DbErr> {
    if let Some(existing) = by_provider(db, provider, provider_id).await? {
        return Ok(existing);
    }
    notarization::ActiveModel {
        notation_id: ActiveValue::Set(notation_id),
        provider: ActiveValue::Set(provider),
        provider_id: ActiveValue::Set(provider_id.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await
}

/// The notarization row for `(provider, provider_id)`, if any.
pub async fn by_provider(
    db: &Db,
    provider: SignatureProvider,
    provider_id: &str,
) -> Result<Option<notarization::Model>, sea_orm::DbErr> {
    notarization::Entity::find()
        .filter(notarization::Column::Provider.eq(provider))
        .filter(notarization::Column::ProviderId.eq(provider_id))
        .one(db)
        .await
}

/// Stamp `notarized_at` when the provider reports completion. Returns
/// `false` for an unknown request.
pub async fn stamp_notarized(
    db: &Db,
    provider: SignatureProvider,
    provider_id: &str,
    notarized_at: &str,
) -> Result<bool, sea_orm::DbErr> {
    let Some(row) = by_provider(db, provider, provider_id).await? else {
        return Ok(false);
    };
    let mut active: notarization::ActiveModel = row.into();
    active.notarized_at = ActiveValue::Set(Some(notarized_at.to_string()));
    active.update(db).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{pg, seed_notation};

    #[tokio::test]
    async fn record_is_idempotent_and_stamp_marks_notarized() {
        let db = pg().await;
        let notation_id = seed_notation(&db).await;
        let first = record_request(&db, notation_id, SignatureProvider::DocuSign, "nz-1")
            .await
            .unwrap();
        let again = record_request(&db, notation_id, SignatureProvider::DocuSign, "nz-1")
            .await
            .unwrap();
        assert_eq!(first.id, again.id);
        assert!(stamp_notarized(
            &db,
            SignatureProvider::DocuSign,
            "nz-1",
            "2026-06-30T00:00:00Z"
        )
        .await
        .unwrap());
        let after = by_provider(&db, SignatureProvider::DocuSign, "nz-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.notarized_at.as_deref(), Some("2026-06-30T00:00:00Z"));
    }
}
