use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    // Load `.env` for local development; in-cluster deploys inject
    // env vars via Kubernetes Secrets/ConfigMaps and this call is a
    // no-op. Must run before any `env::var` read (telemetry, config,
    // …) so a developer's `.env` overrides the empty process env.
    let _ = dotenvy::dotenv();
    // Fill in any values `navigator start-dev-server` wrote for the local KIND loop
    // (port-forward URLs, dev OAuth secrets). `from_path` does NOT
    // overwrite values already set in the process env, so `.env`
    // (user-edited) always wins over `.devx/env` (tool-generated).
    let _ = dotenvy::from_path(".devx/env");

    // One observability seam shared with every Neon Law Navigator binary: stdout logs
    // (JSON when an OTLP endpoint is set) plus OTLP traces + metrics. Held to
    // graceful shutdown below so batched spans/metrics flush before exit.
    let telemetry_guard = telemetry::init("navigator-web");

    let cfg = web::AppConfig::from_env().context("loading AppConfig")?;
    tracing::info!(?cfg.db, "configured database backend");

    // Fail loud if a production deploy lacks Restate, OPA, or GCS —
    // each silently degrades into a dev-only fallback that would lose
    // durability, allow-all every request, or persist client files
    // on a node-local filesystem.
    web::config::enforce_prod_invariants(|k| std::env::var(k).ok())
        .context("production environment invariants")?;

    let db = store::connect(&cfg.db)
        .await
        .context("connecting database")?;
    store::migrate(&db)
        .await
        .context("running database migrations")?;
    tracing::info!("migrations applied");

    // Object storage is created before the seed because template bodies
    // are now seeded into blob storage (not an inline column).
    let storage = cloud::from_env()
        .await
        .context("configuring object storage")?;
    tracing::info!("object storage configured");

    // The canonical seed writes template bodies as blobs to object storage.
    // In KIND the web pod can start before fake-gcs-server is reachable, so
    // wait for the store to answer a probe before seeding — otherwise the
    // first seed fails on a connection error and the pod crash-loops (with a
    // growing backoff) until the dependency is up. That crash-loop window is
    // the root of the KIND e2e flake. The `fs` backend answers instantly, so
    // this is a no-op for local/`fs` dev.
    cloud::wait_until_ready(&storage, Duration::from_mins(1))
        .await
        .context("waiting for object storage to become ready")?;
    tracing::info!("object storage ready");

    // Public-assets lane: blank government forms are pulled from here at
    // fill/download time and verified against their repo `.sha256` pins.
    // A distinct bucket in prod (`NAVIGATOR_ASSETS_BUCKET`); the same
    // root as `storage` for the fs backend and single-bucket KIND.
    let assets_storage = cloud::assets_from_env()
        .await
        .context("configuring public-assets object storage")?;
    tracing::info!("assets object storage configured");

    let seed_report = store::seed::seed_canonical(&db, &storage)
        .await
        .context("seeding canonical fixtures")?;
    tracing::info!(summary = %seed_report.summary(), "seed applied");

    let public_dir = std::env::var("NAVIGATOR_PUBLIC_DIR")
        .map_or_else(|_| PathBuf::from(web::DEFAULT_PUBLIC_DIR), PathBuf::from);
    tracing::info!(?public_dir, "serving static assets");

    let workshops_dir = std::env::var("NAVIGATOR_WORKSHOPS_DIR")
        .map_or_else(|_| PathBuf::from(web::DEFAULT_WORKSHOPS_DIR), PathBuf::from);
    let workshop_materials = web::workshops::loader::load_navigator(&workshops_dir)
        .context("loading workshop content")?;
    tracing::info!(
        count = workshop_materials.len(),
        ?workshops_dir,
        "loaded workshop materials"
    );
    let workshops = web::WorkshopIndex::new(workshop_materials);

    let marketing_dir = std::env::var("NAVIGATOR_MARKETING_DIR")
        .map_or_else(|_| PathBuf::from(web::DEFAULT_MARKETING_DIR), PathBuf::from);
    let marketing_docs =
        web::marketing::loader::load_dir(&marketing_dir).context("loading marketing content")?;
    tracing::info!(
        count = marketing_docs.len(),
        ?marketing_dir,
        "loaded marketing fragments"
    );
    // Spanish (`es`) marketing twins live in a parallel subtree; a slug
    // missing there falls back to English at lookup time. The mission
    // letter is transcreated (not literally translated) — its
    // `es/mission.md` sits alongside the other Spanish fragments.
    let marketing_es = web::marketing::loader::load_dir(&marketing_dir.join("es"))
        .context("loading Spanish marketing content")?;
    tracing::info!(
        count = marketing_es.len(),
        "loaded Spanish marketing fragments"
    );
    let marketing = web::MarketingIndex::new(marketing_docs).with_es(marketing_es);

    let blog_dir = std::env::var("NAVIGATOR_BLOG_DIR")
        .map_or_else(|_| PathBuf::from(web::DEFAULT_BLOG_DIR), PathBuf::from);
    let blog = web::blog::load_dir(&blog_dir).context("loading blog posts")?;
    tracing::info!(count = blog.posts().len(), ?blog_dir, "loaded blog posts");

    let events_dir = std::env::var("NAVIGATOR_EVENTS_DIR")
        .map_or_else(|_| PathBuf::from(web::DEFAULT_EVENTS_DIR), PathBuf::from);
    let events = web::events::load_dir(&events_dir).context("loading events")?;
    tracing::info!(count = events.events().len(), ?events_dir, "loaded events");

    // Reconcile the markdown events into the `events` table: the files are the
    // source of truth, so new files are inserted, existing rows are updated in
    // place (their registrations preserved), and rows whose file is gone are
    // hard-deleted (data minimization — we keep no event we no longer publish).
    let event_sync_inputs: Vec<store::events::EventSyncInput> = events
        .events()
        .iter()
        .map(|e| store::events::EventSyncInput {
            slug: e.slug.clone(),
            public_slug: e.public_slug.clone(),
            event_type: store::events::EventType::ShowAndTell,
            starts_at: e.starts_at,
            ends_at: e.ends_at,
            timezone: e.timezone.clone(),
            draft: e.draft,
        })
        .collect();
    let event_sync = store::events::sync_from_markdown(&db, &event_sync_inputs)
        .await
        .context("syncing events to database")?;
    tracing::info!(
        created = event_sync.created,
        updated = event_sync.updated,
        deleted = event_sync.deleted,
        "synced events to database"
    );

    let foundation_dir = std::env::var("NAVIGATOR_FOUNDATION_DIR").map_or_else(
        |_| PathBuf::from(web::DEFAULT_FOUNDATION_DIR),
        PathBuf::from,
    );
    let transparency =
        web::transparency::load_dir(&foundation_dir).context("loading foundation documents")?;
    tracing::info!(
        governance = transparency.governance().len(),
        minutes = transparency.minutes().len(),
        ?foundation_dir,
        "loaded foundation transparency documents"
    );

    let auth = web::AuthConfig::from_env().await;
    tracing::info!(enforced = auth.is_enforced(), "auth configured");

    let google_oauth = web::google_oauth::GoogleOauthConfig::from_env();
    tracing::info!(
        enforced = google_oauth.is_enforced(),
        "google_oauth configured"
    );

    let canonical_host = web::CanonicalHost::from_env();
    tracing::info!(
        enforced = canonical_host.is_enforced(),
        "canonical host configured"
    );

    let portal_only = web::PortalOnly::from_env();
    tracing::info!(
        enabled = portal_only.enabled(),
        "portal-only mode configured"
    );

    let sessions = web::SessionStore::from_env()
        .unwrap_or_else(|| web::SessionStore::new(web::session::random_token_32()));
    let oauth = web::OAuthConfig::from_env()
        .await
        .context("loading OAuth config")?;
    tracing::info!(
        enabled = oauth.is_some(),
        "oauth (Authorization Code + PKCE) configured"
    );

    let policy = web::policy::PolicyClient::from_env();
    tracing::info!("policy client configured");

    // Real DocuSign provider when the env is configured; otherwise the
    // stub (KIND / local dev). The inbound completion webhook
    // (`web::esignature_webhook`) closes the loop in both cases — the
    // stub's synthetic ids still persist + correlate.
    let signature_provider: std::sync::Arc<dyn web::signature::SignatureProvider> =
        web::signature::DocuSignSignatureProvider::from_env().map_or_else(
            || {
                tracing::info!("signature provider: StubSignatureProvider (DOCUSIGN_* unset)");
                std::sync::Arc::new(web::signature::StubSignatureProvider::new())
                    as std::sync::Arc<dyn web::signature::SignatureProvider>
            },
            |ds| {
                tracing::info!("signature provider: DocuSignSignatureProvider");
                std::sync::Arc::new(ds)
            },
        );

    // Real Xero provider when the env is configured; otherwise the stub
    // (KIND / local dev), so a fork boots and self-tests without a Xero
    // custom connection. Mirrors the signature-provider wiring above.
    let billing_provider: std::sync::Arc<dyn web::billing::BillingProvider> =
        web::billing::XeroBillingProvider::from_env().map_or_else(
            || {
                tracing::info!("billing provider: StubBillingProvider (XERO_* unset)");
                std::sync::Arc::new(web::billing::StubBillingProvider::new())
                    as std::sync::Arc<dyn web::billing::BillingProvider>
            },
            |xero| {
                tracing::info!("billing provider: XeroBillingProvider");
                std::sync::Arc::new(xero)
            },
        );

    let esignature_webhook_secret = std::env::var("DOCUSIGN_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    let esignature_hmac_key = std::env::var("DOCUSIGN_HMAC_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    tracing::info!(
        path_secret = esignature_webhook_secret.is_some(),
        hmac_key = esignature_hmac_key.is_some(),
        "e-signature webhook auth configured"
    );

    let email = web::email::from_env(db.clone()).context("loading email config")?;
    tracing::info!("email backend configured");

    // Runtime selection: if `RESTATE_BROKER_URL` is set in the
    // environment, the `web` binary talks to the in-cluster
    // `workflows-service` worker through Restate. Otherwise we fall
    // back to the in-process `InMemoryRuntime` *wrapped in
    // `DispatchingRuntime`* — without that wrap the local dev binary
    // never fires the welcome email (the `email_send__*` step has no
    // worker to consume it).
    //
    // Both implement `StateMachineRuntime`; the workflow and
    // questionnaire timelines share a single runtime instance keyed
    // by `(MachineKind, notation_id)`.
    let (workflow_runtime, questionnaire_runtime): (
        std::sync::Arc<dyn workflows::StateMachineRuntime>,
        std::sync::Arc<dyn workflows::StateMachineRuntime>,
    ) = if std::env::var("RESTATE_BROKER_URL").is_ok() {
        let rt = std::sync::Arc::new(workflows::RestateRuntime::from_env());
        tracing::info!("runtime: RestateRuntime (RESTATE_BROKER_URL is set)");
        (rt.clone(), rt)
    } else {
        let inner: std::sync::Arc<dyn workflows::StateMachineRuntime> =
            std::sync::Arc::new(workflows::InMemoryRuntime::new());
        let workflow = std::sync::Arc::new(workflows::DispatchingRuntime::new(
            inner.clone(),
            email.clone(),
            storage.clone(),
        ));
        tracing::info!(
            "runtime: DispatchingRuntime<InMemoryRuntime> (RESTATE_BROKER_URL unset; dispatches \
             email_send__* steps in-process through the EmailService)"
        );
        (workflow, inner)
    };

    let inbound_email_secret = std::env::var("SENDGRID_INBOUND_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    tracing::info!(
        configured = inbound_email_secret.is_some(),
        "inbound email webhook secret configured"
    );

    let email_events_secret = std::env::var("SENDGRID_EVENTS_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    tracing::info!(
        configured = email_events_secret.is_some(),
        "email events webhook secret configured"
    );

    let sendgrid_events_public_key = std::env::var("SENDGRID_EVENTS_PUBLIC_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    tracing::info!(
        configured = sendgrid_events_public_key.is_some(),
        "email events webhook signature verification configured"
    );

    let state = web::AppState {
        db: db.clone(),
        workshops,
        docs: web::docs::loader::bundled(),
        marketing,
        blog,
        transparency,
        events,
        auth,
        google_oauth,
        rate_limit: web::rate_limit::RateLimit::from_env(),
        canonical_host,
        portal_only,
        sessions,
        oauth,
        storage,
        assets_storage,
        forms_registry: std::sync::Arc::new(
            forms::registry().context("loading the vendored forms registry")?,
        ),
        policy,
        workflow_runtime,
        questionnaire_runtime,
        signature_provider,
        billing_provider,
        // Inbound-contract reviewer: Vertex Gemini when configured, else
        // the deterministic stub — selected here exactly like the A2A
        // router (chosen inside `build_router`). The
        // `analysis__contract_deviations` step is web-driven; the worker
        // has no LLM access.
        contract_reviewer: web::contract_review::GeminiContractReviewer::from_env().map_or_else(
            || -> std::sync::Arc<dyn web::contract_review::ContractReviewer> {
                std::sync::Arc::new(web::contract_review::StubContractReviewer)
            },
            |r| std::sync::Arc::new(r),
        ),
        esignature_webhook_secret,
        esignature_hmac_key,
        email,
        inbound_email_secret,
        email_events_secret,
        sendgrid_events_public_key,
        bootstrap_admin_email: web::oauth::bootstrap_admin_email_from_env(),
        // Opt-in email/password sign-in via GCP Identity Platform; `None`
        // unless `NAVIGATOR_IDENTITY_PLATFORM_API_KEY` is set.
        identity_password: web::oauth::IdentityPasswordConfig::from_env(),
        // Opt-in admin door (password reset + email confirm); `None`
        // unless `NAVIGATOR_GCP_PROJECT_ID` is set.
        identity_admin: web::idp_admin::IdentityAdminConfig::from_env(),
        // Production picks the router from env inside `build_router`
        // (Gemini when configured, else Null); no override here.
        a2a_router: None,
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, "web listening");

    axum::serve(listener, web::build_router(state, &public_dir))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum serve")?;
    let _ = db.close().await;
    telemetry_guard.shutdown();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = term => {}
    }
}
