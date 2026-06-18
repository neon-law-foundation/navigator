//! Coupon reads/writes — the reusable named discounts staff apply to a
//! subscription at sign-up.
//!
//! A coupon holds the *intent* of a standing discount (Xero has no coupon
//! object). The application flow is two steps, deliberately split so a
//! below-list check on the product can sit between them:
//!
//! 1. [`resolve`] — read-only: the coupon exists, is `active`, is not
//!    expired, matches the product scope, and has redemptions left. It
//!    returns the coupon so the caller can read its discount and validate
//!    it against the product's list price (the below-list guardrail lives
//!    in `billing::LineDiscount::validate`, which `store` does not depend
//!    on — so the web layer bridges).
//! 2. [`mark_redeemed`] — increment `redeemed_count`, called only after
//!    the subscription is committed, so a failed below-list check never
//!    burns a redemption.
//!
//! The discount itself is snapshotted onto the subscription at apply time,
//! so editing or expiring the coupon later never re-prices an existing
//! client — see [`crate::subscriptions`].

use chrono::{DateTime, Utc};
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::coupon;
use crate::Db;

/// Why a coupon could not be created or applied. The web layer maps these
/// to a 400 with the message; none is an internal error except [`Db`](CouponError::Db).
#[derive(Debug, thiserror::Error)]
pub enum CouponError {
    #[error("no coupon with code `{0}`")]
    NotFound(String),
    #[error("coupon `{0}` is not active")]
    Inactive(String),
    #[error("coupon `{0}` has expired")]
    Expired(String),
    #[error("coupon `{0}` has reached its redemption limit")]
    Exhausted(String),
    #[error("coupon `{code}` is for product `{scope}`, not `{requested}`")]
    ProductMismatch {
        code: String,
        scope: String,
        requested: String,
    },
    #[error("a coupon must set exactly one of a percent or a flat-amount discount")]
    DiscountShape,
    #[error("a percent discount must be between 0 and 100")]
    PercentRange,
    #[error("a flat-amount discount must be a positive number of cents")]
    AmountRange,
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
}

/// Fields to mint a coupon. Exactly one of `discount_percent` /
/// `discount_amount_cents` must be set (enforced by [`create`]).
#[derive(Clone, Debug)]
pub struct NewCoupon {
    pub code: String,
    pub discount_percent: Option<i32>,
    pub discount_amount_cents: Option<i64>,
    pub product_code: Option<String>,
    pub expires_at: Option<String>,
    pub max_redemptions: Option<i32>,
}

impl NewCoupon {
    /// Reject a malformed discount before it reaches the database: exactly
    /// one shape, a percent in `0..=100`, a positive flat amount.
    fn validate(&self) -> Result<(), CouponError> {
        match (self.discount_percent, self.discount_amount_cents) {
            (Some(_), Some(_)) | (None, None) => Err(CouponError::DiscountShape),
            (Some(pct), None) => {
                if (0..=100).contains(&pct) {
                    Ok(())
                } else {
                    Err(CouponError::PercentRange)
                }
            }
            (None, Some(cents)) => {
                if cents > 0 {
                    Ok(())
                } else {
                    Err(CouponError::AmountRange)
                }
            }
        }
    }
}

/// Mint a new `active`, never-redeemed coupon.
///
/// # Errors
///
/// [`CouponError::DiscountShape`] / [`CouponError::PercentRange`] /
/// [`CouponError::AmountRange`] for a malformed discount;
/// [`CouponError::Db`] for any database error (including a duplicate code,
/// which violates the unique index).
pub async fn create(db: &Db, new: NewCoupon) -> Result<coupon::Model, CouponError> {
    new.validate()?;
    let model = coupon::ActiveModel {
        code: ActiveValue::Set(new.code),
        discount_percent: ActiveValue::Set(new.discount_percent),
        discount_amount_cents: ActiveValue::Set(new.discount_amount_cents),
        product_code: ActiveValue::Set(new.product_code),
        expires_at: ActiveValue::Set(new.expires_at),
        max_redemptions: ActiveValue::Set(new.max_redemptions),
        redeemed_count: ActiveValue::Set(0),
        active: ActiveValue::Set(true),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(model)
}

/// Fetch a coupon by its exact `code`. `None` when no such coupon exists.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_code(db: &Db, code: &str) -> Result<Option<coupon::Model>, sea_orm::DbErr> {
    coupon::Entity::find()
        .filter(coupon::Column::Code.eq(code))
        .one(db)
        .await
}

/// Every coupon, newest first — backs the admin listing page.
///
/// # Errors
///
/// Propagates any database error.
pub async fn list_all(db: &Db) -> Result<Vec<coupon::Model>, sea_orm::DbErr> {
    coupon::Entity::find()
        .order_by_desc(coupon::Column::InsertedAt)
        .all(db)
        .await
}

/// Read-only application check for `code` against `product_code` at `now`:
/// the coupon must exist, be `active`, be unexpired, match the product
/// scope (a `None` scope matches anything), and have redemptions left.
/// Returns the coupon so the caller can read its discount and validate it
/// below the product's list price before committing the subscription.
///
/// This does **not** increment the redemption count — call
/// [`mark_redeemed`] once the subscription is committed.
///
/// # Errors
///
/// A [`CouponError`] naming the specific failure, or [`CouponError::Db`].
pub async fn resolve(
    db: &Db,
    code: &str,
    product_code: &str,
    now: DateTime<Utc>,
) -> Result<coupon::Model, CouponError> {
    let coupon = by_code(db, code)
        .await?
        .ok_or_else(|| CouponError::NotFound(code.to_string()))?;
    if !coupon.active {
        return Err(CouponError::Inactive(code.to_string()));
    }
    if let Some(expires_at) = &coupon.expires_at {
        // An unparseable expiry is treated as expired — fail closed rather
        // than honour a discount we can't bound in time.
        let expired =
            DateTime::parse_from_rfc3339(expires_at).map_or(true, |e| e.with_timezone(&Utc) <= now);
        if expired {
            return Err(CouponError::Expired(code.to_string()));
        }
    }
    if let Some(scope) = &coupon.product_code {
        if scope != product_code {
            return Err(CouponError::ProductMismatch {
                code: code.to_string(),
                scope: scope.clone(),
                requested: product_code.to_string(),
            });
        }
    }
    if let Some(max) = coupon.max_redemptions {
        if coupon.redeemed_count >= max {
            return Err(CouponError::Exhausted(code.to_string()));
        }
    }
    Ok(coupon)
}

/// Increment a coupon's `redeemed_count` by one. Called after the
/// subscription that applied it is committed. A missing id is a no-op.
///
/// # Errors
///
/// Propagates any database error.
pub async fn mark_redeemed(db: &Db, id: Uuid) -> Result<(), sea_orm::DbErr> {
    let Some(existing) = coupon::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let next = existing.redeemed_count + 1;
    let mut active: coupon::ActiveModel = existing.into();
    active.redeemed_count = ActiveValue::Set(next);
    active.updated_at = ActiveValue::NotSet;
    active.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{by_code, create, list_all, mark_redeemed, resolve, CouponError, NewCoupon};
    use crate::test_support::pg;
    use chrono::{Duration, Utc};

    fn percent(code: &str, pct: i32) -> NewCoupon {
        NewCoupon {
            code: code.to_string(),
            discount_percent: Some(pct),
            discount_amount_cents: None,
            product_code: Some("nexus".into()),
            expires_at: None,
            max_redemptions: None,
        }
    }

    #[tokio::test]
    async fn create_rejects_a_malformed_discount() {
        let db = pg().await;
        // Both shapes set.
        let both = NewCoupon {
            discount_amount_cents: Some(100),
            ..percent("BOTH", 50)
        };
        assert!(matches!(
            create(&db, both).await,
            Err(CouponError::DiscountShape)
        ));
        // Neither shape set.
        let neither = NewCoupon {
            discount_percent: None,
            ..percent("NEITHER", 0)
        };
        assert!(matches!(
            create(&db, neither).await,
            Err(CouponError::DiscountShape)
        ));
        // Out-of-range percent.
        assert!(matches!(
            create(&db, percent("OVER", 101)).await,
            Err(CouponError::PercentRange)
        ));
    }

    #[tokio::test]
    async fn resolve_returns_an_active_unexpired_in_scope_coupon() {
        let db = pg().await;
        let made = create(&db, percent("FRIEND99", 99)).await.unwrap();
        let got = resolve(&db, "FRIEND99", "nexus", Utc::now()).await.unwrap();
        assert_eq!(got.id, made.id);
        assert_eq!(got.discount_percent, Some(99));
    }

    #[tokio::test]
    async fn resolve_rejects_missing_inactive_and_out_of_scope() {
        let db = pg().await;
        create(&db, percent("FRIEND99", 99)).await.unwrap();
        // Unknown code.
        assert!(matches!(
            resolve(&db, "NOPE", "nexus", Utc::now()).await,
            Err(CouponError::NotFound(_))
        ));
        // Wrong product scope.
        assert!(matches!(
            resolve(&db, "FRIEND99", "nautilus", Utc::now()).await,
            Err(CouponError::ProductMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn resolve_rejects_an_expired_coupon() {
        let db = pg().await;
        let yesterday = (Utc::now() - Duration::days(1)).to_rfc3339();
        let expiring = NewCoupon {
            expires_at: Some(yesterday),
            ..percent("LAPSED", 50)
        };
        create(&db, expiring).await.unwrap();
        assert!(matches!(
            resolve(&db, "LAPSED", "nexus", Utc::now()).await,
            Err(CouponError::Expired(_))
        ));
    }

    #[tokio::test]
    async fn redemptions_are_capped_and_counted() {
        let db = pg().await;
        let capped = NewCoupon {
            max_redemptions: Some(1),
            ..percent("ONCE", 25)
        };
        let made = create(&db, capped).await.unwrap();
        // First use resolves, then is redeemed.
        resolve(&db, "ONCE", "nexus", Utc::now()).await.unwrap();
        mark_redeemed(&db, made.id).await.unwrap();
        // Now exhausted.
        assert!(matches!(
            resolve(&db, "ONCE", "nexus", Utc::now()).await,
            Err(CouponError::Exhausted(_))
        ));
        assert_eq!(
            by_code(&db, "ONCE").await.unwrap().unwrap().redeemed_count,
            1
        );
    }

    #[tokio::test]
    async fn an_unscoped_coupon_matches_any_product() {
        let db = pg().await;
        let anywhere = NewCoupon {
            product_code: None,
            ..percent("ANY50", 50)
        };
        create(&db, anywhere).await.unwrap();
        assert!(resolve(&db, "ANY50", "nautilus", Utc::now()).await.is_ok());
        assert!(resolve(&db, "ANY50", "nexus", Utc::now()).await.is_ok());
    }

    #[tokio::test]
    async fn list_all_returns_created_coupons() {
        let db = pg().await;
        create(&db, percent("A", 10)).await.unwrap();
        create(&db, percent("B", 20)).await.unwrap();
        let all = list_all(&db).await.unwrap();
        assert!(all.len() >= 2);
    }
}
