//! Recurring-subscription reads/writes for the recurring-billing workflow.
//!
//! A [`subscription`](crate::entity::subscription) is an active recurring
//! engagement: a billed party tied to a `recurring` product (Nexus,
//! Nautilus), invoiced one Xero invoice per billing period. This module
//! is the read/write seam the workflow uses; the load-bearing pair is
//! [`due_for_period`] (which subscriptions to bill this month) and
//! [`mark_invoiced`] (advance the durable idempotency ledger after a
//! successful invoice).

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder,
};
use uuid::Uuid;

use crate::entity::subscription;
use crate::Db;

/// Fields to open a recurring subscription. `started_at` is an RFC 3339
/// timestamp; a discount is at most one of `discount_percent` /
/// `discount_amount_cents` (both `None` bills at list). `status` is the
/// initial lifecycle state — [`STATUS_PENDING`](subscription::STATUS_PENDING)
/// for a retainer-gated engagement (activated when the retainer is signed),
/// [`STATUS_ACTIVE`](subscription::STATUS_ACTIVE) for one already billable.
#[derive(Clone, Debug)]
pub struct NewSubscription {
    pub person_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub product_code: String,
    pub contact_name: String,
    pub contact_email: String,
    pub status: String,
    pub started_at: String,
    pub discount_percent: Option<i32>,
    pub discount_amount_cents: Option<i64>,
}

/// Open a new subscription in `new.status`, never yet invoiced.
///
/// # Errors
///
/// Propagates any database error.
pub async fn create(db: &Db, new: NewSubscription) -> Result<subscription::Model, sea_orm::DbErr> {
    subscription::ActiveModel {
        person_id: ActiveValue::Set(new.person_id),
        entity_id: ActiveValue::Set(new.entity_id),
        project_id: ActiveValue::Set(new.project_id),
        product_code: ActiveValue::Set(new.product_code),
        contact_name: ActiveValue::Set(new.contact_name),
        contact_email: ActiveValue::Set(new.contact_email),
        status: ActiveValue::Set(new.status),
        started_at: ActiveValue::Set(new.started_at),
        last_invoiced_period: ActiveValue::Set(None),
        discount_percent: ActiveValue::Set(new.discount_percent),
        discount_amount_cents: ActiveValue::Set(new.discount_amount_cents),
        ..Default::default()
    }
    .insert(db)
    .await
}

/// Every subscription, newest first — backs the admin listing page.
///
/// # Errors
///
/// Propagates any database error.
pub async fn list_all(db: &Db) -> Result<Vec<subscription::Model>, sea_orm::DbErr> {
    subscription::Entity::find()
        .order_by_desc(subscription::Column::InsertedAt)
        .all(db)
        .await
}

/// Fetch one subscription by id. `None` when no such row exists.
///
/// # Errors
///
/// Propagates any database error.
pub async fn by_id(db: &Db, id: Uuid) -> Result<Option<subscription::Model>, sea_orm::DbErr> {
    subscription::Entity::find_by_id(id).one(db).await
}

/// Activate every `pending` subscription tied to `project_id`, returning
/// how many were activated. Called when that project's retainer is signed:
/// a recurring engagement only becomes billable once its engagement
/// agreement is executed. Idempotent — a second call after activation
/// finds no `pending` rows and is a no-op (returns 0). Rows already
/// `paused`/`cancelled` are left untouched (they are not `pending`).
///
/// # Errors
///
/// Propagates any database error.
pub async fn activate_pending_for_project(
    db: &Db,
    project_id: Uuid,
) -> Result<u64, sea_orm::DbErr> {
    let pending = subscription::Entity::find()
        .filter(subscription::Column::ProjectId.eq(project_id))
        .filter(subscription::Column::Status.eq(subscription::STATUS_PENDING))
        .all(db)
        .await?;
    let mut activated = 0;
    for row in pending {
        let mut active: subscription::ActiveModel = row.into();
        active.status = ActiveValue::Set(subscription::STATUS_ACTIVE.to_string());
        active.updated_at = ActiveValue::NotSet;
        active.update(db).await?;
        activated += 1;
    }
    Ok(activated)
}

/// Every `active` subscription due for `period` (`YYYY-MM`): its product
/// is in `recurring_codes` and its `last_invoiced_period` is behind
/// `period` — never billed (`NULL`) or billed for an earlier month
/// (lexicographic compare is correct for the fixed `YYYY-MM` shape).
/// Ordered by `id` for a deterministic run. An empty code set selects
/// nothing.
///
/// # Errors
///
/// Propagates any database error.
pub async fn due_for_period(
    db: &Db,
    recurring_codes: &[String],
    period: &str,
) -> Result<Vec<subscription::Model>, sea_orm::DbErr> {
    if recurring_codes.is_empty() {
        return Ok(Vec::new());
    }
    subscription::Entity::find()
        .filter(subscription::Column::Status.eq(subscription::STATUS_ACTIVE))
        .filter(subscription::Column::ProductCode.is_in(recurring_codes.to_vec()))
        .filter(
            Condition::any()
                .add(subscription::Column::LastInvoicedPeriod.is_null())
                .add(subscription::Column::LastInvoicedPeriod.lt(period)),
        )
        .order_by_asc(subscription::Column::Id)
        .all(db)
        .await
}

/// Advance the durable idempotency ledger: record `period` as the most
/// recent invoiced period. Called only **after** the Xero invoice returns
/// Ok, so a re-run in the same month never re-selects this subscription —
/// the real defense against double-billing. A missing id is a no-op.
///
/// # Errors
///
/// Propagates any database error.
pub async fn mark_invoiced(db: &Db, id: Uuid, period: &str) -> Result<(), sea_orm::DbErr> {
    let Some(existing) = subscription::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut active: subscription::ActiveModel = existing.into();
    active.last_invoiced_period = ActiveValue::Set(Some(period.to_string()));
    // Let the behavior macro bump `updated_at`.
    active.updated_at = ActiveValue::NotSet;
    active.update(db).await?;
    Ok(())
}

/// Set a subscription's `status` (`active` | `paused` | `cancelled`). The
/// admin pause/cancel control; the workflow only ever bills `active`
/// rows. A missing id is a no-op.
///
/// # Errors
///
/// Propagates any database error.
pub async fn set_status(db: &Db, id: Uuid, status: &str) -> Result<(), sea_orm::DbErr> {
    let Some(existing) = subscription::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut active: subscription::ActiveModel = existing.into();
    active.status = ActiveValue::Set(status.to_string());
    active.updated_at = ActiveValue::NotSet;
    active.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        activate_pending_for_project, create, due_for_period, mark_invoiced, set_status,
        NewSubscription,
    };
    use crate::entity::subscription;
    use crate::test_support::pg;
    use uuid::Uuid;

    fn sub(product_code: &str, email: &str) -> NewSubscription {
        NewSubscription {
            person_id: None,
            entity_id: None,
            project_id: None,
            product_code: product_code.to_string(),
            contact_name: "Capricorn".into(),
            contact_email: email.into(),
            status: subscription::STATUS_ACTIVE.into(),
            started_at: "2026-06-01T00:00:00Z".into(),
            discount_percent: None,
            discount_amount_cents: None,
        }
    }

    #[tokio::test]
    async fn due_selects_active_unbilled_then_advances_past_the_period() {
        let db = pg().await;
        let s = create(&db, sub("nautilus", "a@example.com")).await.unwrap();

        let codes = vec!["nautilus".to_string()];
        // Never billed → due for the current period.
        let due = due_for_period(&db, &codes, "2026-06").await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, s.id);

        // After billing 2026-06, it is no longer due that month…
        mark_invoiced(&db, s.id, "2026-06").await.unwrap();
        assert!(due_for_period(&db, &codes, "2026-06")
            .await
            .unwrap()
            .is_empty());
        // …but is due again the next month.
        let next = due_for_period(&db, &codes, "2026-07").await.unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].id, s.id);
    }

    #[tokio::test]
    async fn paused_and_cancelled_subscriptions_are_never_due() {
        let db = pg().await;
        let paused = create(&db, sub("nexus", "p@example.com")).await.unwrap();
        let cancelled = create(&db, sub("nexus", "c@example.com")).await.unwrap();
        set_status(&db, paused.id, subscription::STATUS_PAUSED)
            .await
            .unwrap();
        set_status(&db, cancelled.id, subscription::STATUS_CANCELLED)
            .await
            .unwrap();

        let due = due_for_period(&db, &["nexus".to_string()], "2026-06")
            .await
            .unwrap();
        assert!(due.is_empty(), "non-active subscriptions are skipped");
    }

    #[tokio::test]
    async fn pending_subscriptions_are_never_due_until_activated() {
        let db = pg().await;
        let project_id = Uuid::now_v7();
        // A retainer-gated subscription: created pending, tied to a project.
        let mut new = sub("nexus", "ami@alps.example");
        new.status = subscription::STATUS_PENDING.into();
        new.project_id = Some(project_id);
        let s = create(&db, new).await.unwrap();
        assert_eq!(s.status, subscription::STATUS_PENDING);

        let codes = vec!["nexus".to_string()];
        // Pending → never billed, even though it is otherwise due.
        assert!(due_for_period(&db, &codes, "2026-06")
            .await
            .unwrap()
            .is_empty());

        // The retainer is signed → activate the project's pending subs.
        let activated = activate_pending_for_project(&db, project_id).await.unwrap();
        assert_eq!(activated, 1);

        // Now it is active and due for the period.
        let due = due_for_period(&db, &codes, "2026-06").await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, s.id);

        // Activation is idempotent: a second signal finds nothing pending.
        assert_eq!(
            activate_pending_for_project(&db, project_id).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn activation_only_touches_the_matching_project() {
        let db = pg().await;
        let signed = Uuid::now_v7();
        let other = Uuid::now_v7();
        let mut a = sub("nexus", "a@example.com");
        a.status = subscription::STATUS_PENDING.into();
        a.project_id = Some(signed);
        create(&db, a).await.unwrap();
        let mut b = sub("nexus", "b@example.com");
        b.status = subscription::STATUS_PENDING.into();
        b.project_id = Some(other);
        let other_sub = create(&db, b).await.unwrap();

        assert_eq!(activate_pending_for_project(&db, signed).await.unwrap(), 1);
        // The other project's subscription is untouched.
        let still_pending = super::by_id(&db, other_sub.id).await.unwrap().unwrap();
        assert_eq!(still_pending.status, subscription::STATUS_PENDING);
    }

    #[tokio::test]
    async fn an_empty_code_set_selects_nothing() {
        let db = pg().await;
        create(&db, sub("nautilus", "x@example.com")).await.unwrap();
        assert!(due_for_period(&db, &[], "2026-06")
            .await
            .unwrap()
            .is_empty());
    }
}
