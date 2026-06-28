#![allow(clippy::doc_markdown)]
//! Neon Law Navigator web server library.
//!
//! Exposes [`build_router`] so the binary and the integration tests
//! share the exact same router instance — there is no second
//! definition of the route table in tests.

pub mod a2a;
pub mod access;
pub mod admin;
pub mod admin_csv;
pub mod agent_router;
pub mod api;
pub mod archives;
pub mod auth;
pub mod billing_admin;
pub mod blog;
// The billing-provider seam moved to the `billing` crate so the
// worker-side `billing-workflows` can share it. Re-exported here so
// existing `web::billing` / `web::xero_auth` paths keep resolving.
pub use billing;
pub use billing::xero_auth;
pub mod admin_contract_reviews;
pub mod admin_playbooks;
pub mod canonical_host;
pub mod clauses;
pub mod cli_auth;
pub mod config;
pub mod content_loader;
pub mod contract_review;
pub mod contract_review_walk;
pub mod conversation;
pub mod csrf;
pub mod docs;
pub mod documents;
pub mod docusign_auth;
pub mod email;
pub mod email_confirm;
pub mod email_events;
pub mod email_threads;
pub mod esign_view;
pub mod esignature_webhook;
pub mod estate;
pub mod events;
pub mod expunge;
pub mod expunge_request_route;
pub mod expunge_route;
pub mod git_http;
pub mod git_lfs;
pub mod git_meta;
mod github_stars;
pub mod google_oauth;
pub mod gov_forms;
pub mod idp_admin;
pub mod inbound_email;
pub mod intake;
pub mod marketing;
pub mod matter_documents;
pub mod mcp_principal;
pub mod oauth;
pub mod openapi;
pub mod password_reset;
pub mod people_list_answer;
pub mod policy;
pub mod portal;
pub mod portal_only;
pub mod project_documents;
pub mod project_export;
pub mod rate_limit;
pub mod retainer_walk;
pub mod review;
pub mod session;
pub mod session_renew;
pub mod signature;
pub mod signature_render;
pub mod statutes;
pub mod template_api;
pub mod template_gallery;
mod template_paths;
/// Shared test scaffolding (the canonical `AppState` builder). Always
/// compiled so both the integration tests and the `features` crate can
/// use it; see the module docs.
pub mod test_support;
pub mod transcript_intake;
pub mod transparency;
pub mod webhook_auth;
pub mod welcome;
pub mod workshops;

pub use oauth::{AuthState as OAuthState, OAuthConfig};
pub use session::{SessionData, SessionStore};

pub use canonical_host::CanonicalHost;
pub use portal_only::PortalOnly;

pub use auth::{AuthClaims, AuthConfig};
pub use blog::{BlogIndex, BlogPost};
pub use config::{AppConfig, ConfigError};
pub use docs::{Doc, DocsIndex};
pub use events::{Event, EventIndex};
pub use marketing::{MarketingDoc, MarketingIndex, PricingCard};
pub use transparency::{DocCategory, TransparencyDoc, TransparencyIndex};
// The A2A confirmation gate looks the *approver* up in `persons`, so a
// test that drives the gate must inject the same `Principal` the auth
// middleware produces in prod. Re-export it so the BDD suite can build
// one without depending on `mcp` directly.
pub use mcp::Principal;
pub use workshops::{WorkshopIndex, WorkshopMaterial};

use std::path::Path;
use std::sync::Arc;

use axum::extract::{FromRef, Path as AxumPath, Query, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use maud::{html, Markup, DOCTYPE};
use store::Db;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use views::pages::workshops as workshop_views;

/// Header name used for the per-request correlation ID. Lowercase per
/// the HTTP/2 convention; `SetRequestIdLayer` adds the header if the
/// client did not send one, and `PropagateRequestIdLayer` mirrors it
/// onto the response.
const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// `Cache-Control` for `/public/` static assets. One hour is the
/// conservative default until we add content-hashed filenames; bump
/// to `immutable` once asset paths are fingerprinted.
const STATIC_CACHE_CONTROL: HeaderValue = HeaderValue::from_static("public, max-age=3600");

const SHOW_TELL_EVENTS_PER_PAGE: usize = 5;

/// `Strict-Transport-Security` value — two years with
/// `includeSubDomains` and `preload`, making the site eligible for
/// the HSTS preload list. Safe because every public entry point
/// terminates TLS at the GCP HTTPS LB before reaching the pod (see
/// `cloud/README.md`).
const HSTS_VALUE: HeaderValue =
    HeaderValue::from_static("max-age=63072000; includeSubDomains; preload");

/// `Content-Security-Policy` header value. All JS/CSS is vendored under
/// the same-origin `/public` mount (no CDN), so `script-src 'self'` is
/// achievable and there are no inline `<script>` tags to allow.
/// `style-src` keeps `'unsafe-inline'` because the maud templates use
/// inline `style` attributes; everything else is locked to same-origin.
/// `object-src` and `frame-ancestors 'none'` kill plugin and
/// clickjacking vectors (the latter matching `X-Frame-Options: DENY`),
/// and `form-action 'self'` stops a reflected form from posting
/// credentials cross-origin. Applied with `if_not_present`, so the
/// Swagger UI route keeps its own looser CSP.
///
/// The one cross-origin allowance is `img-src`: responsive photos
/// resolve through `views::assets::asset_url`, which in production
/// points at the photo CDN bucket (`NAVIGATOR_ASSET_BASE_URL`). When
/// that var names an absolute origin, [`asset_csp_img_origin`] adds it
/// to `img-src` so the browser doesn't block CDN-hosted photos —
/// without it, the photos 404-or-block to alt text. Scripts and styles
/// stay `'self'`: only photos leave the origin, never code (see
/// `views::components::code`, which keeps highlight.js on `/public`).
/// Derived from the env var, never a hard-coded host, so OSS forks point
/// at their own CDN.
fn csp_value() -> HeaderValue {
    let img_extra = asset_csp_img_origin()
        .map(|origin| format!(" {origin}"))
        .unwrap_or_default();
    let csp = format!(
        "default-src 'self'; base-uri 'self'; object-src 'none'; \
         frame-ancestors 'none'; img-src 'self' data:{img_extra}; \
         style-src 'self' 'unsafe-inline'; script-src 'self'; form-action 'self'"
    );
    HeaderValue::from_str(&csp).expect("CSP is built from a fixed template and an ASCII URL origin")
}

/// The `scheme://host[:port]` origin of `NAVIGATOR_ASSET_BASE_URL`, for
/// inclusion in the CSP `img-src` directive — or `None` when the base
/// is the same-origin `/public` default (or any relative path), in which
/// case `'self'` already covers it. A CSP host-source is an origin, not
/// a path, so the bucket sub-path (`…/<project>-assets`) is dropped.
fn asset_csp_img_origin() -> Option<String> {
    csp_img_origin_from(&std::env::var("NAVIGATOR_ASSET_BASE_URL").ok()?)
}

/// Pure core of [`asset_csp_img_origin`], split out so tests exercise
/// every base form without stomping the process-wide env var (which
/// would race the parallel test runner).
fn csp_img_origin_from(base: &str) -> Option<String> {
    let base = base.trim();
    let (scheme, rest) = base
        .strip_prefix("https://")
        .map(|r| ("https://", r))
        .or_else(|| base.strip_prefix("http://").map(|r| ("http://", r)))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}{authority}"))
}

/// Directory the `web` binary serves under `/public/` by default —
/// the crate-bundled `public/` folder. Set `NAVIGATOR_PUBLIC_DIR`
/// at runtime to override.
pub const DEFAULT_PUBLIC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/public");

/// Root for the bundled workshop materials. Override with
/// `NAVIGATOR_WORKSHOPS_DIR`.
pub const DEFAULT_WORKSHOPS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/content/workshops");

/// Root for the bundled marketing fragments (hero copy, service
/// summaries, foundation mission). Override with
/// `NAVIGATOR_MARKETING_DIR`.
pub const DEFAULT_MARKETING_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/content/marketing");

/// Root for the bundled blog posts served at `/blog`. Override with
/// `NAVIGATOR_BLOG_DIR`.
pub const DEFAULT_BLOG_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/content/blog");

/// Root for the bundled Nebula show-and-tell pages. Override with
/// `NAVIGATOR_EVENTS_DIR`.
pub const DEFAULT_EVENTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/content/events");

/// Root for the bundled Foundation transparency documents served under
/// `/foundation/transparency` (bylaws, conflict policy, quarterly board
/// minutes). Override with `NAVIGATOR_FOUNDATION_DIR`.
pub const DEFAULT_FOUNDATION_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/content/foundation");

/// Shared router state. `Clone`-cheap — every field is `Arc`-backed
/// or wraps one.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub workshops: WorkshopIndex,
    /// Workspace docs published at `/docs/{slug}`, baked from the
    /// `docs/` tree at compile time. See [`docs`].
    pub docs: DocsIndex,
    pub marketing: MarketingIndex,
    /// Firm blog posts served at `/blog`, loaded at boot from a
    /// directory of dated `.md` files. See [`blog`].
    pub blog: BlogIndex,
    /// Foundation transparency documents (bylaws, conflict policy, quarterly
    /// board minutes) served under `/foundation/transparency`, loaded at boot
    /// from `web/content/foundation/`. See [`transparency`].
    pub transparency: TransparencyIndex,
    /// Public Nebula show-and-tells, loaded from dated markdown files.
    /// See [`events`].
    pub events: EventIndex,
    pub auth: AuthConfig,
    /// Google OAuth access-token validator for `/mcp`. Pass-through
    /// when `GOOGLE_OAUTH_CLIENT_IDS` is unset (KIND / local dev).
    pub google_oauth: google_oauth::GoogleOauthConfig,
    /// Per-IP request limiter for the abuse-sensitive endpoints
    /// (`/auth/*`, `/mcp`, `/api/aida/rpc`). Disabled in tests/dev;
    /// `RateLimit::from_env` enables it in production.
    pub rate_limit: rate_limit::RateLimit,
    pub canonical_host: CanonicalHost,
    /// White-label "portal-only" mode. When enabled, the public
    /// marketing + Foundation surface is not mounted and `/` redirects to
    /// `/portal`. Disabled by default. Sourced from
    /// `NAVIGATOR_PORTAL_ONLY`. See [`portal_only`].
    pub portal_only: PortalOnly,
    pub sessions: SessionStore,
    pub oauth: Option<OAuthConfig>,
    /// Object storage backend (filesystem in dev, Google Cloud
    /// Storage in production via the `cloud` crate).
    pub storage: std::sync::Arc<dyn cloud::StorageService>,
    /// Policy decision client (OPA sidecar at `NAVIGATOR_OPA_URL`).
    pub policy: policy::PolicyClient,
    /// Durable runtime for both timelines (workflow + questionnaire).
    /// In-memory in dev/tests; the `RestateRuntime` adapter is wired
    /// in production. Callers pick the timeline by passing
    /// [`workflows::MachineKind`] explicitly.
    pub workflow_runtime: Arc<dyn workflows::StateMachineRuntime>,
    /// Same `Arc` as `workflow_runtime` (the two timelines share one
    /// runtime instance keyed by `(MachineKind, notation_id)`). Kept
    /// as a separate field for now so call sites that drive only the
    /// questionnaire don't pretend to own the workflow side.
    pub questionnaire_runtime: Arc<dyn workflows::StateMachineRuntime>,
    /// Pluggable signature provider. The stub is the default; a
    /// real provider (DocuSign, Dropbox Sign) drops in behind the
    /// same trait.
    pub signature_provider: Arc<dyn signature::SignatureProvider>,
    /// Inbound-contract deviation reviewer. Selected like
    /// [`build_router`]'s A2A router: [`contract_review::GeminiContractReviewer`]
    /// (Vertex) when `NAVIGATOR_GCP_PROJECT_ID` is set, else the
    /// deterministic [`contract_review::StubContractReviewer`] (KIND /
    /// tests). The `analysis__contract_deviations` step runs this web-side
    /// — the worker has no LLM access.
    pub contract_reviewer: Arc<dyn contract_review::ContractReviewer>,
    /// Pluggable billing provider. The stub is the default; the real
    /// `XeroBillingProvider` drops in behind the same trait when the
    /// `XERO_*` env is configured. The matter-close step raises the
    /// flat-fee invoice through this seam.
    pub billing_provider: Arc<dyn billing::BillingProvider>,
    /// Coarse path secret the e-signature provider must include in its
    /// completion-webhook URL (`/webhook/esignature/{secret}`). Same
    /// `None`-accepts-any-token dev posture as `inbound_email_secret`;
    /// loaded from `DOCUSIGN_WEBHOOK_SECRET`. Defense-in-depth — the
    /// real gate is `esignature_hmac_key`. See [`esignature_webhook`].
    pub esignature_webhook_secret: Option<String>,
    /// Shared HMAC-SHA256 key the e-signature webhook verifies over the
    /// raw request body before advancing workflow state. `None` in
    /// dev/tests skips verification; required in production via
    /// `enforce_prod_invariants`. Loaded from `DOCUSIGN_HMAC_KEY`.
    pub esignature_hmac_key: Option<String>,
    /// Outbound email backend. `CapturingEmail` in dev/tests so
    /// outbound mail never escapes the host; `SendGridEmail` (wrapped
    /// in `RetryingEmail`) in production. Selected by
    /// [`email::from_env`].
    pub email: Arc<dyn email::EmailService>,
    /// Shared secret SendGrid Inbound Parse must include in the
    /// webhook URL path. `None` in dev/tests (the route accepts any
    /// path token); required in production via
    /// `enforce_prod_invariants`. Loaded from `SENDGRID_INBOUND_SECRET`.
    pub inbound_email_secret: Option<String>,
    /// Shared secret SendGrid's Event Webhook must include in the
    /// delivery-event URL path (`/api/email-events/{secret}`). Same
    /// `None`-accepts-any-token dev posture as `inbound_email_secret`;
    /// loaded from `SENDGRID_EVENTS_SECRET`. See [`email_events`].
    pub email_events_secret: Option<String>,
    /// SendGrid's "Signed Event Webhook" verification key — a
    /// base64-encoded DER `SubjectPublicKeyInfo` for the ECDSA/P-256
    /// public key SendGrid issues. When set, the Event Webhook verifies
    /// each delivery-event POST's signature over `timestamp || body`
    /// (the real payload-level gate; the path secret is only coarse).
    /// `None` in dev/tests skips it; required in production via
    /// `enforce_prod_invariants`. Loaded from `SENDGRID_EVENTS_PUBLIC_KEY`.
    pub sendgrid_events_public_key: Option<String>,
    /// Email that is always granted the `admin` role on sign-in and
    /// JIT-created when missing. `None` disables the carve-out, so
    /// every sign-in then strictly requires a pre-seeded `persons`
    /// row. Sourced from `NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL` in production;
    /// tests set it explicitly so the suite can run in parallel
    /// without env-var stomping.
    pub bootstrap_admin_email: Option<String>,
    /// Opt-in email/password front door, delegated to GCP Identity
    /// Platform. `None` (the default) keeps `/auth/login` a pure OIDC
    /// redirect. Sourced from `NAVIGATOR_IDENTITY_PLATFORM_API_KEY` in
    /// production; tests inject a mock-endpoint config directly so the
    /// password path can be exercised without touching process env.
    pub identity_password: Option<oauth::IdentityPasswordConfig>,
    /// Opt-in admin door to GCP Identity Platform, backing the
    /// password-reset and email-confirm flows (they write a new password
    /// or flip `emailVerified` for a signed-out user). `None` unless
    /// `NAVIGATOR_GCP_PROJECT_ID` is set; tests inject a mock-endpoint
    /// config directly. See [`idp_admin::IdentityAdminConfig`].
    pub identity_admin: Option<idp_admin::IdentityAdminConfig>,
    /// Optional override for the A2A natural-language router. `None` in
    /// production and KIND — [`build_router`] then selects
    /// [`agent_router::GeminiRouter`] (when `NAVIGATOR_GCP_PROJECT_ID`
    /// is set) or [`agent_router::NullRouter`]. Tests inject a scripted
    /// [`agent_router::AgentRouter`] here to drive the agentic loop
    /// deterministically — exercising the loop, the real tools, and the
    /// real email side-effects — without a live LLM.
    pub a2a_router: Option<Arc<dyn agent_router::AgentRouter>>,
}

impl FromRef<AppState> for Db {
    fn from_ref(s: &AppState) -> Self {
        s.db.clone()
    }
}

impl FromRef<AppState> for WorkshopIndex {
    fn from_ref(s: &AppState) -> Self {
        s.workshops.clone()
    }
}

impl FromRef<AppState> for MarketingIndex {
    fn from_ref(s: &AppState) -> Self {
        s.marketing.clone()
    }
}

impl FromRef<AppState> for DocsIndex {
    fn from_ref(s: &AppState) -> Self {
        s.docs.clone()
    }
}

impl FromRef<AppState> for BlogIndex {
    fn from_ref(s: &AppState) -> Self {
        s.blog.clone()
    }
}

impl FromRef<AppState> for TransparencyIndex {
    fn from_ref(s: &AppState) -> Self {
        s.transparency.clone()
    }
}

impl FromRef<AppState> for EventIndex {
    fn from_ref(s: &AppState) -> Self {
        s.events.clone()
    }
}

impl FromRef<AppState> for SessionStore {
    fn from_ref(s: &AppState) -> Self {
        s.sessions.clone()
    }
}

impl FromRef<AppState> for CanonicalHost {
    fn from_ref(s: &AppState) -> Self {
        s.canonical_host.clone()
    }
}

impl FromRef<AppState> for Arc<dyn email::EmailService> {
    fn from_ref(s: &AppState) -> Self {
        s.email.clone()
    }
}

impl FromRef<AppState> for Arc<dyn cloud::StorageService> {
    fn from_ref(s: &AppState) -> Self {
        s.storage.clone()
    }
}

/// Axum extractor that produces an [`views::AuthState`] for any
/// handler that wants to render the auth-aware header. The session
/// cookie (if present and unexpired) yields `Authenticated`;
/// everything else — missing cookie, bad signature, expired payload —
/// yields `Anonymous`. Inserting this extractor on a public-page
/// handler is the only thing that lights up the "Admin / Sign in"
/// swap in the layout.
pub struct MaybeAuth(pub views::AuthState);

impl<S> axum::extract::FromRequestParts<S> for MaybeAuth
where
    S: Send + Sync,
    SessionStore: FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let sessions = SessionStore::from_ref(state);
        let authed =
            if let Ok(cookies) = tower_cookies::Cookies::from_request_parts(parts, state).await {
                cookies
                    .get(session::SESSION_COOKIE_NAME)
                    .and_then(|c| sessions.decode(c.value()))
                    .is_some_and(|s| !s.is_expired())
            } else {
                false
            };
        let auth = if authed {
            views::AuthState::Authenticated
        } else {
            views::AuthState::Anonymous
        };
        Ok(MaybeAuth(auth))
    }
}

/// Build the application's router with every public route mounted
/// against the given state, and static assets served from `public_dir`.
#[allow(clippy::too_many_lines)]
pub fn build_router(state: AppState, public_dir: &Path) -> Router {
    let static_files = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            STATIC_CACHE_CONTROL,
        ))
        .service(ServeDir::new(public_dir));
    // JSON API uses only `Db` as state; merge it under the same root.
    // Every `/api/*` route runs behind `require_policy` so OPA enforces
    // the OIDC requirement uniformly with `/portal/*`. The OPA rule in
    // `k8s/base/opa/opa.yaml` exempts `/openapi.json` so the published
    // schema stays discoverable without a session.
    let api = api::routes().with_state(state.db.clone()).route_layer(
        axum::middleware::from_fn_with_state(
            (state.sessions.clone(), state.policy.clone()),
            crate::policy::require_policy,
        ),
    );
    let admin_state = admin::AdminState {
        db: state.db.clone(),
        workflow_runtime: state.workflow_runtime.clone(),
        signature_provider: state.signature_provider.clone(),
        retainer_intake_questionnaire: workflows::retainer_intake_questionnaire(),
        questionnaire_runtime: state.questionnaire_runtime.clone(),
        storage: state.storage.clone(),
        email: state.email.clone(),
        billing_provider: state.billing_provider.clone(),
        contract_reviewer: state.contract_reviewer.clone(),
        bootstrap_admin_email: state.bootstrap_admin_email.clone(),
    };
    let admin = admin::routes(
        admin_state,
        state.auth.clone(),
        state.sessions.clone(),
        state.policy.clone(),
    );
    // Role-aware portal landing. `/portal/*` is the only URL space
    // for the back-office surface; the firm-wide CRUD lives under
    // `/portal/admin/*` and project routes live under
    // `/portal/projects/*` (see `admin::routes`).
    let portal = portal::routes(
        portal::PortalState {
            db: state.db.clone(),
        },
        state.auth.clone(),
        state.sessions.clone(),
        state.policy.clone(),
    );
    // MCP rides on the same Pod / host as the public site, served at
    // `POST /mcp`. The layer stack (outermost first):
    //
    //   1. google_oauth::require_google_oauth — prod: validates the
    //      Google OAuth access token Gemini Enterprise sends as
    //      Bearer via tokeninfo, populates AuthClaims. Pass-through
    //      when GOOGLE_OAUTH_CLIENT_IDS is unset (KIND / local dev).
    //      Replaces the earlier IAP layer; IAP couldn't parse the
    //      opaque ya29.* tokens Gemini Enterprise actually sends.
    //   2. require_auth — KIND: validates Bearer JWT. In prod the
    //      Google-OAuth layer already populated AuthClaims so this
    //      short-circuits.
    //   3. require_policy — OPA decision; same as /portal.
    //
    // CSRF is intentionally NOT in the chain — JSON-RPC clients send
    // a Bearer token, not a session cookie.
    let mut mcp_state = mcp::McpState::new(state.db.clone(), state.questionnaire_runtime.clone());
    // Object storage is always available to the MCP tools — the
    // questionnaire walker reads template bodies from blob storage, so a
    // non-bundled template's spec can still be parsed.
    mcp_state.storage = Some(state.storage.clone());
    let mcp = mcp::build_router(mcp_state.clone())
        .route_layer(axum::middleware::from_fn_with_state(
            state.google_oauth.clone(),
            crate::mcp_principal::inject_principal,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            (state.sessions.clone(), state.policy.clone()),
            crate::policy::require_policy,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.auth.clone(),
            crate::auth::require_auth,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.google_oauth.clone().with_db(state.db.clone()),
            crate::google_oauth::require_google_oauth,
        ))
        // Outermost: shed an over-budget IP with 429 before any auth or
        // tokeninfo work runs.
        .route_layer(axum::middleware::from_fn_with_state(
            state.rate_limit.clone(),
            crate::rate_limit::enforce,
        ));
    // A2A surface — public agent card at `/api/aida.json`, JSON-RPC
    // at `/api/aida/rpc` behind the same auth stack as `/mcp`. The
    // card MUST stay public: Gemini Enterprise (and any other A2A
    // client) fetches it anonymously during agent registration to
    // learn the transport and security schemes.
    //
    // The natural-language router maps free-form messages
    // (`message/send` without `metadata.skill`) onto a skill via
    // Vertex AI Gemini Flash. Pod's GSA needs `roles/aiplatform.user`
    // for Workload Identity to fetch a token. When
    // `NAVIGATOR_GCP_PROJECT_ID` is unset (KIND / local dev), falls
    // back to `NullRouter` which returns a helpful Task explaining
    // the `metadata.skill` backdoor.
    let router: Arc<dyn agent_router::AgentRouter> =
        if let Some(injected) = state.a2a_router.clone() {
            tracing::info!("a2a router: injected override (test harness)");
            injected
        } else if let Some(gemini) = agent_router::GeminiRouter::from_env() {
            tracing::info!("a2a router: Vertex AI Gemini Flash");
            Arc::new(gemini)
        } else {
            tracing::info!("a2a router: NullRouter (set NAVIGATOR_GCP_PROJECT_ID to enable)");
            Arc::new(agent_router::NullRouter)
        };
    let a2a_state = a2a::A2aState {
        mcp: mcp_state,
        canonical_host: state.canonical_host.clone(),
        router,
        pending: a2a::PendingConfirmations::new(),
    };
    let (a2a_card, a2a_rpc) = a2a::build_routers(
        a2a_state,
        state.google_oauth.clone(),
        state.auth.clone(),
        state.sessions.clone(),
        state.policy.clone(),
    );
    // Rate-limit the JSON-RPC endpoint (the agent card stays public +
    // unlimited so registration always succeeds).
    let a2a_rpc = a2a_rpc.layer(axum::middleware::from_fn_with_state(
        state.rate_limit.clone(),
        crate::rate_limit::enforce,
    ));
    // Loopback-OAuth endpoints for the `navigator` CLI. `/auth/cli/start`
    // mints a CLI bearer from the browser session; `/auth/cli/whoami`
    // echoes the bearer caller's identity. Both live under the
    // private-mode-exempt `/auth/*` prefix.
    let cli_auth = cli_auth::routes(state.sessions.clone());
    let host_layer = axum::middleware::from_fn_with_state(
        state.canonical_host.clone(),
        canonical_host::enforce_canonical_host,
    );
    // Browser-flow login routes only mount when OAUTH_* is configured;
    // otherwise the bearer-token path remains the only auth surface.
    let bootstrap_admin = state.bootstrap_admin_email.clone();
    let identity_password = state.identity_password.clone();
    let identity_admin = state.identity_admin.clone();
    let oauth_routes = state.oauth.as_ref().map(|oauth| {
        oauth::routes(oauth::AuthState {
            oauth: oauth.clone(),
            sessions: state.sessions.clone(),
            db: state.db.clone(),
            email: state.email.clone(),
            workflow_runtime: state.workflow_runtime.clone(),
            bootstrap_admin_email: bootstrap_admin.clone(),
            // Opt-in email/password front door via GCP Identity Platform;
            // `None` (the default) keeps `/auth/login` a pure OIDC redirect.
            // Threaded from `AppState` (not read from env here) so tests can
            // inject a mock endpoint without mutating process env.
            identity_password: identity_password.clone(),
            // Opt-in admin door for the password-reset / email-confirm
            // flows; threaded from `AppState` for the same reason.
            identity_admin: identity_admin.clone(),
            // `Secure` auth cookies whenever the deployment's external
            // scheme is HTTPS (prod), off for the `http://localhost` KIND
            // loop so cookies still round-trip in dev. The redirect URI
            // carries the external scheme even behind a TLS-terminating LB.
            secure_cookies: oauth.redirect_uri().starts_with("https://"),
        })
        // Throttle the credential endpoints (login, password submit,
        // callback) per IP — the brute-force / credential-stuffing target.
        .layer(axum::middleware::from_fn_with_state(
            state.rate_limit.clone(),
            crate::rate_limit::enforce,
        ))
    });

    // Captured before `state` is moved into `.with_state` below. Decides
    // whether the public marketing + Foundation surface mounts at all.
    let portal_only = state.portal_only;
    // Sliding session renewal state, captured before the move. `secure`
    // mirrors the auth router's cookie posture: `Secure` whenever the
    // external scheme is HTTPS (prod), off for the `http://localhost`
    // KIND loop. No OAuth configured ⇒ no browser sessions to renew.
    let session_renew = session_renew::RenewState {
        sessions: state.sessions.clone(),
        secure: state
            .oauth
            .as_ref()
            .is_some_and(|o| o.redirect_uri().starts_with("https://")),
    };

    // Always-on application surface — mounts in both modes: the legal
    // pages, the machine-readable corpus index, the Kubernetes probes,
    // the authenticated webhooks, and the DocuSign consent landing.
    let mut router = Router::new()
        .route("/privacy", get(privacy))
        .route("/terms", get(terms))
        .route("/llms.txt", get(llms_txt))
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
        .route("/github-stars", get(github_stars::handler))
        .route(
            "/webhook/sendgrid/inbound/{secret}",
            axum::routing::post(inbound_email::webhook),
        )
        .route(
            "/api/email-events/{secret}",
            axum::routing::post(email_events::webhook),
        )
        .route(
            "/webhook/esignature/{secret}",
            axum::routing::post(esignature_webhook::webhook),
        )
        .route("/docusign/consent-callback", get(docusign_consent_callback));

    if portal_only.enabled() {
        // White-label app-only deploy (NAVIGATOR_PORTAL_ONLY): the firm's
        // own marketing site owns the public surface, so mount none of the
        // marketing / Foundation pages and 303 the bare host to the portal.
        router = router.route(
            "/",
            get(|| async { axum::response::Redirect::to("/portal") }),
        );
    } else {
        router = router
            .route("/", get(home))
            .route("/blog", get(blog_index))
            .route("/blog/{slug}", get(blog_post))
            .route("/contact", get(contact))
            .route("/foundation", get(foundation_mission))
            .route(
                "/foundation/mission",
                get(|| async { axum::response::Redirect::permanent("/foundation") }),
            )
            .route(
                "/foundation/contact",
                get(|| async { axum::response::Redirect::permanent("/contact") }),
            )
            .route("/foundation/nimbus", get(foundation_nimbus))
            // The Neon Law Navigator hub and its per-package pages. `/navigator`
            // and `/lsp` were the old top-level URLs; keep them as
            // permanent redirects so existing links never dead-end.
            .route("/foundation/navigator", get(navigator))
            .route("/foundation/navigator/lsp", get(navigator_lsp))
            .route("/foundation/navigator/cli", get(navigator_cli))
            .route("/foundation/navigator/mcp", get(navigator_mcp))
            .route("/foundation/navigator/web", get(navigator_web))
            .route("/foundation/notations", get(notation_templates))
            .route(
                "/foundation/notation-templates",
                get(|| async { axum::response::Redirect::permanent("/foundation/notations") }),
            )
            .route("/foundation/transparency", get(foundation_transparency))
            .route(
                "/foundation/transparency/{slug}",
                get(foundation_transparency_doc),
            )
            .route(
                "/navigator",
                get(|| async { axum::response::Redirect::permanent("/foundation/navigator") }),
            )
            // The DB-backed catalog: every active product at its
            // `list_price_cents` — the price a prospect sees is the row
            // Xero invoices. Public on the open site. This single
            // `/services` page replaces the old Services dropdown; each
            // card links out to a `/services/<slug>` detail.
            .route("/services", get(service_index))
            .route("/services/nexus", get(service_nexus))
            .route("/services/northstar", get(service_northstar))
            .route("/services/nest", get(service_nest))
            .route("/services/nautilus", get(service_nautilus))
            .route("/services/nook", get(service_nook))
            .route("/services/litigation", get(service_litigation))
            .route("/services/nerd", get(service_nerd))
            .route("/services/node", get(service_node))
            .route("/services/newleaf", get(service_newleaf))
            .route("/services/namesake", get(service_namesake))
            .route("/services/nucleus", get(service_nucleus))
            .route("/services/pro-bono", get(service_pro_bono))
            // Spanish (`es`) marketing surface — URL-prefix routing. Each
            // page mirrors its English twin and renders Spanish chrome +
            // transcreated prose; see docs/i18n.md.
            .route("/es", get(home_es))
            .route("/es/foundation", get(foundation_mission_es))
            .route(
                "/es/foundation/mission",
                get(|| async { axum::response::Redirect::permanent("/es/foundation") }),
            )
            .route("/es/services", get(service_index_es))
            .route("/es/services/nexus", get(service_nexus_es))
            .route("/es/services/northstar", get(service_northstar_es))
            .route("/es/services/nest", get(service_nest_es))
            .route("/es/services/nautilus", get(service_nautilus_es))
            .route("/es/services/nook", get(service_nook_es))
            .route("/es/services/litigation", get(service_litigation_es))
            .route("/es/services/nerd", get(service_nerd_es))
            .route("/es/services/node", get(service_node_es))
            .route("/es/services/newleaf", get(service_newleaf_es))
            .route("/es/services/namesake", get(service_namesake_es))
            .route("/es/services/nucleus", get(service_nucleus_es))
            .route("/es/services/pro-bono", get(service_pro_bono_es))
            .route("/es/foundation/nebula", get(nebula_landing_es))
            .route("/es/foundation/navigator", get(navigator_es))
            // Nebula is the Foundation's sharing surface: workshops,
            // show-and-tells, and presentations.
            .route(
                "/foundation/workshops",
                get(|| async { axum::response::Redirect::permanent("/foundation/nebula") }),
            )
            .route(
                "/foundation/workshops/navigator",
                get(|| async {
                    axum::response::Redirect::permanent(
                        "/foundation/nebula/workshops/use-the-navigator",
                    )
                }),
            )
            .route("/events", get(legacy_events_redirect))
            .route("/events/{slug}", get(legacy_event_redirect))
            .route(
                "/events/{slug}/calendar.ics",
                get(legacy_event_ics_redirect),
            )
            .route("/foundation/nebula", get(nebula_landing))
            .route(
                "/foundation/nebula/show-and-tell",
                get(nebula_show_tell_index),
            )
            .route("/foundation/nebula/{category}/{slug}", get(nebula_material))
            .route(
                "/foundation/nebula/{category}/{slug}/step/{step}",
                get(nebula_material_step),
            )
            // The light-table grid of every slide in a workshop, plus the
            // client-gated certificate request form it mints CSRF for.
            .route(
                "/foundation/nebula/{category}/{slug}/slides",
                get(nebula_slides),
            )
            .route(
                "/foundation/nebula/show-and-tell/{slug}/calendar.ics",
                get(nebula_show_tell_ics),
            )
            // POST-only: register for a show-and-tell. A stray GET lands the
            // visitor back on the event page where the form lives.
            .route(
                "/foundation/nebula/show-and-tell/{slug}/register",
                axum::routing::post(nebula_show_tell_register).get(
                    |AxumPath(slug): AxumPath<String>| async move {
                        axum::response::Redirect::to(&format!(
                            "/foundation/nebula/show-and-tell/{slug}"
                        ))
                    },
                ),
            )
            // POST-only: the certificate request. A stray GET lands the
            // learner back on the light table where the form lives.
            .route(
                "/foundation/nebula/{category}/{slug}/certificate",
                axum::routing::post(nebula_certificate_submit).get(
                    |AxumPath((category, slug)): AxumPath<(String, String)>| async move {
                        axum::response::Redirect::to(&format!(
                            "/foundation/nebula/{category}/{slug}/slides"
                        ))
                    },
                ),
            )
            .route("/docs", get(docs_index_page))
            .route("/docs/{slug}", get(docs_page))
            // Public, no-login template gallery + the LSP showcase — the
            // "our legal documents are plain markdown" demo surfaces.
            .route("/templates", get(templates_index))
            .route("/templates/{*path}", get(template_entry))
            // Raw template markdown as an API — the bytes the README's
            // `notation_templates/**/*.md` links point at on the website.
            .route("/api/templates/{*path}", get(api_template_raw))
            .route(
                "/lsp",
                get(|| async { axum::response::Redirect::permanent("/foundation/navigator/lsp") }),
            )
            .route("/design", get(design_page))
            .route("/statutes", get(statutes::index))
            .route("/statutes/nrs/{chapter}", get(statutes::chapter))
            .route("/statutes/nrs/{chapter}/{section}", get(statutes::section));
    }

    let mut router = router
        .nest_service("/public", static_files)
        // The git smart-HTTP transport + LFS share `AppState`, so merge
        // them while the router is still `Router<AppState>` (before state
        // is applied); the sub-routers below are already state-applied.
        .merge(git_http::routes())
        .merge(git_lfs::routes())
        .with_state(state)
        .merge(api)
        .merge(admin)
        .merge(portal)
        .merge(mcp)
        .merge(a2a_card)
        .merge(a2a_rpc)
        .merge(cli_auth);
    if let Some(oauth) = oauth_routes {
        router = router.merge(oauth);
    }
    let router = router.fallback(fallback_not_found);
    // Layer ordering (outermost first — `Router::layer` wraps the
    // chain so the LAST `.layer(...)` runs first on the request and
    // last on the response):
    //
    //   request  → request-id → trace → security-headers → propagate-id
    //            → host       → cookies → renew → handler
    //   response ← request-id ← trace ← security-headers ← propagate-id
    //            ← host       ← cookies ← renew ← handler
    //
    // We want the request-id assigned BEFORE the trace span opens
    // (so the span carries the id) and the security headers applied
    // to every response (including 3xx redirects from the host
    // layer), so they sit on the outside of the cookie + host pair.
    // Session renewal sits *inside* the cookie manager so the cookie it
    // re-issues is serialized into the response's `Set-Cookie` header.
    router
        .layer(axum::middleware::from_fn_with_state(
            session_renew,
            session_renew::renew_session,
        ))
        .layer(tower_cookies::CookieManagerLayer::new())
        .layer(host_layer)
        .layer(PropagateRequestIdLayer::new(X_REQUEST_ID))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            csp_value(),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("strict-transport-security"),
            HSTS_VALUE,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(X_REQUEST_ID, MakeRequestUuid))
}

async fn home(State(db): State<Db>, MaybeAuth(auth): MaybeAuth) -> Markup {
    let testimonials = store::testimonials::published_for_home(&db, 2)
        .await
        .unwrap_or_default();
    let cards = testimonial_cards(&testimonials);
    views::pages::home::render_in(auth, views::Locale::En, &cards)
}

/// Spanish landing page (`/es`).
async fn home_es(State(db): State<Db>, MaybeAuth(auth): MaybeAuth) -> Markup {
    let testimonials = store::testimonials::published_for_home(&db, 2)
        .await
        .unwrap_or_default();
    let cards = testimonial_cards(&testimonials);
    views::pages::home::render_in(auth, views::Locale::Es, &cards)
}

/// Human-readable publish date for the blog (e.g. `"June 19, 2026"`).
/// Kept in `web` so the `views` crate stays free of `chrono`.
fn format_blog_date(date: chrono::NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

fn format_event_datetime_range(
    start: chrono::NaiveDateTime,
    end: chrono::NaiveDateTime,
    timezone: &str,
) -> String {
    format!(
        "{}-{} {}",
        start.format("%B %-d, %Y, %-I:%M %p"),
        end.format("%-I:%M %p"),
        timezone_label(timezone)
    )
}

fn timezone_label(timezone: &str) -> &str {
    match timezone {
        "America/Los_Angeles" => "Pacific",
        "America/Denver" => "Mountain",
        "America/Chicago" => "Central",
        "America/New_York" => "Eastern",
        _ => timezone,
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
struct ShowTellPagination {
    upcoming_page: Option<usize>,
    past_page: Option<usize>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct ReferralQuery {
    #[serde(default, rename = "ref")]
    referral: Option<String>,
}

impl ReferralQuery {
    fn is_1337lawyers(&self) -> bool {
        self.referral.as_deref() == Some("1337lawyers")
    }
}

/// `GET /blog` — the firm blog index, newest post first.
async fn blog_index(State(blog): State<BlogIndex>, MaybeAuth(auth): MaybeAuth) -> Markup {
    let dates: Vec<String> = blog
        .posts()
        .iter()
        .map(|p| format_blog_date(p.date))
        .collect();
    let summaries: Vec<views::pages::blog::PostSummary<'_>> = blog
        .posts()
        .iter()
        .zip(&dates)
        .map(|(p, date)| views::pages::blog::PostSummary {
            slug: &p.slug,
            date,
            title: &p.title,
            description: &p.description,
        })
        .collect();
    views::pages::blog::render_index(&summaries, auth)
}

/// Build the kebab-case redirect target for a file-backed asset route
/// when any path segment is in the legacy underscore form, or `None`
/// when every segment is already canonical.
///
/// Borrowing the JSON:API member-name convention, every public asset URL
/// uses hyphens (see [`views::slug`]); this powers the permanent
/// redirect that lands a `…_…` link on its canonical `…-…` home, shared
/// by the blog, template, and docs routes so the rule can't drift apart.
fn kebab_redirect_path(segments: &[&str]) -> Option<String> {
    if segments.iter().any(|s| views::slug::needs_redirect(s)) {
        let path = segments
            .iter()
            .map(|s| views::slug::to_url(s))
            .collect::<Vec<_>>()
            .join("/");
        Some(format!("/{path}"))
    } else {
        None
    }
}

/// `GET /blog/{slug}` — one post, or a 404 page when the slug is unknown.
///
/// Slugs are canonically kebab-case (`thanks-apple`). A request for the
/// legacy underscore form (`thanks_apple`) is permanently redirected to
/// the hyphenated form so external links written either way resolve.
async fn blog_post(
    State(blog): State<BlogIndex>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    if let Some(to) = kebab_redirect_path(&["blog", &slug]) {
        return axum::response::Redirect::permanent(&to).into_response();
    }
    match blog.get(&slug) {
        Some(post) => {
            let date = format_blog_date(post.date);
            (
                StatusCode::OK,
                views::pages::blog::render_post(
                    &views::pages::blog::PostContent {
                        date: &date,
                        title: &post.title,
                        body_html: &post.body_html,
                    },
                    auth,
                ),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

async fn contact(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::contact::render(auth)
}

/// `GET /foundation/navigator` — the Neon Law Navigator hub: the workspace README
/// over a strip that fans out to the per-package pages below.
async fn navigator(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::navigator::render(auth)
}

/// `GET /es/foundation/navigator` — the Spanish twin of the hub: the hero
/// and sovereign-software pitch transcreated, the README body kept English.
async fn navigator_es(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::navigator::render_in(auth, views::Locale::Es)
}

// Each per-package page (`/foundation/navigator/<pkg>`) renders that
// crate's README, baked at compile time so the page can never drift from
// the repo. Paths resolve from the `web` crate manifest dir, one level up
// to the workspace root.
const CLI_README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../cli/README.md"));
const MCP_README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../mcp/README.md"));
const WEB_README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../web/README.md"));

/// `GET /foundation/navigator/lsp` — the bespoke LSP showcase + install
/// page (richer than a bare README render).
async fn navigator_lsp(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::lsp::render(auth)
}

/// `GET /foundation/navigator/cli` — the `navigator` operator CLI.
async fn navigator_cli(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::package::render_cli(
        "Neon Law Navigator CLI",
        "The navigator operator CLI — validate markdown templates, import and seed \
         data, render the ER diagram, and drive deploys.",
        CLI_README,
        "/foundation/navigator/cli",
        auth,
    )
}

/// `GET /foundation/navigator/mcp` — AIDA, Neon Law Navigator's agent,
/// reached over A2A (then MCP) from Google Gemini Enterprise.
async fn navigator_mcp(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::package::render(
        "Neon Law Navigator MCP",
        "Tag @AIDA in Google Gemini Enterprise to work with the firm's data in plain English — \
         the agent surface behind it is A2A, then MCP, over one tool catalog.",
        MCP_README,
        "/foundation/navigator/mcp",
        auth,
    )
}

/// `GET /foundation/navigator/web` — the web app + JSON API, the product
/// this very binary serves.
async fn navigator_web(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::package::render(
        "Neon Law Navigator Web",
        "The Neon Law Navigator web app and JSON API — the public site, the portal, the admin UI, \
         and the agent surfaces, all from one axum binary.",
        WEB_README,
        "/foundation/navigator/web",
        auth,
    )
}

/// `GET /foundation/notations` — the notation template tree README,
/// rendered under the Foundation brand.
async fn notation_templates(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::notation_templates::render(auth)
}

async fn foundation_mission(
    State(marketing): State<MarketingIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> Markup {
    foundation_mission_in(&marketing, auth, views::Locale::En)
}

/// Spanish mission letter (`/es/foundation`).
async fn foundation_mission_es(
    State(marketing): State<MarketingIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> Markup {
    foundation_mission_in(&marketing, auth, views::Locale::Es)
}

fn foundation_mission_in(
    marketing: &MarketingIndex,
    auth: views::AuthState,
    locale: views::Locale,
) -> Markup {
    let doc = marketing.find_localized("mission", locale);
    // The mission letters live with the other marketing fragments under
    // `web/content/marketing/`; `web/`'s CARGO_MANIFEST_DIR is `…/web`,
    // so they resolve relative to it. The "last edited" freshness line
    // tracks the locale's own source file. Returns None in production
    // where distroless has no git binary.
    let source_file = match locale {
        views::Locale::Es => "content/marketing/es/mission.md",
        views::Locale::En => "content/marketing/mission.md",
    };
    let content = doc.map_or_else(views::pages::mission::MissionContent::default, |d| {
        views::pages::mission::MissionContent {
            title: &d.title,
            description: &d.description,
            body_html: &d.body_html,
            last_edited: git_meta::last_touched(
                &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(source_file),
            ),
        }
    });
    views::pages::mission::render_in(&content, auth, locale)
}

/// `GET /foundation/transparency` — the Foundation's public-disclosure hub.
///
/// Lists the IRS determination letter (a required IRC §6104(d) disclosure,
/// served as a PDF from `/public/`) alongside the documents the Foundation
/// publishes voluntarily: bylaws, the conflict of interest policy, and the
/// quarterly board minutes loaded from `web/content/foundation/`.
async fn foundation_transparency(
    State(transparency): State<TransparencyIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> Markup {
    let to_link = |d: &transparency::TransparencyDoc| views::pages::transparency::DocLink {
        href: format!("/foundation/transparency/{}", d.slug),
        title: d.title.clone(),
        description: d.description.clone(),
    };
    let governance: Vec<_> = transparency.governance().into_iter().map(to_link).collect();
    let minutes: Vec<_> = transparency.minutes().into_iter().map(to_link).collect();
    let content = views::pages::transparency::IndexContent {
        determination_letter_href: "/public/foundation/determination-letter.pdf",
        governance: &governance,
        minutes: &minutes,
    };
    views::pages::transparency::render_index(&content, auth)
}

/// `GET /foundation/transparency/{slug}` — one transparency document
/// (`bylaws`, `conflict-of-interest`, or `minutes-YYYY-qN`), or a 404 page
/// when the slug is unknown.
async fn foundation_transparency_doc(
    State(transparency): State<TransparencyIndex>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    match transparency.get(&slug) {
        Some(doc) => {
            let canonical = format!("/foundation/transparency/{}", doc.slug);
            (
                StatusCode::OK,
                views::pages::transparency::render_doc(
                    &views::pages::transparency::DocContent {
                        title: &doc.title,
                        description: &doc.description,
                        canonical_path: &canonical,
                        body_html: &doc.body_html,
                    },
                    auth,
                ),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

/// `GET /docs` — the workspace documentation index.
async fn docs_index_page(
    State(docs): State<DocsIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> impl IntoResponse {
    render_doc_page(&docs, "index", auth)
}

/// `GET /docs/{slug}` — a workspace doc rendered from the baked `docs/`
/// tree. Public even in private mode (reference vocabulary, like
/// `/privacy`). Unknown slugs 404.
async fn docs_page(
    State(docs): State<DocsIndex>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    if let Some(to) = kebab_redirect_path(&["docs", &slug]) {
        return axum::response::Redirect::permanent(&to).into_response();
    }
    if slug == "index" {
        return axum::response::Redirect::permanent("/docs").into_response();
    }
    render_doc_page(&docs, &slug, auth)
}

fn render_doc_page(
    docs: &DocsIndex,
    slug: &str,
    auth: views::AuthState,
) -> axum::response::Response {
    match docs.find(slug) {
        Some(doc) => (
            StatusCode::OK,
            views::pages::docs::render(
                &views::pages::docs::DocContent {
                    title: &doc.title,
                    body_html: &doc.body_html,
                },
                auth,
            ),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

/// Map a curated [`template_gallery::GalleryTemplate`] to its view card.
fn template_card(
    t: &'static template_gallery::GalleryTemplate,
) -> views::pages::templates::TemplateCard<'static> {
    views::pages::templates::TemplateCard {
        href: t.detail_path(),
        name: t.name,
        title: &t.title,
        blurb: t.blurb,
        jurisdiction_label: t.jurisdiction.label(),
        jurisdiction_badge_class: t.jurisdiction.badge_class(),
    }
}

/// `GET /templates` — the public, no-login gallery index. Lists the
/// curated, client-safe subset of `notation_templates/`.
async fn templates_index(MaybeAuth(auth): MaybeAuth) -> Markup {
    let cards: Vec<_> = template_gallery::gallery()
        .iter()
        .map(template_card)
        .collect();
    views::pages::templates::index(&cards, auth)
}

/// `GET /templates/*path` — one template's detail page: the
/// notation frontmatter, a download link, and a start-a-matter CTA. A
/// template not on the curated allow-list 404s (never leaks).
async fn template_entry(
    MaybeAuth(auth): MaybeAuth,
    AxumPath(path): AxumPath<String>,
) -> impl IntoResponse {
    let (path, is_download) = path
        .strip_suffix("/download")
        .map_or((path.as_str(), false), |base| (base, true));
    let path = template_gallery::legacy_alias(path).unwrap_or(path);
    let redirect_segments = if is_download {
        ["templates", path, "download"].join("/")
    } else {
        ["templates", path].join("/")
    };
    let redirect_parts: Vec<&str> = redirect_segments.split('/').collect();
    if let Some(to) = kebab_redirect_path(&redirect_parts) {
        return axum::response::Redirect::permanent(&to).into_response();
    }
    match template_gallery::find_path(path) {
        Some(t) => {
            if is_download {
                let mut headers = axum::http::HeaderMap::new();
                headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/markdown; charset=utf-8"),
                );
                if let Ok(disposition) = HeaderValue::try_from(format!(
                    "attachment; filename=\"{}\"",
                    t.download_filename()
                )) {
                    headers.insert(header::CONTENT_DISPOSITION, disposition);
                }
                return (StatusCode::OK, headers, t.raw).into_response();
            }
            let download_href = t.download_path();
            let detail = views::pages::templates::TemplateDetail {
                card: template_card(t),
                frontmatter: t.frontmatter(),
                download_href: &download_href,
                // A serious prospect routes into the firm's contact path.
                start_matter_href: "/contact",
            };
            (
                StatusCode::OK,
                views::pages::templates::detail(&detail, auth),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

/// `GET /api/templates/*path` — the raw template markdown,
/// served inline as `text/markdown`. Unlike `/templates/.../download`
/// (the curated gallery, attachment-dispositioned), this serves any
/// `confidential: false` template under `notation_templates/` so the README's
/// `notation_templates/**/*.md` links resolve on the site. Confidential
/// templates and unknown paths 404.
async fn api_template_raw(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let path = template_api::legacy_alias(&path).unwrap_or(&path);
    let redirect_segments = ["api", "templates", path].join("/");
    let redirect_parts: Vec<&str> = redirect_segments.split('/').collect();
    if let Some(to) = kebab_redirect_path(&redirect_parts) {
        return axum::response::Redirect::permanent(&to).into_response();
    }
    match template_api::find_raw_path(path) {
        Some(raw) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown; charset=utf-8"),
            )],
            raw,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "template not found\n").into_response(),
    }
}

/// `GET /design` — the firm's living design system: the shared Bootstrap
/// cards, toasts, navbar, and brand cyan palette rendered in one place.
/// A public contributor reference, like `/lsp`.
async fn design_page(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::design::render(auth)
}

/// Which brand a [`ServicePage`] wears. Firm pages (`/services/*`) carry
/// the firm chrome, a `/es` Spanish twin, and the firm inbox; Foundation
/// pages (`/foundation/nimbus`) carry the 501(c)(3) chrome, the Foundation
/// inbox, and (being English-only) no canonical/hreflang twin.
#[derive(Clone, Copy)]
enum Surface {
    Firm,
    Foundation,
}

/// One marketing service/product page: the slug, the locale-less canonical
/// path, the English fallback title, the brand it wears, and the product's
/// Bootstrap Icon. The `/services/...`, `/es/services/...`, and
/// `/foundation/...` handlers share this table.
///
/// `icon` is the glyph name without the `bi-` prefix (e.g.
/// `"diagram-3-fill"`). These are the same marks that denoted each product
/// in the old Services dropdown; with the dropdown collapsed to a single
/// `/services` catalog, each product keeps its mark on its own page.
struct ServicePage {
    slug: &'static str,
    canonical_path: &'static str,
    fallback_title: &'static str,
    surface: Surface,
    icon: Option<&'static str>,
    hero_variant: Option<&'static str>,
}

const SERVICE_NORTHSTAR: ServicePage = ServicePage {
    slug: "northstar",
    canonical_path: "/services/northstar",
    fallback_title: "Estate planning",
    surface: Surface::Firm,
    icon: Some("star-fill"),
    hero_variant: None,
};
const SERVICE_NEST: ServicePage = ServicePage {
    slug: "nest",
    canonical_path: "/services/nest",
    fallback_title: "Corporate services",
    surface: Surface::Firm,
    icon: Some("building-fill"),
    hero_variant: None,
};
const SERVICE_NEXUS: ServicePage = ServicePage {
    slug: "nexus",
    canonical_path: "/services/nexus",
    fallback_title: "Fractional GC",
    surface: Surface::Firm,
    icon: Some("diagram-3-fill"),
    hero_variant: Some("nexus"),
};
const SERVICE_NAUTILUS: ServicePage = ServicePage {
    slug: "nautilus",
    canonical_path: "/services/nautilus",
    fallback_title: "Debt-collection help",
    surface: Surface::Firm,
    icon: Some("shield-fill-check"),
    hero_variant: None,
};
const SERVICE_NOOK: ServicePage = ServicePage {
    slug: "nook",
    canonical_path: "/services/nook",
    fallback_title: "Real-estate closing",
    surface: Surface::Firm,
    icon: Some("house-door-fill"),
    hero_variant: None,
};
const SERVICE_LITIGATION: ServicePage = ServicePage {
    slug: "litigation",
    canonical_path: "/services/litigation",
    fallback_title: "Litigation",
    surface: Surface::Firm,
    // The scales of justice (Libra). No Bootstrap Icons glyph ships a
    // balance scale, so this is the inline-SVG sentinel resolved by
    // `views::components::product_icon`.
    icon: Some("libra-scales"),
    hero_variant: None,
};
const SERVICE_NERD: ServicePage = ServicePage {
    slug: "nerd",
    canonical_path: "/services/nerd",
    fallback_title: "Expert witness",
    surface: Surface::Firm,
    icon: Some("eyeglasses"),
    hero_variant: None,
};
const SERVICE_NODE: ServicePage = ServicePage {
    slug: "node",
    canonical_path: "/services/node",
    fallback_title: "On-chain attestation",
    surface: Surface::Firm,
    icon: Some("hdd-network-fill"),
    hero_variant: None,
};
const SERVICE_NEWLEAF: ServicePage = ServicePage {
    slug: "newleaf",
    canonical_path: "/services/newleaf",
    fallback_title: "Uncontested divorce",
    surface: Surface::Firm,
    icon: Some("tree-fill"),
    hero_variant: None,
};
const SERVICE_NAMESAKE: ServicePage = ServicePage {
    slug: "namesake",
    canonical_path: "/services/namesake",
    fallback_title: "Trademark filing",
    surface: Surface::Firm,
    icon: Some("award-fill"),
    hero_variant: None,
};
const SERVICE_NUCLEUS: ServicePage = ServicePage {
    slug: "nucleus",
    canonical_path: "/services/nucleus",
    fallback_title: "Fund formation",
    surface: Surface::Firm,
    icon: Some("bank2"),
    hero_variant: None,
};
/// Pro bono — free legal help for people who cannot afford a lawyer, with
/// the Neon Law Foundation and legal-aid partners. A firm-surface page, but
/// not a catalog product: it closes the `/services` lineup as a free card.
const SERVICE_PROBONO: ServicePage = ServicePage {
    slug: "pro-bono",
    canonical_path: "/services/pro-bono",
    fallback_title: "Pro bono",
    surface: Surface::Firm,
    icon: Some("heart-fill"),
    hero_variant: None,
};
/// Neon Law Foundation Nimbus — the 501(c)(3)'s white-label, two-week
/// install-it-on-your-own-cloud engagement. Foundation-branded, so it
/// wears the Foundation chrome and writes to the Foundation inbox.
const SERVICE_NIMBUS: ServicePage = ServicePage {
    slug: "nimbus",
    canonical_path: "/foundation/nimbus",
    fallback_title: "Neon Law Foundation Nimbus",
    surface: Surface::Foundation,
    icon: Some("cloud-fill"),
    hero_variant: None,
};

async fn service_northstar(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NORTHSTAR, a.0, views::Locale::En)
}
async fn service_nest(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NEST, a.0, views::Locale::En)
}
async fn service_nexus(s: State<MarketingIndex>, State(db): State<Db>, a: MaybeAuth) -> Markup {
    render_service_with_product_testimonials(
        &s.0,
        &db,
        &SERVICE_NEXUS,
        a.0,
        views::Locale::En,
        false,
    )
    .await
}
async fn service_nautilus(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NAUTILUS, a.0, views::Locale::En)
}
async fn service_nook(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NOOK, a.0, views::Locale::En)
}
async fn service_litigation(
    s: State<MarketingIndex>,
    State(db): State<Db>,
    Query(q): Query<ReferralQuery>,
    a: MaybeAuth,
) -> Markup {
    render_service_with_product_testimonials(
        &s.0,
        &db,
        &SERVICE_LITIGATION,
        a.0,
        views::Locale::En,
        q.is_1337lawyers(),
    )
    .await
}
async fn service_nerd(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NERD, a.0, views::Locale::En)
}
async fn service_node(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NODE, a.0, views::Locale::En)
}
async fn service_newleaf(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NEWLEAF, a.0, views::Locale::En)
}
async fn service_namesake(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NAMESAKE, a.0, views::Locale::En)
}
async fn service_nucleus(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NUCLEUS, a.0, views::Locale::En)
}
async fn service_pro_bono(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_PROBONO, a.0, views::Locale::En)
}
/// `GET /services` — the public, DB-backed catalog that replaced the old
/// Services dropdown. Lists every active product at its `list_price_cents`
/// with a link out to each `/services/<slug>` detail page.
async fn service_index(State(db): State<Db>, MaybeAuth(auth): MaybeAuth) -> Markup {
    render_products(&db, auth, views::Locale::En).await
}
async fn foundation_nimbus(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NIMBUS, a.0, views::Locale::En)
}

async fn service_northstar_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NORTHSTAR, a.0, views::Locale::Es)
}
async fn service_nest_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NEST, a.0, views::Locale::Es)
}
async fn service_nexus_es(s: State<MarketingIndex>, State(db): State<Db>, a: MaybeAuth) -> Markup {
    render_service_with_product_testimonials(
        &s.0,
        &db,
        &SERVICE_NEXUS,
        a.0,
        views::Locale::Es,
        false,
    )
    .await
}
async fn service_nautilus_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NAUTILUS, a.0, views::Locale::Es)
}
async fn service_nook_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NOOK, a.0, views::Locale::Es)
}
async fn service_litigation_es(
    s: State<MarketingIndex>,
    State(db): State<Db>,
    Query(q): Query<ReferralQuery>,
    a: MaybeAuth,
) -> Markup {
    render_service_with_product_testimonials(
        &s.0,
        &db,
        &SERVICE_LITIGATION,
        a.0,
        views::Locale::Es,
        q.is_1337lawyers(),
    )
    .await
}
async fn service_nerd_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NERD, a.0, views::Locale::Es)
}
async fn service_node_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NODE, a.0, views::Locale::Es)
}
async fn service_newleaf_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NEWLEAF, a.0, views::Locale::Es)
}
async fn service_namesake_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NAMESAKE, a.0, views::Locale::Es)
}
async fn service_nucleus_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_NUCLEUS, a.0, views::Locale::Es)
}
async fn service_pro_bono_es(s: State<MarketingIndex>, a: MaybeAuth) -> Markup {
    render_service(&s.0, &SERVICE_PROBONO, a.0, views::Locale::Es)
}
/// `GET /es/services` — the Spanish twin of the catalog.
async fn service_index_es(State(db): State<Db>, MaybeAuth(auth): MaybeAuth) -> Markup {
    render_products(&db, auth, views::Locale::Es).await
}

fn render_service(
    marketing: &MarketingIndex,
    page: &ServicePage,
    auth: views::AuthState,
    locale: views::Locale,
) -> Markup {
    render_service_with_testimonials(marketing, page, auth, locale, &[], false)
}

async fn render_service_with_product_testimonials(
    marketing: &MarketingIndex,
    db: &Db,
    page: &ServicePage,
    auth: views::AuthState,
    locale: views::Locale,
    show_referral_terminal: bool,
) -> Markup {
    let testimonials = store::testimonials::published_for_product(db, page.slug, 2)
        .await
        .unwrap_or_default();
    let cards = testimonial_cards(&testimonials);
    render_service_with_testimonials(
        marketing,
        page,
        auth,
        locale,
        &cards,
        show_referral_terminal,
    )
}

fn render_service_with_testimonials(
    marketing: &MarketingIndex,
    page: &ServicePage,
    auth: views::AuthState,
    locale: views::Locale,
    testimonials: &[views::components::TestimonialCard<'_>],
    show_referral_terminal: bool,
) -> Markup {
    // Brand-driven fallback description used when the marketing
    // markdown doesn't ship a copy for this service slug. Leaked
    // once at first call so the rest of the function can keep its
    // `&'static str` tuple shape.
    static FALLBACK_DESCRIPTION: std::sync::LazyLock<&'static str> =
        std::sync::LazyLock::new(|| {
            Box::leak(
                format!(
                    "Flat-fee legal services from {}.",
                    views::brand::FIRM_BRAND.site_name,
                )
                .into_boxed_str(),
            )
        });
    let slug = page.slug;
    let fallback_title = page.fallback_title;
    let doc = marketing.find_localized(slug, locale);
    let (title, description, body_html) = doc.map_or(
        (
            fallback_title,
            *FALLBACK_DESCRIPTION,
            "<p>Email <a href=\"mailto:support@example.com\">support@example.com</a> \
             for a flat-fee quote.</p>",
        ),
        |d| {
            (
                d.title.as_str(),
                d.description.as_str(),
                d.body_html.as_str(),
            )
        },
    );
    // Map the owned content-schema cards onto the borrowed view input.
    // `views` owns the markup, `web` owns the schema; the strings live
    // in `doc` for the duration of this render.
    let pricing: Vec<views::components::PricingCard<'_>> = doc
        .map(|d| d.pricing.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|c| views::components::PricingCard {
            title: &c.title,
            price: &c.price,
            cadence: c.cadence.as_deref(),
            blurb: &c.blurb,
            features: c.features.iter().map(String::as_str).collect(),
            cta_label: &c.cta_label,
            cta_href: &c.cta_href,
            featured: c.featured,
            featured_label: c.featured_label.as_deref(),
        })
        .collect();
    // Desktop column count for the pricing row. By default one column per
    // card (tiered plans sit three across; a flat-fee menu may run to
    // four), but a page can force the layout with a top-level
    // `pricing_cols:` frontmatter key — `pricing_cols: 1` stacks the cards
    // one per row at every breakpoint (e.g. Nimbus's two offers).
    let pricing_cols = doc
        .and_then(|d| d.metadata.get("pricing_cols"))
        .and_then(|v| v.parse::<u8>().ok())
        .map_or_else(
            || u8::try_from(pricing.len().clamp(1, 4)).unwrap_or(3),
            |n| n.clamp(1, 4),
        );
    // Optional `hero_image:` frontmatter slug turns the page into a split
    // hero (see `views::pages::service`). Absent → the body renders flat.
    let hero_image = doc.and_then(|d| d.metadata.get("hero_image").map(String::as_str));
    // Optional `hero_scene: clouds` frontmatter swaps the hero's moving grid
    // floor for a soft drifting cloud field (Nimbus, the cloud install).
    let hero_clouds = doc
        .and_then(|d| d.metadata.get("hero_scene"))
        .is_some_and(|v| v == "clouds");
    // Brand the page from its surface: the firm chrome + inbox for
    // `/services/*`, the Foundation chrome + inbox for a Foundation
    // product. A Foundation product is English-only, so it omits the
    // canonical path — that path drives the hreflang twin + Spanish
    // language switcher, which would 404 without a `/es` mirror.
    let (brand, cta_email, canonical) = match page.surface {
        Surface::Firm => (
            *views::brand::FIRM_BRAND,
            views::brand::firm_email(),
            Some(page.canonical_path),
        ),
        Surface::Foundation => (
            *views::brand::FOUNDATION_BRAND,
            views::brand::foundation_email(),
            None,
        ),
    };
    let referral_terminal_close_href =
        show_referral_terminal.then(|| views::i18n::localize_href(page.canonical_path, locale));
    views::pages::service::render_in(
        &views::pages::service::ServiceContent {
            title,
            description,
            body_html,
            pricing,
            pricing_cols,
            hero_image,
            hero_variant: page.hero_variant,
            hero_clouds,
            brand,
            cta_email,
            icon: page.icon,
            testimonials,
            referral_terminal_close_href: referral_terminal_close_href.as_deref(),
        },
        auth,
        locale,
        canonical,
    )
}

fn testimonial_cards(
    testimonials: &[store::testimonials::PublishedTestimonial],
) -> Vec<views::components::TestimonialCard<'_>> {
    testimonials
        .iter()
        .map(|t| {
            let attribution = t
                .attribution_label
                .as_deref()
                .unwrap_or(t.person_name.as_str());
            views::components::TestimonialCard {
                quote: &t.quote,
                attribution,
                detail: t.person_title.as_deref().or(Some(t.project_name.as_str())),
                profile_image_url: t.profile_image_url.as_deref(),
                product_label: t.product_code.as_deref().and_then(product_label),
            }
        })
        .collect()
}

fn product_label(code: &str) -> Option<&'static str> {
    match code {
        "litigation" => Some("Litigation"),
        "namesake" => Some("Namesake"),
        "nautilus" => Some("Nautilus"),
        "nest" => Some("Nest"),
        "nerd" => Some("Nerd"),
        "newleaf" => Some("Newleaf"),
        "nexus" => Some("Nexus"),
        "node" => Some("Node"),
        "nook" => Some("Nook"),
        "northstar" => Some("Northstar"),
        "nucleus" => Some("Nucleus"),
        _ => None,
    }
}

#[cfg(test)]
mod testimonial_label_tests {
    use super::{product_label, ReferralQuery};

    #[test]
    fn product_label_covers_the_seeded_catalog() {
        for code in [
            "litigation",
            "namesake",
            "nautilus",
            "nest",
            "nerd",
            "newleaf",
            "nexus",
            "node",
            "nook",
            "northstar",
            "nucleus",
        ] {
            assert!(product_label(code).is_some(), "{code} should have a label");
        }
        assert_eq!(product_label("unknown"), None);
    }

    #[test]
    fn referral_query_only_matches_the_1337lawyers_campaign() {
        assert!(ReferralQuery {
            referral: Some("1337lawyers".into())
        }
        .is_1337lawyers());
        assert!(!ReferralQuery {
            referral: Some("other".into())
        }
        .is_1337lawyers());
        assert!(!ReferralQuery::default().is_1337lawyers());
    }
}

/// Map a product `code` to the `/services/<slug>` marketing page that
/// describes it. Each page slug now matches its product code, so the
/// mapping is a straight `/services/<code>`; it stays explicit (rather
/// than formatting the code in) to keep the route table auditable and to
/// fall back to the services index for an unknown code.
fn product_service_path(code: &str) -> &'static str {
    match code {
        "northstar" => "/services/northstar",
        "nest" => "/services/nest",
        "nexus" => "/services/nexus",
        "nautilus" => "/services/nautilus",
        "nook" => "/services/nook",
        "litigation" => "/services/litigation",
        "nerd" => "/services/nerd",
        "node" => "/services/node",
        "newleaf" => "/services/newleaf",
        "namesake" => "/services/namesake",
        "nucleus" => "/services/nucleus",
        _ => "/services",
    }
}

/// The Bootstrap Icon glyph for a product `code` — the same mark its
/// `/services/<slug>` detail page wears (kept in lockstep with the
/// `SERVICE_*` consts above), so the catalog card and the page agree. An
/// unlisted code renders no icon.
fn product_icon_for(code: &str) -> Option<&'static str> {
    match code {
        "northstar" => Some("star-fill"),
        "nest" => Some("building-fill"),
        "nexus" => Some("diagram-3-fill"),
        "nautilus" => Some("shield-fill-check"),
        "nook" => Some("house-door-fill"),
        "litigation" => Some("libra-scales"),
        "nerd" => Some("eyeglasses"),
        "node" => Some("hdd-network-fill"),
        "newleaf" => Some("tree-fill"),
        "namesake" => Some("award-fill"),
        "nucleus" => Some("bank2"),
        _ => None,
    }
}

/// The i18n key for a product's one-sentence catalog description, by
/// `code`. An unlisted product falls back to a generic flat-fee line so a
/// new code never renders an empty card.
fn product_description_key(code: &str) -> &'static str {
    match code {
        "nest" => "products.desc_nest",
        "nexus" => "products.desc_nexus",
        "northstar" => "products.desc_northstar",
        "nautilus" => "products.desc_nautilus",
        "nook" => "products.desc_nook",
        "litigation" => "products.desc_litigation",
        "nerd" => "products.desc_nerd",
        "node" => "products.desc_node",
        "newleaf" => "products.desc_newleaf",
        "namesake" => "products.desc_namesake",
        "nucleus" => "products.desc_nucleus",
        _ => "products.desc_default",
    }
}

/// The curated `/services` display order, by product `code`. The catalog
/// is a deliberate lineup — the firm's flat-fee products in ascending
/// "repdigit" order by leading digit (Nest $1,111 → Nexus $2,222 →
/// Northstar $3,333 → Node $44 → Newleaf $555 → Nautilus $66 → Namesake
/// $777 → Nucleus $8,888 → Nook $9,999), followed by Neon Law Nerd
/// (expert witness) and 1337 Lawyers (litigation) — not an alphabetical
/// or strictly by-price list. A `code` not listed here sorts after the
/// curated set, keeping its `list_active` display-name position. This is
/// a presentation choice, so it lives in the render layer rather than on
/// the product row.
const CATALOG_ORDER: [&str; 11] = [
    "nest",
    "nexus",
    "northstar",
    "node",
    "newleaf",
    "nautilus",
    "namesake",
    "nucleus",
    "nook",
    "nerd",
    "litigation",
];

/// Position of `code` in [`CATALOG_ORDER`]; unlisted codes sort last.
fn catalog_rank(code: &str) -> usize {
    CATALOG_ORDER
        .iter()
        .position(|c| *c == code)
        .unwrap_or(CATALOG_ORDER.len())
}

/// Render the `/services` catalog from the `products` table. Numeric card
/// prices are formatted from `list_price_cents`, except litigation: it no
/// longer publishes a dollar figure, so the card advertises quoted
/// phase-based pricing. A DB error degrades to an empty catalog rather
/// than a 500 (the page is public chrome, not a transaction).
async fn render_products(db: &Db, auth: views::AuthState, locale: views::Locale) -> Markup {
    // Owned display fields outlive the borrowed `ProductCard` slice below.
    struct Owned {
        display_name: String,
        price: String,
        cadence_suffix: String,
        description: String,
        learn_href: String,
        icon: Option<&'static str>,
    }
    let mut products = store::products::list_active(db).await.unwrap_or_default();
    // `list_active` returns display-name order; re-sort into the curated
    // catalog lineup. `sort_by_key` is stable, so any product outside
    // `CATALOG_ORDER` keeps its display-name position after the curated set.
    products.sort_by_key(|p| catalog_rank(&p.code));
    let mut owned: Vec<Owned> = products
        .into_iter()
        .map(|p| {
            let (price, cadence_suffix) = if p.code == "litigation" {
                (
                    views::i18n::t(locale, "products.litigation_phase_price"),
                    views::i18n::t(locale, "products.litigation_phase_suffix"),
                )
            } else {
                (
                    store::products::format_price(p.list_price_cents),
                    store::products::cadence_suffix(&p.cadence).to_string(),
                )
            };
            Owned {
                display_name: p.display_name,
                price,
                cadence_suffix,
                description: views::i18n::t(locale, product_description_key(&p.code)),
                learn_href: views::i18n::localize_href(product_service_path(&p.code), locale),
                icon: product_icon_for(&p.code),
            }
        })
        .collect();
    // Pro bono closes the catalog: free legal help for people who can't
    // afford a lawyer, with the Neon Law Foundation and legal-aid partners.
    // It is not a billable `products` row (no price, no Xero item), so it is
    // a presentation-only card appended after the curated lineup — always
    // last, under every priced product.
    owned.push(Owned {
        display_name: views::i18n::t(locale, "products.probono_name"),
        price: views::i18n::t(locale, "products.free"),
        cadence_suffix: String::new(),
        description: views::i18n::t(locale, "products.desc_probono"),
        learn_href: views::i18n::localize_href("/services/pro-bono", locale),
        icon: Some("heart-fill"),
    });
    let cards: Vec<views::pages::products::ProductCard<'_>> = owned
        .iter()
        .map(|o| views::pages::products::ProductCard {
            display_name: &o.display_name,
            price: &o.price,
            cadence_suffix: &o.cadence_suffix,
            description: &o.description,
            learn_href: &o.learn_href,
            icon: o.icon,
        })
        .collect();
    views::pages::products::index_in(&cards, auth, locale, Some("/services"))
}

async fn privacy(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::privacy::render(auth)
}

async fn terms(MaybeAuth(auth): MaybeAuth) -> Markup {
    views::pages::terms::render(auth)
}

/// `GET /docusign/consent-callback` — the ceremonial landing for the
/// DocuSign JWT-grant one-time consent click.
///
/// JWT grant never sends an authorization `code` back, so this URI exists
/// only so the `Allow` button has somewhere to redirect; the page is
/// purely informational. It is registered as the app's Redirect URI (see
/// [`docs/docusign-esignature.md`](../docs/docusign-esignature.md)),
/// deliberately distinct from the OIDC `/auth/callback`, and is exempt
/// from the private-mode gate so the operator lands on a confirmation
/// rather than a login bounce or a 404.
async fn docusign_consent_callback() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="robots" content="noindex";
                title { "DocuSign consent recorded — Neon Law" }
                style {
                    "body{font-family:system-ui,sans-serif;background:#0b0b0f;color:#e8e8ea;"
                    "display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}"
                    ".card{max-width:34rem;padding:2.5rem;text-align:center}"
                    "h1{color:#22d3ee;font-size:1.5rem;margin:0 0 1rem}"
                    "p{line-height:1.6;margin:0 0 .75rem}code{color:#22d3ee}"
                }
            }
            body {
                main .card {
                    h1 { "Consent recorded" }
                    p {
                        "DocuSign consent for the Neon Law Navigator integration has been granted. "
                        "You can close this tab — JWT grant does not use the redirect, so no "
                        "further action is needed here."
                    }
                    p { "The server can now mint access tokens for this account." }
                }
            }
        }
    }
}

/// Route prefix for the Foundation's sharing surface.
const NEBULA_BASE: &str = "/foundation/nebula";

fn nebula_material_base(category: &str) -> String {
    format!("{NEBULA_BASE}/{category}")
}

async fn nebula_landing(
    State(workshops): State<WorkshopIndex>,
    State(events): State<EventIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> Markup {
    render_nebula_landing(&workshops, &events, auth, views::Locale::En)
}

async fn nebula_landing_es(
    State(workshops): State<WorkshopIndex>,
    State(events): State<EventIndex>,
    MaybeAuth(auth): MaybeAuth,
) -> Markup {
    render_nebula_landing(&workshops, &events, auth, views::Locale::Es)
}

fn render_nebula_landing(
    workshops: &WorkshopIndex,
    events: &EventIndex,
    auth: views::AuthState,
    locale: views::Locale,
) -> Markup {
    // Each card links to its Nebula category one level down. The hrefs
    // are owned, so hold them alive while the borrowing cards build.
    let hrefs: Vec<String> = workshops
        .materials()
        .iter()
        .map(|m| format!("{NEBULA_BASE}/{}/{}", m.category, m.slug))
        .collect();
    let workshop_cards: Vec<workshop_views::MaterialCard<'_>> = workshops
        .materials()
        .iter()
        .zip(&hrefs)
        .filter(|(m, _)| m.category == "workshops")
        .map(|(m, href)| workshop_views::MaterialCard {
            href,
            title: &m.title,
            audience: &m.audience,
            benefit: &m.benefit,
        })
        .collect();
    let presentation_cards: Vec<workshop_views::MaterialCard<'_>> = workshops
        .materials()
        .iter()
        .zip(&hrefs)
        .filter(|(m, _)| m.category == "presentations")
        .map(|(m, href)| workshop_views::MaterialCard {
            href,
            title: &m.title,
            audience: &m.audience,
            benefit: &m.benefit,
        })
        .collect();
    // The landing previews the soonest three upcoming show-and-tells plus the
    // single most recent past one; the full paginated list lives behind the
    // "View all show-and-tells" link.
    let today = chrono::Local::now().date_naive();
    let preview = landing_show_tell_preview(events, today);
    let show_tell_meta: Vec<(String, String, String)> = preview
        .iter()
        .map(|event| {
            (
                format!("{NEBULA_BASE}/show-and-tell/{}", event.public_slug),
                format_event_datetime_range(event.starts_at, event.ends_at, &event.timezone),
                event.place(),
            )
        })
        .collect();
    let event_cards: Vec<workshop_views::EventCard<'_>> = preview
        .iter()
        .zip(&show_tell_meta)
        .map(|(event, (href, time, place))| workshop_views::EventCard {
            href,
            title: &event.title,
            time,
            place,
            description: &event.description,
        })
        .collect();
    workshop_views::landing_in(
        &workshop_cards,
        &presentation_cards,
        &event_cards,
        auth,
        locale,
    )
}

/// How many upcoming show-and-tells the Nebula landing previews before the
/// "View all show-and-tells" link.
const LANDING_UPCOMING_PREVIEW: usize = 3;

/// The show-and-tells the Nebula landing previews: the soonest
/// [`LANDING_UPCOMING_PREVIEW`] upcoming gatherings (nearest first) plus the
/// single most recent past one (so the section reads "what's next, and the
/// last one we ran"). The full paginated history lives at the show-and-tell
/// index.
fn landing_show_tell_preview(events: &EventIndex, today: chrono::NaiveDate) -> Vec<&Event> {
    let mut preview: Vec<&Event> = events
        .upcoming(today)
        .into_iter()
        .take(LANDING_UPCOMING_PREVIEW)
        .collect();
    preview.extend(events.past(today).into_iter().next());
    preview
}

async fn legacy_events_redirect() -> impl IntoResponse {
    axum::response::Redirect::permanent(NEBULA_BASE)
}

fn legacy_event_destination(events: &EventIndex, slug: &str) -> Option<String> {
    events
        .get_public(slug)
        .or_else(|| events.get(slug))
        .map(|event| format!("{NEBULA_BASE}/show-and-tell/{}", event.public_slug))
}

struct EventCardMeta {
    detail_href: String,
    calendar_href: String,
    time: String,
    place: String,
    image_alt: String,
}

fn event_card_meta(event: &Event) -> EventCardMeta {
    let detail_href = format!("{NEBULA_BASE}/show-and-tell/{}", event.public_slug);
    EventCardMeta {
        calendar_href: format!("{detail_href}/calendar.ics"),
        detail_href,
        time: format_event_datetime_range(event.starts_at, event.ends_at, &event.timezone),
        place: event.place(),
        image_alt: event
            .image_alt
            .clone()
            .unwrap_or_else(|| format!("{} event image", event.title)),
    }
}

fn total_pages(total: usize) -> usize {
    total.div_ceil(SHOW_TELL_EVENTS_PER_PAGE).max(1)
}

fn clamped_page(requested: Option<usize>, total: usize) -> usize {
    requested.unwrap_or(1).clamp(1, total_pages(total))
}

fn event_page_slice<'a>(events: &[&'a Event], page: usize) -> Vec<&'a Event> {
    let start = (page - 1) * SHOW_TELL_EVENTS_PER_PAGE;
    events
        .iter()
        .skip(start)
        .take(SHOW_TELL_EVENTS_PER_PAGE)
        .copied()
        .collect()
}

fn show_tell_page_href(upcoming_page: usize, past_page: usize) -> String {
    format!("{NEBULA_BASE}/show-and-tell?upcoming_page={upcoming_page}&past_page={past_page}")
}

fn event_pager<'a>(
    current_page: usize,
    total: usize,
    previous_href: Option<&'a str>,
    next_href: Option<&'a str>,
) -> workshop_views::EventPager<'a> {
    workshop_views::EventPager {
        previous_href,
        next_href,
        current_page,
        total_pages: total_pages(total),
    }
}

async fn nebula_show_tell_index(
    State(events): State<EventIndex>,
    MaybeAuth(auth): MaybeAuth,
    Query(pagination): Query<ShowTellPagination>,
) -> Markup {
    let today = chrono::Local::now().date_naive();
    render_nebula_show_tell_index(&events, auth, today, pagination)
}

fn render_nebula_show_tell_index(
    events: &EventIndex,
    auth: views::AuthState,
    today: chrono::NaiveDate,
    pagination: ShowTellPagination,
) -> Markup {
    let upcoming = events.upcoming(today);
    let past = events.past(today);
    let upcoming_page = clamped_page(pagination.upcoming_page, upcoming.len());
    let past_page = clamped_page(pagination.past_page, past.len());
    let upcoming_page_events = event_page_slice(&upcoming, upcoming_page);
    let past_page_events = event_page_slice(&past, past_page);
    let upcoming_meta: Vec<_> = upcoming_page_events
        .iter()
        .map(|event| event_card_meta(event))
        .collect();
    let past_meta: Vec<_> = past_page_events
        .iter()
        .map(|event| event_card_meta(event))
        .collect();
    let upcoming_items: Vec<_> = upcoming_page_events
        .iter()
        .zip(&upcoming_meta)
        .map(|(event, meta)| workshop_views::EventListItem {
            detail_href: &meta.detail_href,
            calendar_href: &meta.calendar_href,
            title: &event.title,
            time: &meta.time,
            place: &meta.place,
            description: &event.description,
            image_url: event.image_url.as_deref(),
            image_alt: &meta.image_alt,
        })
        .collect();
    let past_items: Vec<_> = past_page_events
        .iter()
        .zip(&past_meta)
        .map(|(event, meta)| workshop_views::EventListItem {
            detail_href: &meta.detail_href,
            calendar_href: &meta.calendar_href,
            title: &event.title,
            time: &meta.time,
            place: &meta.place,
            description: &event.description,
            image_url: event.image_url.as_deref(),
            image_alt: &meta.image_alt,
        })
        .collect();
    let upcoming_prev =
        (upcoming_page > 1).then(|| show_tell_page_href(upcoming_page - 1, past_page));
    let upcoming_next = (upcoming_page < total_pages(upcoming.len()))
        .then(|| show_tell_page_href(upcoming_page + 1, past_page));
    let past_prev = (past_page > 1).then(|| show_tell_page_href(upcoming_page, past_page - 1));
    let past_next = (past_page < total_pages(past.len()))
        .then(|| show_tell_page_href(upcoming_page, past_page + 1));
    let page = workshop_views::ShowTellIndex {
        upcoming: &upcoming_items,
        past: &past_items,
        upcoming_pager: event_pager(
            upcoming_page,
            upcoming.len(),
            upcoming_prev.as_deref(),
            upcoming_next.as_deref(),
        ),
        past_pager: event_pager(
            past_page,
            past.len(),
            past_prev.as_deref(),
            past_next.as_deref(),
        ),
    };
    workshop_views::show_tell_index(&page, auth)
}

async fn legacy_event_redirect(
    State(events): State<EventIndex>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    match legacy_event_destination(&events, &slug) {
        Some(destination) => axum::response::Redirect::permanent(&destination).into_response(),
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

async fn legacy_event_ics_redirect(
    State(events): State<EventIndex>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    match legacy_event_destination(&events, &slug) {
        Some(destination) => {
            let calendar = format!("{destination}/calendar.ics");
            axum::response::Redirect::permanent(&calendar).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Build the table of contents shared by the overview and every step.
fn step_summaries(m: &WorkshopMaterial) -> Vec<workshop_views::StepSummary<'_>> {
    m.sections
        .iter()
        .enumerate()
        .map(|(i, s)| workshop_views::StepSummary {
            number: i + 1,
            title: &s.title,
        })
        .collect()
}

/// Serve a raw Markdown document as `text/markdown` — the
/// machine-readable twin of a stepped-content page. LLM crawlers and
/// the on-page "Copy as Markdown" button both fetch this; it is the one
/// canonical source for the corpus, so the HTML view never embeds the
/// raw markdown itself.
fn markdown_response(raw: &str) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        )],
        raw.to_owned(),
    )
}

/// Resolve the absolute base URL (`scheme://authority`) for links in
/// machine-readable artifacts. Prefers `CANONICAL_HOST`; falls back to
/// the request `Host` header in dev. Mirrors the A2A agent card's
/// authority resolution so every absolute URL the site advertises uses
/// the same host, with no hard-coded domain (OSS forks get their own).
fn resolve_base_url(canonical_host: &CanonicalHost, headers: &axum::http::HeaderMap) -> String {
    let authority = canonical_host
        .host()
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "www.example.com".to_string());
    let scheme = if authority.starts_with("localhost")
        || authority.starts_with("127.0.0.1")
        || authority.starts_with("0.0.0.0")
    {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{authority}")
}

/// `/llms.txt` — the machine-readable corpus index in the
/// [llmstxt.org](https://llmstxt.org) convention: an H1, a one-line
/// summary, then one bullet per Markdown document the site serves so an
/// LLM crawler discovers every `.md` twin from a single file instead of
/// scraping rendered HTML. URLs are absolute and derived from
/// `CANONICAL_HOST` (see [`resolve_base_url`]), so a fork advertises its
/// own domain with no edits.
async fn llms_txt(
    State(workshops): State<WorkshopIndex>,
    State(canonical_host): State<CanonicalHost>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    use std::fmt::Write as _;
    let base = resolve_base_url(&canonical_host, &headers);
    let brand = &*views::brand::FIRM_BRAND;
    let mut out = format!("# {}\n\n> {}\n", brand.site_name, brand.tagline);

    if !workshops.materials().is_empty() {
        out.push_str("\n## Nebula\n\n");
        for m in workshops.materials() {
            let _ = writeln!(
                out,
                "- [{}]({base}{NEBULA_BASE}/{}/{}.md): {}",
                m.title, m.category, m.slug, m.description
            );
        }
    }

    markdown_response(&out)
}

async fn nebula_material(
    State(app): State<AppState>,
    MaybeAuth(auth): MaybeAuth,
    cookies: tower_cookies::Cookies,
    AxumPath((category, slug)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    if category == "show-and-tell" {
        return nebula_show_tell(&app, &cookies, auth, &slug).into_response();
    }
    // `…/{slug}.md` is the raw-Markdown twin of `…/{slug}`. matchit
    // captures the whole `readme.md` segment into `slug`, so we branch
    // on the suffix here rather than registering a second route.
    if let Some(stem) = slug.strip_suffix(".md") {
        return match app.workshops.find_in_category(&category, stem) {
            Some(m) => markdown_response(&m.raw_markdown).into_response(),
            None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
        };
    }
    if let Some(m) = app.workshops.find_in_category(&category, &slug) {
        let steps = step_summaries(m);
        let material_base = nebula_material_base(&m.category);
        let md_href = format!("{material_base}/{}.md", m.slug);
        (
            StatusCode::OK,
            workshop_views::overview(
                &workshop_views::MaterialOverview {
                    base: &material_base,
                    slug: &m.slug,
                    title: &m.title,
                    description: &m.description,
                    intro_html: &m.intro_html,
                    body_html: &m.body_html,
                    steps: &steps,
                    md_href: &md_href,
                },
                auth,
            ),
        )
            .into_response()
    } else {
        (StatusCode::NOT_FOUND, views::not_found_page()).into_response()
    }
}

/// Dedicated double-submit CSRF cookie for the show-and-tell registration
/// form. Distinct from the certificate cookie so the two forms never
/// clobber each other across tabs.
const NEBULA_REGISTER_CSRF_COOKIE_NAME: &str = "navigator_nebula_register_csrf";

fn nebula_show_tell(
    app: &AppState,
    cookies: &tower_cookies::Cookies,
    auth: views::AuthState,
    slug: &str,
) -> impl IntoResponse {
    match app.events.get_public(slug) {
        Some(event) => {
            let time = format_event_datetime_range(event.starts_at, event.ends_at, &event.timezone);
            let place = event.place();
            let ics_url = format!(
                "{NEBULA_BASE}/show-and-tell/{}/calendar.ics",
                event.public_slug
            );
            let register_action =
                format!("{NEBULA_BASE}/show-and-tell/{}/register", event.public_slug);
            let upcoming = event.date >= chrono::Local::now().date_naive();
            // Only mint a CSRF token (and set its cookie) when the form is
            // actually rendered — i.e. for an upcoming event.
            let csrf = if upcoming {
                crate::password_reset::mint_csrf_with(
                    &app.sessions,
                    secure_cookies(app),
                    cookies,
                    NEBULA_REGISTER_CSRF_COOKIE_NAME,
                )
            } else {
                String::new()
            };
            (
                StatusCode::OK,
                workshop_views::show_tell(
                    &workshop_views::ShowTellDetail {
                        title: &event.title,
                        description: &event.description,
                        time: &time,
                        place: &place,
                        upcoming,
                        register_action: &register_action,
                        csrf_token: &csrf,
                        ics_url: &ics_url,
                        body_html: &event.body_html,
                        video_url: event.video_url.as_deref(),
                        recap_url: event.recap_url.as_deref(),
                    },
                    auth,
                ),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

/// Form body for the show-and-tell registration (`POST …/{slug}/register`).
#[derive(serde::Deserialize)]
struct RegisterForm {
    email: String,
    #[serde(default)]
    csrf_token: String,
}

/// `POST …/{slug}/register` — a visitor registers for an upcoming
/// show-and-tell. Validates the double-submit CSRF token, then records
/// only the email against the event (data minimization). The reply is the
/// same neutral confirmation whether the email was new or already on the
/// list, so the endpoint is not an address-enumeration oracle.
async fn nebula_show_tell_register(
    State(app): State<AppState>,
    MaybeAuth(auth): MaybeAuth,
    cookies: tower_cookies::Cookies,
    AxumPath(slug): AxumPath<String>,
    axum::extract::Form(form): axum::extract::Form<RegisterForm>,
) -> impl IntoResponse {
    use store::events::RegisterOutcome::{AlreadyRegistered, EventNotFound, Registered};

    // Resolve the (published) event first so a draft or unknown slug 404s
    // before we touch CSRF or the database.
    let Some(event) = app.events.get_public(&slug) else {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    };
    if !crate::password_reset::verify_csrf_with(
        &app.sessions,
        &cookies,
        &form.csrf_token,
        NEBULA_REGISTER_CSRF_COOKIE_NAME,
    ) {
        return (StatusCode::BAD_REQUEST, "invalid or missing CSRF token").into_response();
    }
    cookies.add(crate::oauth::expired_cookie(
        NEBULA_REGISTER_CSRF_COOKIE_NAME,
    ));

    let email = form.email.trim();
    if !email.contains('@') || email.len() > 254 {
        return (
            StatusCode::BAD_REQUEST,
            "Please enter a valid email address.",
        )
            .into_response();
    }

    let detail_href = format!("{NEBULA_BASE}/show-and-tell/{}", event.public_slug);
    let ics_url = format!("{detail_href}/calendar.ics");
    match store::events::register(&app.db, &event.public_slug, email).await {
        // A real write happened (new email) or the email was already on the
        // list — either way the stored state is consistent, so the reply stays
        // neutral and the confirmation page is honest.
        Ok(Registered | AlreadyRegistered) => (
            StatusCode::OK,
            workshop_views::show_tell_registered(&event.title, &detail_href, &ics_url, auth),
        )
            .into_response(),
        // The in-memory index served a published event (we 404 unknown/draft
        // slugs above), yet the store found no published row — the markdown
        // index and the DB have diverged (row hard-deleted or draft flipped
        // after boot). Nothing was stored, so never paint a confirmation:
        // surface the same generic 500 as a write failure and warn (event +
        // outcome only, never the registrant — trust boundary).
        Ok(EventNotFound) => {
            tracing::warn!(
                event = %event.public_slug,
                "event registration: index/DB divergence, no published row"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong saving your registration — please try again.",
            )
                .into_response()
        }
        Err(e) => {
            // A DbErr means *nothing was stored* — never paint a "you're
            // registered" confirmation over a failed write. Surface a generic
            // 500 so the visitor can retry. Instrument the event + outcome
            // only, never the registrant (trust boundary).
            tracing::warn!(error = %e, event = %event.public_slug, "event registration failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong saving your registration — please try again.",
            )
                .into_response()
        }
    }
}

async fn nebula_show_tell_ics(
    State(events): State<EventIndex>,
    AxumPath(slug): AxumPath<String>,
) -> impl IntoResponse {
    match events.get_public(&slug) {
        Some(event) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/calendar; charset=utf-8")],
            event.ics(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn nebula_material_step(
    State(workshops): State<WorkshopIndex>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath((category, slug, step)): AxumPath<(String, String, usize)>,
) -> impl IntoResponse {
    let not_found = || (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    let Some(m) = workshops.find_in_category(&category, &slug) else {
        return not_found();
    };
    // Steps are 1-based; index 0 and any number past the last section
    // are out of range.
    let Some(section) = step.checked_sub(1).and_then(|i| m.sections.get(i)) else {
        return not_found();
    };
    let steps = step_summaries(m);
    let material_base = nebula_material_base(&m.category);
    (
        StatusCode::OK,
        workshop_views::step(
            &workshop_views::WorkshopStep {
                base: &material_base,
                slug: &m.slug,
                workshop_title: &m.title,
                title: &section.title,
                body_html: &section.body_html,
                notes_html: &section.notes_html,
                number: step,
                total: m.sections.len(),
                steps: &steps,
            },
            auth,
        ),
    )
        .into_response()
}

/// Dedicated double-submit CSRF cookie for the workshop certificate form.
/// Distinct from `ACCOUNT_CSRF_COOKIE_NAME` (password reset / email-confirm)
/// so opening a workshop light table never clobbers an in-flight account
/// recovery in another tab, and vice versa.
const WORKSHOP_CERT_CSRF_COOKIE_NAME: &str = "navigator_workshop_cert_csrf";

/// Whether cookies should carry the `Secure` flag — true when the OAuth
/// redirect URI is HTTPS (prod), false in plain-HTTP local dev. Mirrors
/// how `AuthState::secure_cookies` is derived, for handlers that hold the
/// full `AppState` instead of an `AuthState`.
fn secure_cookies(app: &AppState) -> bool {
    app.oauth
        .as_ref()
        .is_some_and(|o| o.redirect_uri().starts_with("https://"))
}

/// The light-table grid for one workshop: every slide as a thumbnail.
/// Mints a double-submit CSRF token for the certificate form embedded on
/// the page (revealed client-side once every slide has been viewed).
async fn nebula_slides(
    State(app): State<AppState>,
    MaybeAuth(auth): MaybeAuth,
    cookies: tower_cookies::Cookies,
    AxumPath((category, slug)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    let Some(m) = app.workshops.find_in_category(&category, &slug) else {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    };
    let thumbs: Vec<workshop_views::SlideThumb<'_>> = m
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| workshop_views::SlideThumb {
            number: i + 1,
            title: &s.title,
            body_html: &s.body_html,
        })
        .collect();
    let csrf = crate::password_reset::mint_csrf_with(
        &app.sessions,
        secure_cookies(&app),
        &cookies,
        WORKSHOP_CERT_CSRF_COOKIE_NAME,
    );
    let material_base = nebula_material_base(&m.category);
    (
        StatusCode::OK,
        workshop_views::slides(
            &workshop_views::LightTable {
                base: &material_base,
                slug: &m.slug,
                workshop_title: &m.title,
                slides: &thumbs,
                csrf_token: &csrf,
            },
            auth,
        ),
    )
        .into_response()
}

/// Form body for the certificate request (`POST …/{slug}/certificate`).
#[derive(serde::Deserialize)]
struct CertificateForm {
    name: String,
    email: String,
    #[serde(default)]
    csrf_token: String,
}

/// `POST …/{slug}/certificate` — a student who has worked through every
/// slide asks for their completion certificate. Validates the
/// double-submit CSRF token, then dispatches the durable
/// `workshop__certificate` workflow (which renders the PDF and emails it
/// from the Foundation address). Completion is client-trusted
/// (localStorage, no telemetry), so this endpoint can't verify the slides
/// were actually viewed — it's an educational courtesy, not a credential.
async fn nebula_certificate_submit(
    State(app): State<AppState>,
    MaybeAuth(auth): MaybeAuth,
    cookies: tower_cookies::Cookies,
    AxumPath((category, slug)): AxumPath<(String, String)>,
    axum::extract::Form(form): axum::extract::Form<CertificateForm>,
) -> impl IntoResponse {
    let Some(m) = app.workshops.find_in_category(&category, &slug) else {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    };
    if !crate::password_reset::verify_csrf_with(
        &app.sessions,
        &cookies,
        &form.csrf_token,
        WORKSHOP_CERT_CSRF_COOKIE_NAME,
    ) {
        return (StatusCode::BAD_REQUEST, "invalid or missing CSRF token").into_response();
    }
    cookies.add(crate::oauth::expired_cookie(WORKSHOP_CERT_CSRF_COOKIE_NAME));

    let name = form.name.trim();
    let email = form.email.trim();
    // Server-side bounds mirror the form's maxlength, so a client that
    // bypasses the HTML constraint can't feed a multi-megabyte name into
    // the Typst renderer or an oversized address to SendGrid.
    if name.is_empty() || name.len() > 120 || !email.contains('@') || email.len() > 254 {
        return (
            StatusCode::BAD_REQUEST,
            "Please enter your name and a valid email address.",
        )
            .into_response();
    }

    // The issue date is stamped here (web), so it rides the Restate signal
    // value and a replay reuses it deterministically — the worker never
    // reads the clock.
    let issued = chrono::Utc::now().format("%B %-d, %Y").to_string();
    // A fresh key per request: each certificate is its own ephemeral
    // workflow invocation.
    let key = uuid::Uuid::new_v4();
    let runtime = app.workflow_runtime.clone();
    if let Err(e) = workflows::email::certificate::trigger_certificate(
        runtime.as_ref(),
        key,
        name,
        email,
        &m.title,
        &issued,
    )
    .await
    {
        // Logged, never surfaced — the reply is the same neutral page so
        // the endpoint isn't an address-enumeration oracle. Instrument the
        // workshop + outcome only, never the recipient (trust boundary).
        tracing::warn!(error = %e, workshop = %m.slug, "certificate dispatch failed");
    }
    let material_base = nebula_material_base(&m.category);
    (
        StatusCode::OK,
        workshop_views::certificate_sent(&m.title, &format!("{material_base}/{}", m.slug), auth),
    )
        .into_response()
}

/// `GET /version` — report the release of the build that is actually
/// running, so an operator/CI/AIDA/browser can confirm which release prod
/// is on without shelling into a (shell-less) distroless pod.
///
/// The headline field is `release`: the `YY.MM.DD` ghcr tag the daily
/// `deploy.yml` published, baked into the image as `NAVIGATOR_RELEASE_TAG`.
/// Under the ghcr model an image is pulled by that dated tag, so `release`
/// is what a `ship` rolls onto and what an operator pins — it is the
/// deploy's identity. The git fields stay alongside it for traceability:
/// `images/Dockerfile.web` turns the `GIT_SHA`/`BUILD_TIME` build-args
/// (set by CI to the released commit) into `NAVIGATOR_GIT_SHA` /
/// `NAVIGATOR_BUILD_TIME`. All three are baked into the image bytes, so
/// they cannot drift from what was deployed. A local `cargo run` honestly
/// reports `"unknown"` (no env var, no build-arg).
///
/// Public, unauthenticated, exempt from the private-mode gate — it is an
/// ops/health-class endpoint like `/health` and `/readyz`.
async fn version() -> impl IntoResponse {
    let release = std::env::var("NAVIGATOR_RELEASE_TAG").unwrap_or_else(|_| "unknown".into());
    let commit_full = std::env::var("NAVIGATOR_GIT_SHA").unwrap_or_else(|_| "unknown".into());
    // The short SHA is the load-bearing field. Derive it from the full
    // one (first 7 chars) so the two can never disagree.
    let commit = if commit_full == "unknown" {
        "unknown".to_string()
    } else {
        commit_full.chars().take(7).collect()
    };
    let built = std::env::var("NAVIGATOR_BUILD_TIME").unwrap_or_else(|_| "unknown".into());
    axum::Json(serde_json::json!({
        "release": release,
        "commit": commit,
        "commit_full": commit_full,
        "built": built,
        "crate_version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn health(State(db): State<Db>) -> impl IntoResponse {
    match store::ping(&db).await {
        Ok(()) => (
            StatusCode::OK,
            "ok\nNothing here is legal advice without a signed retainer.",
        ),
        Err(e) => {
            tracing::warn!(error = %e, "health: db ping failed");
            (StatusCode::SERVICE_UNAVAILABLE, "db unavailable")
        }
    }
}

/// Readiness probe: the pod is ready to take traffic only if every
/// downstream the request path needs is reachable. Wire this to the
/// Kubernetes `readinessProbe` so a pod with a flapping OPA sidecar
/// or a stalled DB gets removed from the service endpoints; keep
/// `/health` (DB-only) on the `livenessProbe` so the kubelet doesn't
/// kill an otherwise-healthy pod because an external dependency
/// twitched.
async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let mut failures: Vec<String> = Vec::new();
    if let Err(e) = store::ping(&state.db).await {
        failures.push(format!("db: {e}"));
    }
    if let Err(e) = state.policy.probe_health().await {
        failures.push(e);
    }
    if failures.is_empty() {
        (StatusCode::OK, "ready").into_response()
    } else {
        let body = failures.join("\n");
        tracing::warn!(reasons = %body, "readyz: degraded");
        (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
    }
}

/// Router-level fallback for paths that no other handler matched.
/// HTML clients (browsers, anything not under `/api` or `/mcp`) get
/// the styled 404 page; API/JSON-RPC clients get a tiny JSON body.
async fn fallback_not_found(req: axum::extract::Request) -> impl IntoResponse {
    let path = req.uri().path();
    if wants_json(path) {
        (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "not_found" })),
        )
            .into_response()
    } else {
        (StatusCode::NOT_FOUND, views::not_found_page()).into_response()
    }
}

/// `true` when the request should get a machine-readable error body
/// rather than the HTML chrome. The two non-HTML surfaces this server
/// hosts are `/api/*` (JSON listings + Swagger meta) and `/mcp` (MCP
/// JSON-RPC). Everything else — including `/portal/*` HTML pages and
/// the `/auth/*` flows — gets the styled error page.
#[must_use]
pub fn wants_json(path: &str) -> bool {
    path.starts_with("/api/")
        || path == "/api"
        || path.starts_with("/mcp/")
        || path == "/mcp"
        || path == "/openapi.json"
}

#[cfg(test)]
mod version_tests {
    use super::version;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// `GET /version` answers 200 with a JSON body, reflects the baked
    /// `NAVIGATOR_RELEASE_TAG` + `NAVIGATOR_GIT_SHA` when present (short =
    /// first 7 of the full SHA), and falls back to `"unknown"` when they
    /// are unset. The handler needs no `State`, so a one-route router
    /// exercises the real route wiring without standing up a DB. Both
    /// cases run in one test, sequentially, because the env var is
    /// process-global.
    #[tokio::test]
    async fn version_reports_baked_sha_or_unknown() {
        async fn get_version_json() -> serde_json::Value {
            let app = Router::new().route("/version", get(version));
            let resp = app
                .oneshot(
                    axum::http::Request::builder()
                        .uri("/version")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), axum::http::StatusCode::OK);
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        }

        // SAFETY: single-threaded within this test; no other code reads
        // NAVIGATOR_GIT_SHA, so there is no concurrent reader to race.
        std::env::set_var("NAVIGATOR_RELEASE_TAG", "26.06.23");
        std::env::set_var(
            "NAVIGATOR_GIT_SHA",
            "ef143cba1fdd299c0f57f99eddb7806df5464b68",
        );
        std::env::set_var("NAVIGATOR_BUILD_TIME", "2026-06-11T17:01:25-07:00");
        let v = get_version_json().await;
        assert_eq!(v["release"], "26.06.23");
        assert_eq!(v["commit"], "ef143cb");
        assert_eq!(v["commit_full"], "ef143cba1fdd299c0f57f99eddb7806df5464b68");
        assert_eq!(v["built"], "2026-06-11T17:01:25-07:00");
        assert!(v["crate_version"].is_string());

        std::env::remove_var("NAVIGATOR_RELEASE_TAG");
        std::env::remove_var("NAVIGATOR_GIT_SHA");
        std::env::remove_var("NAVIGATOR_BUILD_TIME");
        let v = get_version_json().await;
        assert_eq!(v["release"], "unknown");
        assert_eq!(v["commit"], "unknown");
        assert_eq!(v["commit_full"], "unknown");
        assert_eq!(v["built"], "unknown");
    }
}

#[cfg(test)]
mod csp_tests {
    use super::{csp_img_origin_from, csp_value};

    /// An absolute `https`/`http` asset base contributes its
    /// `scheme://host` origin (the bucket sub-path is dropped — a CSP
    /// host-source is an origin, not a path). A relative base (the
    /// `/public` default) or junk contributes nothing, since `'self'`
    /// already covers same-origin photos.
    #[test]
    fn img_origin_is_the_scheme_and_host_only() {
        assert_eq!(
            csp_img_origin_from("https://storage.googleapis.com/my-proj-assets"),
            Some("https://storage.googleapis.com".to_string()),
        );
        assert_eq!(
            csp_img_origin_from("https://cdn.example.com"),
            Some("https://cdn.example.com".to_string()),
        );
        assert_eq!(
            csp_img_origin_from("  http://localhost:8080/assets/  "),
            Some("http://localhost:8080".to_string()),
        );
        assert_eq!(csp_img_origin_from("/public"), None);
        assert_eq!(csp_img_origin_from(""), None);
        assert_eq!(csp_img_origin_from("https://"), None);
    }

    /// With no `NAVIGATOR_ASSET_BASE_URL` the CSP stays same-origin —
    /// `img-src 'self' data:` with no extra host, and scripts/styles
    /// never leave `'self'`. Setting it to a bucket widens `img-src`
    /// only. The env var is process-global, so this test owns it.
    #[test]
    fn csp_value_widens_img_src_only_for_the_asset_host() {
        // SAFETY: single-threaded within this test; the only readers of
        // NAVIGATOR_ASSET_BASE_URL are these helpers, run sequentially here.
        std::env::remove_var("NAVIGATOR_ASSET_BASE_URL");
        let csp = csp_value();
        let csp = csp.to_str().unwrap().to_string();
        assert!(csp.contains("img-src 'self' data:;"), "got: {csp}");
        assert!(csp.contains("script-src 'self'"), "got: {csp}");
        assert!(!csp.contains("googleapis"), "got: {csp}");

        std::env::set_var(
            "NAVIGATOR_ASSET_BASE_URL",
            "https://storage.googleapis.com/my-proj-assets",
        );
        let csp = csp_value();
        let csp = csp.to_str().unwrap().to_string();
        assert!(
            csp.contains("img-src 'self' data: https://storage.googleapis.com;"),
            "asset host must widen img-src only: {csp}",
        );
        // Code never leaves the origin even when photos do.
        assert!(csp.contains("script-src 'self'"), "got: {csp}");
        assert!(!csp.contains("script-src 'self' https"), "got: {csp}");
        std::env::remove_var("NAVIGATOR_ASSET_BASE_URL");
    }
}

#[cfg(test)]
mod nebula_landing_tests {
    use super::{landing_show_tell_preview, Event, EventIndex};
    use chrono::NaiveDate;

    fn event_on(slug: &str, date: NaiveDate) -> Event {
        Event {
            slug: slug.into(),
            public_slug: slug.into(),
            date,
            title: slug.into(),
            description: String::new(),
            body_html: String::new(),
            starts_at: date.and_hms_opt(18, 0, 0).unwrap(),
            ends_at: date.and_hms_opt(20, 0, 0).unwrap(),
            timezone: "America/Los_Angeles".into(),
            location_name: "Room".into(),
            location_address: "City".into(),
            meeting_url: None,
            draft: false,
            image_url: None,
            image_alt: None,
            video_url: None,
            recap_url: None,
        }
    }

    #[test]
    fn landing_preview_takes_three_upcoming_and_one_recent_past() {
        // Deliberately out of order; the preview must impose its own.
        let ix = EventIndex::new(vec![
            event_on("p-old", NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
            event_on("u4", NaiveDate::from_ymd_opt(2026, 7, 26).unwrap()),
            event_on("p-recent", NaiveDate::from_ymd_opt(2026, 6, 20).unwrap()),
            event_on("u1", NaiveDate::from_ymd_opt(2026, 7, 5).unwrap()),
            event_on("u3", NaiveDate::from_ymd_opt(2026, 7, 19).unwrap()),
            event_on("u2", NaiveDate::from_ymd_opt(2026, 7, 12).unwrap()),
        ]);
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let preview: Vec<_> = landing_show_tell_preview(&ix, today)
            .iter()
            .map(|e| e.slug.clone())
            .collect();
        // Soonest three upcoming (nearest first), then the single most recent
        // past — never the older one, never a fourth upcoming.
        assert_eq!(preview, vec!["u1", "u2", "u3", "p-recent"]);
    }

    #[test]
    fn landing_preview_with_no_past_is_upcoming_only() {
        let ix = EventIndex::new(vec![
            event_on("u1", NaiveDate::from_ymd_opt(2026, 7, 5).unwrap()),
            event_on("u2", NaiveDate::from_ymd_opt(2026, 7, 12).unwrap()),
        ]);
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let preview: Vec<_> = landing_show_tell_preview(&ix, today)
            .iter()
            .map(|e| e.slug.clone())
            .collect();
        assert_eq!(preview, vec!["u1", "u2"]);
    }
}
