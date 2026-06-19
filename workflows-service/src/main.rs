//! Worker entry point. Opens the shared Postgres connection,
//! builds the worker's `EmailService` (bare `SendGrid` in prod,
//! `CapturingEmail` otherwise), wires the `Notation` virtual-object
//! endpoint, and listens on the port the Restate broker discovers
//! via `restate-cli register`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use archives::workflow::{Archives, ArchivesService};
use billing_workflows::canary::{BillingCanary, BillingCanaryService};
use billing_workflows::digest::{BillingDigest, BillingDigestService};
use billing_workflows::matter_close::{MatterCloseInvoice, MatterCloseInvoiceService};
use billing_workflows::reconcile::{ReconcileInvoices, ReconcileInvoicesService};
use billing_workflows::recurring::{RecurringBilling, RecurringBillingService};
use restate_sdk::prelude::*;
use statutes::workflow::{Statutes, StatutesService};
use workflows::{EmailService, OpsEmailMirror};
use workflows_service::heartbeat::{Heartbeat, HeartbeatService};
use workflows_service::notation_service::Notation;
use workflows_service::{email_from_env, notifier_from_env, NotationService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    // `.devx/env` overlay for local KIND iteration (port-forward URLs,
    // dev OAuth secrets). Loaded second so `.env` always wins.
    let _ = dotenvy::from_path(".devx/env");
    // One observability seam for every binary: stdout logs (JSON when an
    // OTLP endpoint is set) plus OTLP traces + metrics. Held to end of main
    // so the drop flushes any batched export before the process exits.
    let _telemetry = telemetry::init("navigator-workflows-service");

    let cfg = store::DbConfig::from_env().context("read DbConfig from env")?;
    let db = store::connect(&cfg).await.context("connect to database")?;
    store::migrate(&db).await.context("apply migrations")?;

    let email = email_from_env().context("build email service from env")?;
    tracing::info!(
        backend = if std::env::var("NAVIGATOR_EMAIL_BACKEND").as_deref() == Ok("sendgrid") {
            "SendGrid"
        } else {
            "Capturing"
        },
        "workflows-service email backend"
    );

    // Internal ops notifications (Heartbeat, Archives, Statutes, BillingCanary,
    // BillingDigest) dual-send: the email backend above PLUS a Slack incoming
    // webhook to the engineering channel, so the recurring liveness signal also
    // lands where engineers watch. `ops_email` wraps the bare email backend in
    // a best-effort Slack mirror; a Slack outage never fails the durable email
    // step. Client-facing services (Notation, RecurringBilling invoices) keep
    // the plain `email` backend — mirroring client content into chat would
    // cross the firm's no-content trust boundary.
    let notifier = notifier_from_env();
    tracing::info!(
        backend = if workflows_service::notify_config::slack_enabled(|k| std::env::var(k).ok()) {
            "Slack"
        } else {
            "Capturing"
        },
        "workflows-service ops-notification backend"
    );
    let ops_email: Arc<dyn EmailService> = Arc::new(OpsEmailMirror::new(email.clone(), notifier));

    // Object storage for `document_open__*` step dispatch (the worker
    // renders the PDF and persists it here). Same `cloud::from_env`
    // backend selection as `web`: GCS in prod, FsStorage in dev.
    let storage = cloud::from_env()
        .await
        .context("configure object storage")?;

    let listen: SocketAddr = std::env::var("WORKFLOWS_SERVICE_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9080".into())
        .parse()
        .context("parse WORKFLOWS_SERVICE_LISTEN")?;

    tracing::info!(%listen, "workflows-service listening");

    // One endpoint hosts every workflow: the `Notation` virtual object and
    // the `Archives` nightly-export, `Statutes` weekly-scrape, `Heartbeat`
    // durable-execution liveness canary, `BillingCanary`, `BillingDigest`
    // (daily GCP cost email), and `MatterCloseInvoice` workflows. The
    // cron-driven ones have thin
    // `*-trigger` CronJobs; `MatterCloseInvoice` is fired by `web`'s
    // firm-signature step. All run against this one worker — there is no
    // per-workflow pod. The exact set of service names bound here is mirrored
    // in `workflows_service::registry`, which the registry tests guard against
    // drift (count + PascalCase naming).
    HttpServer::new(
        Endpoint::builder()
            // Client-facing email → plain backend (no Slack mirror).
            .bind(NotationService::new(db.clone(), email.clone(), storage).serve())
            // Internal ops email → dual-sent to Slack via `ops_email`.
            .bind(ArchivesService::new(ops_email.clone()).serve())
            .bind(StatutesService::new(ops_email.clone()).serve())
            .bind(HeartbeatService::new(ops_email.clone()).serve())
            .bind(BillingCanaryService::new(ops_email.clone()).serve())
            .bind(BillingDigestService::new(ops_email).serve())
            .bind(MatterCloseInvoiceService::new(db.clone()).serve())
            // Client-facing invoices → plain backend (no Slack mirror).
            .bind(RecurringBillingService::new(db.clone(), email).serve())
            .bind(ReconcileInvoicesService::new(db).serve())
            .build(),
    )
    .listen_and_serve(listen)
    .await;

    Ok(())
}
