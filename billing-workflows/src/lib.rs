//! Worker-side billing workflows, hosted by the `workflows-service`
//! Restate worker (which binds [`canary::BillingCanaryService`] alongside
//! the `Notation` and `Archives` services — one endpoint,
//! no separate billing pod).
//!
//! Every workflow here reads or reports; none of them raises money. Lawyer
//! agree a matter's price with the client and raise the invoice in Xero
//! directly, so accounting originates there and Navigator only mirrors
//! what Xero already holds.
//!
//! - [`canary::BillingCanaryService`] — a nightly health check that proves the
//!   Xero integration is live end-to-end. It find-or-creates a single
//!   stable canary contact and asserts the resolve is idempotent. The
//!   `billing-canary-trigger` `CronJob` starts one invocation per day;
//!   Restate owns the retry schedule.
//!
//! - [`digest::BillingDigestService`] — a daily internal ops notice reporting
//!   trailing-window GCP cost across **every configured billing account and
//!   every project on each**, grouped by account, by project, and by service,
//!   each with a prior-window trend. `BILLING_EXPORT_TABLE` lists one export
//!   table per account. Reads them via `billing::gcp_cost` (shared with
//!   `archives`); the `billing-digest-trigger` `CronJob` starts one invocation
//!   per day.

pub mod canary;
pub mod digest;
pub mod reconcile;

pub use canary::{run_canary, BillingCanaryService, CanaryReport, RunRequest};
pub use digest::{
    build_digest_email, AccountCost, BillingDigestReport, BillingDigestService, DigestOutcome,
};
pub use reconcile::{reconcile_once, ReconcileInvoicesService, ReconcileReport, ReconcileRequest};
