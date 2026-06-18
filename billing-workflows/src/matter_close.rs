//! The `MatterCloseInvoice` Restate workflow — durably raise + persist a
//! matter's flat close fee when the firm signs the closing letter.
//!
//! `web`'s firm-signature step resolves the fee, the payer, and the
//! project reference (it has the notation context) and fires this
//! workflow with a fully-resolved payload, keyed on `project_id`. Restate
//! admits one invocation per key, and both Xero calls below are
//! idempotent (contact on email, invoice on `project_id` via the
//! `Idempotency-Key` header), so a replay or a double-close never
//! double-bills.
//!
//! Why durable rather than the old inline best-effort: a Xero outage at
//! sign-time used to be logged and the fee silently dropped (the matter
//! still closed). Restate retries the `xero` step until it succeeds, then
//! the `persist` step mirrors the result locally for the portal.
//!
//! Two steps, journaled independently:
//!
//! 1. `ctx.run("xero")` — resolve the client's Xero contact and raise the
//!    `ACCREC` invoice (retryable as a unit; both calls are idempotent).
//! 2. `ctx.run("persist")` — upsert the `xero_invoices` mirror row and
//!    cache the person's `xero_contact_id`, so a Xero-step retry never
//!    re-persists stale data and a persist retry never re-bills.
//!
//! [`raise_invoice`] is provider-agnostic so it unit-tests against the
//! [`billing::StubBillingProvider`] without a worker or a database.

use billing::{
    BillingProvider, ContactRequest, InvoiceLine, InvoiceRequest, MatterCloseInvoiceRequest,
    XeroBillingProvider,
};
use restate_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use store::Db;

/// The Xero ids produced by the raise step, surfaced as the invocation
/// output and consumed by the persist step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RaiseResult {
    pub invoice_id: String,
    pub contact_id: String,
}

#[restate_sdk::workflow]
#[name = "MatterCloseInvoice"]
pub trait MatterCloseInvoice {
    async fn run(req: Json<MatterCloseInvoiceRequest>) -> Result<Json<RaiseResult>, HandlerError>;
}

/// Service registered with the Restate endpoint. Holds a `Db` clone for
/// the persist step (same connection the worker opened at boot); the Xero
/// provider is built from env inside the raise step so no token sits idle.
#[derive(Clone)]
pub struct MatterCloseInvoiceService {
    db: Db,
}

impl MatterCloseInvoiceService {
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl MatterCloseInvoice for MatterCloseInvoiceService {
    async fn run(
        &self,
        ctx: WorkflowContext<'_>,
        req: Json<MatterCloseInvoiceRequest>,
    ) -> Result<Json<RaiseResult>, HandlerError> {
        let payload = req.into_inner();

        // Step 1 — Xero: resolve contact + raise the invoice. Missing Xero
        // config is terminal; a provider/API error is retryable, so
        // Restate replays just this step without re-persisting below.
        let for_xero = payload.clone();
        let result: RaiseResult = ctx
            .run(move || async move {
                let provider = XeroBillingProvider::from_env().ok_or_else(|| {
                    TerminalError::new("Xero is not configured (XERO_* env unset)")
                })?;
                Ok(Json(raise_invoice(&provider, &for_xero).await?))
            })
            .name("xero")
            .await?
            .into_inner();

        // Step 2 — persist the mirror + cache the contact id, journaled
        // separately so a Xero-step retry never re-persists.
        let db = self.db.clone();
        let for_persist = payload.clone();
        let result_for_persist = result.clone();
        ctx.run(move || async move {
            persist(&db, &for_persist, &result_for_persist).await?;
            Ok(())
        })
        .name("persist")
        .await?;

        Ok(Json(result))
    }
}

/// Resolve the payer's Xero contact and raise the `ACCREC` invoice.
/// Provider-agnostic; unit-tested against the stub.
///
/// # Errors
///
/// Propagates any billing-provider error.
pub async fn raise_invoice(
    provider: &dyn BillingProvider,
    payload: &MatterCloseInvoiceRequest,
) -> Result<RaiseResult, billing::BillingError> {
    let contact = provider
        .ensure_contact(&ContactRequest {
            name: payload.contact_name.clone(),
            email: payload.contact_email.clone(),
        })
        .await?;
    let request = InvoiceRequest {
        contact_name: payload.contact_name.clone(),
        contact_email: payload.contact_email.clone(),
        reference: payload.reference.clone(),
        line_items: vec![InvoiceLine {
            description: payload.description.clone(),
            quantity: 1,
            unit_amount_cents: payload.amount_cents,
            account_code: payload.account_code.clone(),
            discount: payload.discount.clone(),
        }],
    };
    let invoice = provider
        .create_invoice(payload.project_id, &request)
        .await?;
    Ok(RaiseResult {
        invoice_id: invoice.0,
        contact_id: contact.0,
    })
}

/// Mirror the raised invoice locally and cache the payer's contact id.
/// Idempotent via [`store::xero_invoices::upsert`] (keyed on
/// `project_id`).
///
/// # Errors
///
/// Propagates any database error.
pub async fn persist(
    db: &Db,
    payload: &MatterCloseInvoiceRequest,
    result: &RaiseResult,
) -> Result<(), store::DbErr> {
    store::xero_invoices::upsert(
        db,
        &store::xero_invoices::UpsertXeroInvoice {
            project_id: payload.project_id,
            xero_invoice_id: result.invoice_id.clone(),
            reference: payload.reference.clone(),
            status: "AUTHORISED".into(),
            // Mirror the *net* (list − discount) so the portal's
            // paid-invoice view matches what the client is billed.
            amount_cents: payload.net_amount_cents(),
            currency: payload.currency.clone(),
        },
    )
    .await?;
    store::persons::set_xero_contact_id(db, payload.person_id, &result.contact_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{raise_invoice, MatterCloseInvoiceRequest};
    use billing::StubBillingProvider;
    use uuid::Uuid;

    fn payload(project_id: Uuid) -> MatterCloseInvoiceRequest {
        MatterCloseInvoiceRequest {
            project_id,
            person_id: Uuid::now_v7(),
            contact_name: "Capricorn".into(),
            contact_email: "capricorn@example.com".into(),
            reference: format!("Matter {project_id}"),
            description: "onboarding__estate flat fee".into(),
            amount_cents: 333_300,
            currency: "USD".into(),
            account_code: "200".into(),
            discount: None,
        }
    }

    #[tokio::test]
    async fn raise_invoice_resolves_contact_then_creates_invoice() {
        let stub = StubBillingProvider::new();
        let project_id = Uuid::now_v7();
        let result = raise_invoice(&stub, &payload(project_id)).await.unwrap();

        assert!(result.contact_id.starts_with("stub-contact-"));
        assert!(result.invoice_id.starts_with("stub-invoice-"));
        // One contact resolve, one invoice create.
        assert_eq!(stub.contact_calls().len(), 1);
        assert_eq!(stub.calls().len(), 1);
        // The invoice was raised against the matter's project id.
        assert_eq!(stub.calls()[0].matter_id, project_id);
    }
}
