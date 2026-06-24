//! Xero invoice mirror helpers.
//!
//! The matter-close fee is raised in Xero (idempotent on `project_id`
//! via the provider's `Idempotency-Key` header) and mirrored into the
//! [`xero_invoices`](crate::entity::xero_invoice) table here, keyed by
//! `project_id`. The portal reads this mirror to show per-project paid
//! invoices; it never calls Xero live. Two writers touch a row:
//!
//! - [`upsert`] — on raise, captures the Xero `InvoiceID` + total. Keyed
//!   on `project_id`, so a replay or double-close updates the one row
//!   rather than inserting a second (preserving any reconciled
//!   `amount_paid_cents`).
//! - [`record_reconcile`] — the nightly reconcile workflow folds Xero's
//!   `Status` + `AmountPaid` back onto the mirror.

use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entity::xero_invoice;
use crate::Db;

/// The fields captured when a matter-close invoice is raised. `currency`
/// defaults to `USD` at the call site; amounts are minor units (cents).
#[derive(Clone, Debug)]
pub struct UpsertXeroInvoice {
    pub project_id: Uuid,
    pub xero_invoice_id: String,
    pub reference: String,
    /// Xero invoice status at create time (`AUTHORISED`).
    pub status: String,
    pub amount_cents: i64,
    pub currency: String,
}

/// Idempotently mirror a raised Xero invoice, keyed on `project_id`.
///
/// Inserts a fresh row, or — when one already exists for the matter —
/// updates the Xero id / reference / status / total in place while
/// **preserving** the reconciled `amount_paid_cents` (the reconcile
/// workflow owns that field). This makes a workflow replay or a
/// double-close a no-op beyond a timestamp bump.
///
/// # Errors
///
/// Propagates any database error.
pub async fn upsert(
    db: &Db,
    input: &UpsertXeroInvoice,
) -> Result<xero_invoice::Model, sea_orm::DbErr> {
    if let Some(existing) = xero_invoice::Entity::find()
        .filter(xero_invoice::Column::ProjectId.eq(input.project_id))
        .one(db)
        .await?
    {
        let mut active: xero_invoice::ActiveModel = existing.into();
        active.xero_invoice_id = ActiveValue::Set(input.xero_invoice_id.clone());
        active.reference = ActiveValue::Set(input.reference.clone());
        active.status = ActiveValue::Set(input.status.clone());
        active.amount_cents = ActiveValue::Set(input.amount_cents);
        active.currency = ActiveValue::Set(input.currency.clone());
        active.update(db).await
    } else {
        xero_invoice::ActiveModel {
            project_id: ActiveValue::Set(input.project_id),
            xero_invoice_id: ActiveValue::Set(input.xero_invoice_id.clone()),
            reference: ActiveValue::Set(input.reference.clone()),
            status: ActiveValue::Set(input.status.clone()),
            amount_cents: ActiveValue::Set(input.amount_cents),
            amount_paid_cents: ActiveValue::Set(0),
            currency: ActiveValue::Set(input.currency.clone()),
            ..Default::default()
        }
        .insert(db)
        .await
    }
}

/// Fold a reconcile result (Xero `Status` + `AmountPaid`) onto the
/// mirror row for a matter. No-op (returns `None`) when no mirror row
/// exists yet for the project.
///
/// # Errors
///
/// Propagates any database error.
pub async fn record_reconcile(
    db: &Db,
    project_id: Uuid,
    status: &str,
    amount_paid_cents: i64,
) -> Result<Option<xero_invoice::Model>, sea_orm::DbErr> {
    let Some(existing) = xero_invoice::Entity::find()
        .filter(xero_invoice::Column::ProjectId.eq(project_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let mut active: xero_invoice::ActiveModel = existing.into();
    active.status = ActiveValue::Set(status.to_string());
    active.amount_paid_cents = ActiveValue::Set(amount_paid_cents);
    Ok(Some(active.update(db).await?))
}

/// Fetch the mirror rows for a set of matters, for the project-scoped
/// portal invoice list. Empty input short-circuits to an empty vec.
///
/// # Errors
///
/// Propagates any database error.
pub async fn for_projects(
    db: &Db,
    project_ids: &[Uuid],
) -> Result<Vec<xero_invoice::Model>, sea_orm::DbErr> {
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    xero_invoice::Entity::find()
        .filter(xero_invoice::Column::ProjectId.is_in(project_ids.to_vec()))
        .all(db)
        .await
}

/// The mirror rows that the nightly reconcile should re-check: anything
/// not already in a terminal state (`PAID` / `VOIDED`). A settled invoice
/// is never polled again.
///
/// # Errors
///
/// Propagates any database error.
pub async fn needing_reconcile(db: &Db) -> Result<Vec<xero_invoice::Model>, sea_orm::DbErr> {
    xero_invoice::Entity::find()
        .filter(xero_invoice::Column::Status.is_not_in(["PAID", "VOIDED"]))
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::{for_projects, needing_reconcile, record_reconcile, upsert, UpsertXeroInvoice};
    use crate::entity::{project, xero_invoice};
    use sea_orm::{ActiveValue, EntityTrait};

    async fn seed_project(db: &crate::Db, name: &str) -> uuid::Uuid {
        use sea_orm::ActiveModelTrait;
        let __dri = crate::test_support::dri_person(db).await;
        project::ActiveModel {
            name: ActiveValue::Set(name.into()),
            status: ActiveValue::Set("closed".into()),
            entity_id: ActiveValue::Set(crate::test_support::seed_entity(db).await),
            staff_dri_person_id: ActiveValue::Set(Some(__dri)),
            client_dri_person_id: ActiveValue::Set(Some(__dri)),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    fn input(project_id: uuid::Uuid, xero_id: &str, cents: i64) -> UpsertXeroInvoice {
        UpsertXeroInvoice {
            project_id,
            xero_invoice_id: xero_id.into(),
            reference: format!("Matter {project_id}"),
            status: "AUTHORISED".into(),
            amount_cents: cents,
            currency: "USD".into(),
        }
    }

    #[tokio::test]
    async fn upsert_inserts_one_row() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db, "northstar").await;

        let row = upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();
        assert_eq!(row.project_id, project_id);
        assert_eq!(row.xero_invoice_id, "xero-1");
        assert_eq!(row.amount_cents, 333_300);
        assert_eq!(row.amount_paid_cents, 0);

        let all = xero_invoice::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_on_project_id() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db, "northstar").await;

        // Raise, then replay with the same idempotent Xero id.
        upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();
        upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();

        let all = xero_invoice::Entity::find().all(&db).await.unwrap();
        assert_eq!(all.len(), 1, "a replay must not write a second row");
    }

    #[tokio::test]
    async fn upsert_preserves_reconciled_amount_paid() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db, "northstar").await;

        upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();
        // Reconcile marks it paid in full.
        record_reconcile(&db, project_id, "PAID", 333_300)
            .await
            .unwrap();
        // A later replay of the raise must not clobber the payment.
        let row = upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();
        assert_eq!(row.amount_paid_cents, 333_300);
        assert_eq!(row.status, "AUTHORISED", "raise resets the create-status");
    }

    #[tokio::test]
    async fn record_reconcile_updates_status_and_paid() {
        let db = crate::test_support::pg().await;
        let project_id = seed_project(&db, "northstar").await;
        upsert(&db, &input(project_id, "xero-1", 333_300))
            .await
            .unwrap();

        let row = record_reconcile(&db, project_id, "PAID", 333_300)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "PAID");
        assert_eq!(row.amount_paid_cents, 333_300);
    }

    #[tokio::test]
    async fn record_reconcile_is_noop_without_a_row() {
        let db = crate::test_support::pg().await;
        let missing = uuid::Uuid::now_v7();
        let out = record_reconcile(&db, missing, "PAID", 100).await.unwrap();
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn needing_reconcile_excludes_settled_invoices() {
        let db = crate::test_support::pg().await;
        let open = seed_project(&db, "open").await;
        let paid = seed_project(&db, "paid").await;
        let void = seed_project(&db, "void").await;
        upsert(&db, &input(open, "x-open", 100)).await.unwrap();
        upsert(&db, &input(paid, "x-paid", 200)).await.unwrap();
        upsert(&db, &input(void, "x-void", 300)).await.unwrap();
        record_reconcile(&db, paid, "PAID", 200).await.unwrap();
        record_reconcile(&db, void, "VOIDED", 0).await.unwrap();

        let rows = needing_reconcile(&db).await.unwrap();
        assert_eq!(rows.len(), 1, "only the AUTHORISED invoice is re-checked");
        assert_eq!(rows[0].project_id, open);
    }

    #[tokio::test]
    async fn for_projects_filters_to_the_requested_matters() {
        let db = crate::test_support::pg().await;
        let a = seed_project(&db, "a").await;
        let b = seed_project(&db, "b").await;
        let c = seed_project(&db, "c").await;
        upsert(&db, &input(a, "xero-a", 100)).await.unwrap();
        upsert(&db, &input(b, "xero-b", 200)).await.unwrap();
        upsert(&db, &input(c, "xero-c", 300)).await.unwrap();

        let rows = for_projects(&db, &[a, c]).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.project_id == a || r.project_id == c));

        assert!(for_projects(&db, &[]).await.unwrap().is_empty());
    }
}
