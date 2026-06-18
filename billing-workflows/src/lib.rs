//! Worker-side billing workflows, hosted by the `workflows-service`
//! Restate worker (which binds [`canary::BillingCanaryService`] alongside
//! the `Notation` and `Archives` services — one endpoint,
//! no separate billing pod).
//!
//! Today this crate hosts one workflow:
//!
//! - [`canary::BillingCanary`] — a nightly health check that proves the
//!   Xero integration is live end-to-end. It find-or-creates a single
//!   stable canary contact and asserts the resolve is idempotent. The
//!   `billing-canary-trigger` `CronJob` starts one invocation per day;
//!   Restate owns the retry schedule.
//!
//! - [`matter_close::MatterCloseInvoice`] — durably raises + persists a
//!   matter's flat close fee when the firm signs the closing letter,
//!   reusing the same `billing` provider seam. `web` fires it from the
//!   firm-signature step; the worker persists the `xero_invoices` mirror.
//!
//! - [`digest::BillingDigest`] — a daily internal email reporting
//!   trailing-window GCP cost by service (gross / credits / net), the
//!   free-trial credit burned to date, and the honest "real cost when the
//!   trial credit expires" figure. Reads the billing export via
//!   `billing::gcp_cost` (shared with `archives`); the
//!   `billing-digest-trigger` `CronJob` starts one invocation per day.

pub mod canary;
pub mod digest;
pub mod matter_close;
pub mod reconcile;
pub mod recurring;

pub use canary::{run_canary, BillingCanary, BillingCanaryService, CanaryReport, RunRequest};
pub use digest::{
    build_digest_email, BillingDigest, BillingDigestReport, BillingDigestService, DigestOutcome,
};
pub use matter_close::{raise_invoice, MatterCloseInvoice, MatterCloseInvoiceService, RaiseResult};
pub use reconcile::{
    reconcile_once, ReconcileInvoices, ReconcileInvoicesService, ReconcileReport, ReconcileRequest,
};
pub use recurring::{
    run_recurring_billing, RecurringBilling, RecurringBillingService, RecurringReport,
};
// The request payload is the shared data contract in `billing` (so `web`
// can fire the workflow without depending on `restate-sdk`).
pub use billing::MatterCloseInvoiceRequest;
