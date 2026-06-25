//! Notation reads/writes that don't belong on the bare entity.
//!
//! Today this owns the **admin-discretion discount** — the recorded
//! decision that an engagement was billed below its catalog list price.
//! Neon Law Navigator is the system of record for that decision (the audit trail);
//! Xero does the client-facing math (see `billing::LineDiscount`). The
//! list price itself never lives here — it stays in the `products`
//! catalog. This module records only *how far below* list, *why*, *who*
//! approved it, and *when*.

use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, IntoActiveModel};
use uuid::Uuid;

use crate::entity::notation;
use crate::Db;

/// The discount decision to record on a notation. Exactly one of
/// [`pct`](Discount::pct) / [`amount_cents`](Discount::amount_cents) must
/// be set — a discount is either a percentage or a flat amount, never
/// both. `reason` is the recorded basis (hardship / pro bono / PPP /
/// mission), required so every discount carries its justification.
#[derive(Debug, Clone)]
pub struct Discount {
    pub pct: Option<i32>,
    pub amount_cents: Option<i64>,
    pub reason: String,
}

/// Errors from [`record_discount`].
#[derive(Debug, thiserror::Error)]
pub enum DiscountError {
    #[error("notation {0} not found")]
    NotFound(Uuid),
    #[error(
        "exactly one of discount percent / amount must be set (got pct={pct:?}, amount={amount:?})"
    )]
    NotExactlyOne {
        pct: Option<i32>,
        amount: Option<i64>,
    },
    #[error("discount percent must be between 0 and 100, got {0}")]
    PercentOutOfRange(i32),
    #[error("discount reason is required")]
    MissingReason,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

/// Record an admin-discretion discount on a notation. Validates that
/// exactly one of percent/amount is set, that a percent is `0..=100`, and
/// that a reason is present. Stamps `approved_by` + `approved_at` (RFC
/// 3339) as the audit trail. Returns the updated row.
///
/// Note: this records that the engagement is billed *below* list; the
/// guardrail that the discount does not exceed list is enforced against
/// the catalog list price at invoice-raise time
/// (`billing::MatterCloseInvoiceRequest::validate_discount`), because
/// "below list" is only meaningful against the resolved list price.
///
/// # Errors
///
/// [`DiscountError`] when the notation is missing or the discount inputs
/// are malformed.
pub async fn record_discount(
    db: &Db,
    notation_id: Uuid,
    discount: &Discount,
    approved_by: &str,
    approved_at: &str,
) -> Result<notation::Model, DiscountError> {
    if discount.pct.is_some() == discount.amount_cents.is_some() {
        return Err(DiscountError::NotExactlyOne {
            pct: discount.pct,
            amount: discount.amount_cents,
        });
    }
    if let Some(pct) = discount.pct {
        if !(0..=100).contains(&pct) {
            return Err(DiscountError::PercentOutOfRange(pct));
        }
    }
    if discount.reason.trim().is_empty() {
        return Err(DiscountError::MissingReason);
    }

    let row = notation::Entity::find_by_id(notation_id)
        .one(db)
        .await?
        .ok_or(DiscountError::NotFound(notation_id))?;
    let mut active = row.into_active_model();
    active.discount_pct = ActiveValue::Set(discount.pct);
    active.discount_amount_cents = ActiveValue::Set(discount.amount_cents);
    active.discount_reason = ActiveValue::Set(Some(discount.reason.clone()));
    active.discount_approved_by = ActiveValue::Set(Some(approved_by.to_string()));
    active.discount_approved_at = ActiveValue::Set(Some(approved_at.to_string()));
    Ok(active.update(db).await?)
}

#[cfg(test)]
mod tests {
    use super::{record_discount, Discount, DiscountError};
    use crate::entity::{notation, project, template};
    use crate::test_support::pg;
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use uuid::Uuid;

    async fn a_notation(db: &crate::Db) -> Uuid {
        let __dri = crate::test_support::dri_person(db).await;
        let project = project::ActiveModel {
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
        let template = template::ActiveModel {
            code: ActiveValue::Set("onboarding__estate".into()),
            title: ActiveValue::Set("Estate".into()),
            respondent_type: ActiveValue::Set("person".into()),
            project_id: ActiveValue::Set(None),
            blob_id: ActiveValue::Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let person = crate::entity::person::ActiveModel {
            name: ActiveValue::Set("Libra".into()),
            email: ActiveValue::Set("libra@example.com".into()),
            oidc_subject: ActiveValue::Set(None),
            role: ActiveValue::Set(crate::entity::person::Role::Client),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        notation::ActiveModel {
            template_id: ActiveValue::Set(template.id),
            person_id: ActiveValue::Set(person.id),
            entity_id: ActiveValue::Set(None),
            project_id: ActiveValue::Set(project.id),
            state: ActiveValue::Set("draft".into()),
            signature_request_id: ActiveValue::Set(None),
            delivery: ActiveValue::Set(notation::DELIVERY_EMBEDDED.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn records_a_percentage_discount_with_audit_trail() {
        let db = pg().await;
        let id = a_notation(&db).await;
        let updated = record_discount(
            &db,
            id,
            &Discount {
                pct: Some(25),
                amount_cents: None,
                reason: "hardship".into(),
            },
            "nick@neonlaw.com",
            "2026-06-10T00:00:00Z",
        )
        .await
        .unwrap();
        assert_eq!(updated.discount_pct, Some(25));
        assert_eq!(updated.discount_amount_cents, None);
        assert_eq!(updated.discount_reason.as_deref(), Some("hardship"));
        assert_eq!(
            updated.discount_approved_by.as_deref(),
            Some("nick@neonlaw.com")
        );
        assert!(updated.discount_approved_at.is_some());
    }

    #[tokio::test]
    async fn rejects_both_percent_and_amount() {
        let db = pg().await;
        let id = a_notation(&db).await;
        let err = record_discount(
            &db,
            id,
            &Discount {
                pct: Some(10),
                amount_cents: Some(5000),
                reason: "pro bono".into(),
            },
            "nick@neonlaw.com",
            "2026-06-10T00:00:00Z",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DiscountError::NotExactlyOne { .. }));
    }

    #[tokio::test]
    async fn rejects_out_of_range_percent_and_empty_reason() {
        let db = pg().await;
        let id = a_notation(&db).await;
        let err = record_discount(
            &db,
            id,
            &Discount {
                pct: Some(150),
                amount_cents: None,
                reason: "mission".into(),
            },
            "nick@neonlaw.com",
            "2026-06-10T00:00:00Z",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DiscountError::PercentOutOfRange(150)));

        let err = record_discount(
            &db,
            id,
            &Discount {
                pct: Some(10),
                amount_cents: None,
                reason: "  ".into(),
            },
            "nick@neonlaw.com",
            "2026-06-10T00:00:00Z",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DiscountError::MissingReason));
    }
}
