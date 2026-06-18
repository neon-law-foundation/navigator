//! The `RecurringBilling` Restate workflow — bill every active recurring
//! subscription one Xero invoice per billing period.
//!
//! The matter-close seam raises an invoice when a *matter closes*;
//! recurring products (Nexus, Nautilus) are subscriptions billed on a
//! *schedule*. This workflow is the sibling of [`crate::matter_close`],
//! driven by a period rather than a close event. It reuses the same
//! `billing` provider seam — this is not a new Xero integration, only a
//! scheduler + a deterministic idempotency key fed into `create_invoice`.
//!
//! **Idempotency is two layers, both required:**
//!
//! 1. *Durable (ours):* [`store::subscriptions::mark_invoiced`] advances
//!    `last_invoiced_period` only after the Xero invoice returns Ok, so a
//!    subscription already billed for the period is simply not re-selected
//!    next run. This is the real defense — it holds across a re-run days
//!    later.
//! 2. *Boundary (Xero):* the `create_invoice` `matter_id` parameter is the
//!    Xero `Idempotency-Key`. We derive a stable UUIDv5 from
//!    `(subscription_id, period)` ([`idempotency_key`]) so a double-POST
//!    in the same period dedupes at Xero (its window is hours); a new
//!    period yields a new key and bills again.
//!
//! We generate each invoice in our own scheduler rather than handing the
//! recurrence to Xero's `/RepeatingInvoices`: the billing source of truth
//! stays in our DB, per-engagement discounts and pause/cancel are applied
//! from the portal without round-tripping Xero templates, and we reuse the
//! `create_invoice` seam + `StubBillingProvider` tests we already have.
//!
//! [`run_recurring_billing`] is the testable core — it bills against the
//! [`billing::StubBillingProvider`] with a real DB, no worker required.

use std::collections::HashMap;
use std::sync::Arc;

use billing::{
    BillingProvider, ContactRequest, InvoiceLine, InvoiceRequest, LineDiscount, XeroBillingProvider,
};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use store::entity::{product, subscription};
use store::Db;
use uuid::Uuid;
use workflows::{EmailService, OutboundEmail};

use crate::matter_close::RaiseResult;

/// Fixed workspace namespace for the recurring-billing idempotency key.
/// Stable forever — changing it would re-bill every subscription, so it is
/// a hard-coded constant, never env-driven.
const RECURRING_NAMESPACE: Uuid = Uuid::from_u128(0x4e65_6f4e_4c61_775f_5265_6375_7272_696e);

/// Default diagnostic-email recipient when `BILLING_RECURRING_NOTIFY_EMAIL`
/// is unset.
const DEFAULT_NOTIFY_EMAIL: &str = "nick@neonlaw.com";

/// The current billing period (`YYYY-MM`, UTC). Bills the full period — no
/// proration. Reads the real clock, so it is only ever called inside a
/// Restate `ctx.run` step (whose result is journaled).
#[must_use]
pub fn current_period() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

/// The stable Xero `Idempotency-Key` for one subscription's invoice in one
/// period: a UUIDv5 of `"{subscription_id}:{period}"` under the workspace
/// namespace. Same subscription + same month → same UUID (Xero dedupes a
/// double-POST); next month → a different UUID (bills again).
#[must_use]
pub fn idempotency_key(subscription_id: Uuid, period: &str) -> Uuid {
    Uuid::new_v5(
        &RECURRING_NAMESPACE,
        format!("{subscription_id}:{period}").as_bytes(),
    )
}

/// Map a subscription's two optional discount columns onto the billing
/// seam's `LineDiscount`. At most one is set; both `None` bills at list.
#[must_use]
pub fn discount_from(percent: Option<i32>, amount_cents: Option<i64>) -> Option<LineDiscount> {
    match (percent, amount_cents) {
        (Some(p), _) => Some(LineDiscount::Percent(u32::try_from(p).unwrap_or(0))),
        (None, Some(a)) => Some(LineDiscount::AmountCents(a)),
        (None, None) => None,
    }
}

/// Build the `ACCREC` invoice request for one subscription's period — the
/// single source of truth being the product row (price + account code).
/// Pure, so the request shape unit-tests without a provider or DB.
#[must_use]
pub fn build_request(
    product: &product::Model,
    sub: &subscription::Model,
    period: &str,
) -> InvoiceRequest {
    InvoiceRequest {
        contact_name: sub.contact_name.clone(),
        contact_email: sub.contact_email.clone(),
        // Human-facing, matching the `NL-NORTHSTAR-0001` style — the month
        // makes each period's invoice distinct on the Xero side.
        reference: format!("NL-{}-{}", product.code.to_uppercase(), period),
        line_items: vec![InvoiceLine {
            description: format!("{} — {}", product.display_name, period),
            quantity: 1,
            unit_amount_cents: product.list_price_cents,
            account_code: product.account_code.clone(),
            discount: discount_from(sub.discount_percent, sub.discount_amount_cents),
        }],
    }
}

/// Resolve the payer's Xero contact and raise the period's invoice, keyed
/// on the per-period idempotency UUID. Provider-agnostic; unit-tested
/// against the stub.
///
/// # Errors
///
/// Propagates any billing-provider error.
pub async fn bill_subscription(
    provider: &dyn BillingProvider,
    product: &product::Model,
    sub: &subscription::Model,
    period: &str,
) -> Result<RaiseResult, billing::BillingError> {
    let contact = provider
        .ensure_contact(&ContactRequest {
            name: sub.contact_name.clone(),
            email: sub.contact_email.clone(),
        })
        .await?;
    let request = build_request(product, sub, period);
    let invoice = provider
        .create_invoice(idempotency_key(sub.id, period), &request)
        .await?;
    Ok(RaiseResult {
        invoice_id: invoice.0,
        contact_id: contact.0,
    })
}

/// One subscription's result for the diagnostic email.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SubscriptionOutcome {
    pub subscription_id: String,
    pub product_code: String,
    pub contact_email: String,
    /// `billed` (invoice raised this run) or `error` (provider failed; the
    /// subscription stays due and is retried next run).
    pub status: String,
    pub invoice_id: Option<String>,
    pub error: Option<String>,
}

/// The result of one recurring-billing run, surfaced as the invocation
/// output and folded into the diagnostic email.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct RecurringReport {
    pub period: String,
    pub billed: usize,
    pub errored: usize,
    pub outcomes: Vec<SubscriptionOutcome>,
}

/// Bill every due `active` subscription for `period`. The testable core:
///
/// 1. load the `recurring` products (the workflow's only product list);
/// 2. select the due `active` subscriptions for those products;
/// 3. for each, raise the Xero invoice, then — on Ok — advance the durable
///    `last_invoiced_period` so a re-run never re-bills it.
///
/// A per-subscription provider error is recorded in the report and the
/// subscription stays due (retried next run); a DB error aborts the run
/// (Restate retries the whole step).
///
/// # Errors
///
/// A database error loading products / subscriptions or advancing the
/// ledger, surfaced as [`billing::BillingError::Provider`].
pub async fn run_recurring_billing(
    provider: &dyn BillingProvider,
    db: &Db,
    period: &str,
) -> Result<RecurringReport, billing::BillingError> {
    let to_provider = |e: store::DbErr| billing::BillingError::Provider(e.to_string());

    let products = store::products::recurring(db).await.map_err(to_provider)?;
    let codes: Vec<String> = products.iter().map(|p| p.code.clone()).collect();
    let by_code: HashMap<String, product::Model> =
        products.into_iter().map(|p| (p.code.clone(), p)).collect();

    let due = store::subscriptions::due_for_period(db, &codes, period)
        .await
        .map_err(to_provider)?;

    let mut report = RecurringReport {
        period: period.to_string(),
        ..RecurringReport::default()
    };
    for sub in due {
        let Some(product) = by_code.get(&sub.product_code) else {
            // A subscription naming a product that is no longer recurring:
            // skip silently (it wasn't in the selected code set anyway).
            continue;
        };
        match bill_subscription(provider, product, &sub, period).await {
            Ok(raise) => {
                // Durable idempotency: advance ONLY after the invoice is Ok.
                store::subscriptions::mark_invoiced(db, sub.id, period)
                    .await
                    .map_err(to_provider)?;
                report.billed += 1;
                report.outcomes.push(SubscriptionOutcome {
                    subscription_id: sub.id.to_string(),
                    product_code: sub.product_code,
                    contact_email: sub.contact_email,
                    status: "billed".into(),
                    invoice_id: Some(raise.invoice_id),
                    error: None,
                });
            }
            Err(e) => {
                report.errored += 1;
                report.outcomes.push(SubscriptionOutcome {
                    subscription_id: sub.id.to_string(),
                    product_code: sub.product_code,
                    contact_email: sub.contact_email,
                    status: "error".into(),
                    invoice_id: None,
                    error: Some(e.to_string()),
                });
            }
        }
    }
    Ok(report)
}

/// Request body for `RecurringBilling::run`. An optional period override
/// (`YYYY-MM`); absent → the current UTC month, resolved inside the
/// journaled billing step.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RunRequest {
    #[serde(default)]
    pub period: Option<String>,
}

#[restate_sdk::workflow]
#[name = "RecurringBilling"]
pub trait RecurringBilling {
    async fn run(req: Json<RunRequest>) -> Result<Json<RecurringReport>, HandlerError>;
}

/// Service registered with the Restate endpoint. Holds a `Db` clone (for
/// the billing step's reads/writes) and the worker-side [`EmailService`]
/// (for the diagnostic). The Xero provider is built from env inside the
/// step so no token sits idle. Same shape as the canary / matter-close
/// services.
#[derive(Clone)]
pub struct RecurringBillingService {
    db: Db,
    email: Arc<dyn EmailService>,
}

impl RecurringBillingService {
    #[must_use]
    pub fn new(db: Db, email: Arc<dyn EmailService>) -> Self {
        Self { db, email }
    }
}

impl RecurringBilling for RecurringBillingService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: Json<RunRequest>,
    ) -> Result<Json<RecurringReport>, HandlerError> {
        let period_override = req.into_inner().period;

        // Step 1 — bill: resolve the period, raise one invoice per due
        // subscription, advance the durable ledger. Missing Xero config is
        // terminal; a DB/provider error is retryable, and the per-period
        // guard makes a replay safe (already-billed rows are not
        // re-selected).
        let db = self.db.clone();
        let report: RecurringReport = ctx
            .run(move || {
                let db = db.clone();
                let period_override = period_override.clone();
                async move {
                    let provider = XeroBillingProvider::from_env().ok_or_else(|| {
                        TerminalError::new("Xero is not configured (XERO_* env unset)")
                    })?;
                    let period = period_override.unwrap_or_else(current_period);
                    Ok(Json(run_recurring_billing(&provider, &db, &period).await?))
                }
            })
            .name("bill")
            .await?
            .into_inner();

        // Step 2 — diagnostic email, journaled separately so a bill-step
        // retry never re-sends.
        let email = build_diagnostic(&report, &notify_recipient(|k| std::env::var(k).ok()));
        let svc = Arc::clone(&self.email);
        ctx.run(move || async move {
            svc.send(email)
                .await
                .map(|_| ())
                .map_err(HandlerError::from)
        })
        .name("email")
        .await?;

        Ok(Json(report))
    }
}

/// The diagnostic-email recipient: `BILLING_RECURRING_NOTIFY_EMAIL`, else
/// the default. Takes a `key -> value` lookup so it is unit-testable.
fn notify_recipient<F: Fn(&str) -> Option<String>>(get: F) -> String {
    get("BILLING_RECURRING_NOTIFY_EMAIL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NOTIFY_EMAIL.to_string())
}

/// Build the firm-branded diagnostic email summarising a run. Pure —
/// exposed for unit-testing the rendered subject/body.
#[must_use]
pub fn build_diagnostic(report: &RecurringReport, recipient: &str) -> OutboundEmail {
    let subject = format!(
        "Recurring billing {} — {} billed, {} errored",
        report.period, report.billed, report.errored
    );
    let mut body = format!(
        "Recurring subscription billing ran for {}.\n\n\
         Invoices raised: {}\n\
         Errors (still due, retried next run): {}\n\n",
        report.period, report.billed, report.errored,
    );
    for o in &report.outcomes {
        match o.status.as_str() {
            "billed" => body.push_str(&format!(
                "  billed   {} {} → invoice {}\n",
                o.product_code,
                o.contact_email,
                o.invoice_id.as_deref().unwrap_or("?"),
            )),
            _ => body.push_str(&format!(
                "  ERROR    {} {} → {}\n",
                o.product_code,
                o.contact_email,
                o.error.as_deref().unwrap_or("?"),
            )),
        }
    }
    let html = workflows::email::render_email_html(
        &body,
        &workflows::email::base_url_from_env(),
        workflows::email::EmailBrand::Firm,
    );
    OutboundEmail::new(recipient.to_string(), subject, body).with_html(html)
}

#[cfg(test)]
mod unit_tests {
    use super::{
        build_diagnostic, discount_from, idempotency_key, notify_recipient, RecurringReport,
        SubscriptionOutcome,
    };
    use billing::LineDiscount;
    use uuid::Uuid;

    #[test]
    fn idempotency_key_is_stable_per_period_and_changes_across_periods() {
        let id = Uuid::from_u128(7);
        // Same subscription + same period → same key (Xero dedupes).
        assert_eq!(
            idempotency_key(id, "2026-06"),
            idempotency_key(id, "2026-06")
        );
        // Next period → a different key (bills again).
        assert_ne!(
            idempotency_key(id, "2026-06"),
            idempotency_key(id, "2026-07")
        );
        // Different subscription, same period → different key.
        assert_ne!(
            idempotency_key(id, "2026-06"),
            idempotency_key(Uuid::from_u128(8), "2026-06")
        );
    }

    #[test]
    fn discount_from_maps_the_two_columns() {
        assert_eq!(discount_from(None, None), None);
        assert_eq!(
            discount_from(Some(20), None),
            Some(LineDiscount::Percent(20))
        );
        assert_eq!(
            discount_from(None, Some(500)),
            Some(LineDiscount::AmountCents(500))
        );
        // Percent wins when (illegally) both are set — defensive.
        assert_eq!(
            discount_from(Some(10), Some(500)),
            Some(LineDiscount::Percent(10))
        );
    }

    #[test]
    fn diagnostic_email_summarises_billed_and_errored() {
        let report = RecurringReport {
            period: "2026-06".into(),
            billed: 1,
            errored: 1,
            outcomes: vec![
                SubscriptionOutcome {
                    subscription_id: "s1".into(),
                    product_code: "nautilus".into(),
                    contact_email: "a@example.com".into(),
                    status: "billed".into(),
                    invoice_id: Some("inv-1".into()),
                    error: None,
                },
                SubscriptionOutcome {
                    subscription_id: "s2".into(),
                    product_code: "nexus".into(),
                    contact_email: "b@example.com".into(),
                    status: "error".into(),
                    invoice_id: None,
                    error: Some("xero 500".into()),
                },
            ],
        };
        let email = build_diagnostic(&report, "ops@example.com");
        assert_eq!(email.to, "ops@example.com");
        assert!(email.subject.contains("2026-06"));
        assert!(email.subject.contains("1 billed"));
        assert!(email.body.contains("inv-1"));
        assert!(email.body.contains("xero 500"));
    }

    #[test]
    fn notify_recipient_defaults_then_honors_env() {
        assert_eq!(notify_recipient(|_| None), "nick@neonlaw.com");
        assert_eq!(
            notify_recipient(
                |k| (k == "BILLING_RECURRING_NOTIFY_EMAIL").then(|| "ops@example.com".to_string())
            ),
            "ops@example.com"
        );
    }
}

#[cfg(test)]
mod db_tests {
    //! Drives [`super::run_recurring_billing`] against a real test
    //! database + the [`billing::StubBillingProvider`] — no worker, no
    //! live Xero. Proves the two-layer idempotency end to end.

    use super::run_recurring_billing;
    use billing::StubBillingProvider;
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue::Set;
    use store::entity::product;
    use store::subscriptions::{create, set_status, NewSubscription};

    async fn seed_recurring_product(db: &store::Db, code: &str, price_cents: i64) {
        product::ActiveModel {
            code: Set(code.to_string()),
            display_name: Set(format!("Neon Law {code}")),
            list_price_cents: Set(price_cents),
            currency: Set("USD".to_string()),
            cadence: Set("monthly".to_string()),
            billing_kind: Set(product::BILLING_KIND_RECURRING.to_string()),
            active: Set(true),
            xero_item_code: Set(Some(code.to_uppercase())),
            account_code: Set("200".to_string()),
            matter_close_template_code: Set(None),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("seed product");
    }

    fn new_sub(code: &str, email: &str) -> NewSubscription {
        NewSubscription {
            person_id: None,
            entity_id: None,
            project_id: None,
            product_code: code.to_string(),
            contact_name: "Capricorn".into(),
            contact_email: email.into(),
            status: store::entity::subscription::STATUS_ACTIVE.into(),
            started_at: "2026-06-01T00:00:00Z".into(),
            discount_percent: None,
            discount_amount_cents: None,
        }
    }

    #[tokio::test]
    async fn one_due_subscription_bills_once_per_period_and_again_next_period() {
        let db = store::test_support::pg().await;
        seed_recurring_product(&db, "nautilus", 4_400).await;
        create(&db, new_sub("nautilus", "a@example.com"))
            .await
            .unwrap();

        let stub = StubBillingProvider::new();

        // First run for 2026-06 raises exactly one invoice at the DB price.
        let r1 = run_recurring_billing(&stub, &db, "2026-06").await.unwrap();
        assert_eq!(r1.billed, 1);
        assert_eq!(r1.errored, 0);
        assert_eq!(stub.calls().len(), 1, "one invoice raised");
        assert_eq!(
            stub.calls()[0].request.line_items[0].unit_amount_cents,
            4_400,
            "billed at the product list price"
        );

        // A second run in the SAME period bills no one — the durable
        // `last_invoiced_period` guard drops the subscription.
        let r2 = run_recurring_billing(&stub, &db, "2026-06").await.unwrap();
        assert_eq!(r2.billed, 0);
        assert_eq!(
            stub.calls().len(),
            1,
            "no second invoice in the same period"
        );

        // Advancing the period bills it again — a new month, a new invoice.
        let r3 = run_recurring_billing(&stub, &db, "2026-07").await.unwrap();
        assert_eq!(r3.billed, 1);
        assert_eq!(stub.calls().len(), 2, "next period raises a second invoice");
    }

    #[tokio::test]
    async fn paused_and_cancelled_subscriptions_are_not_billed() {
        let db = store::test_support::pg().await;
        seed_recurring_product(&db, "nexus", 222_200).await;
        let paused = create(&db, new_sub("nexus", "p@example.com"))
            .await
            .unwrap();
        let cancelled = create(&db, new_sub("nexus", "c@example.com"))
            .await
            .unwrap();
        set_status(&db, paused.id, store::entity::subscription::STATUS_PAUSED)
            .await
            .unwrap();
        set_status(
            &db,
            cancelled.id,
            store::entity::subscription::STATUS_CANCELLED,
        )
        .await
        .unwrap();

        let stub = StubBillingProvider::new();
        let report = run_recurring_billing(&stub, &db, "2026-06").await.unwrap();
        assert_eq!(report.billed, 0);
        assert!(stub.calls().is_empty(), "no invoice for non-active subs");
    }

    #[tokio::test]
    async fn invoice_carries_period_reference_and_account_code_from_the_product() {
        let db = store::test_support::pg().await;
        seed_recurring_product(&db, "nautilus", 4_400).await;
        create(&db, new_sub("nautilus", "a@example.com"))
            .await
            .unwrap();

        let stub = StubBillingProvider::new();
        run_recurring_billing(&stub, &db, "2026-06").await.unwrap();
        let call = &stub.calls()[0];
        assert_eq!(call.request.reference, "NL-NAUTILUS-2026-06");
        let line = &call.request.line_items[0];
        assert!(line.description.contains("2026-06"));
        assert_eq!(
            line.account_code, "200",
            "account code from the product row"
        );
    }
}
