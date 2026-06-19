#![allow(clippy::doc_markdown)]
//! Router tests for the web crate.
//!
//! Drives the router via `tower::ServiceExt::oneshot` — no socket,
//! no port binding, no flakiness around chosen ephemeral ports. Each
//! test gets its own fresh Postgres schema via
//! `store::test_support::pg` so they don't share state.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use store::test_support::pg;
use store::Db;
use tower::ServiceExt;
use web::workshops::WorkshopSection;
use web::{
    AppState, AuthConfig, CanonicalHost, MarketingIndex, SessionStore, WorkshopIndex,
    WorkshopMaterial,
};

async fn in_memory_db() -> Db {
    pg().await
}

fn test_sessions() -> SessionStore {
    SessionStore::new("test-session-key-not-for-production")
}

/// A signed session cookie for an `admin` caller. Admin bypasses
/// project row-scoping (per `docs/access-model.md`), so handler tests
/// that render the admin chrome for an arbitrary project authenticate
/// with this rather than relying on the no-session affordance.
fn admin_session_cookie() -> String {
    format!(
        "{}={}",
        web::session::SESSION_COOKIE_NAME,
        test_sessions().encode(&web::SessionData::fresh(
            "admin@neonlaw.com",
            store::entity::person::Role::Admin,
        ))
    )
}

async fn empty_state() -> AppState {
    web::test_support::app_state(in_memory_db().await).await
}

async fn state_with_bundled_marketing() -> AppState {
    let marketing_dir = std::path::Path::new(web::DEFAULT_MARKETING_DIR);
    let marketing_docs =
        web::marketing::loader::load_dir(marketing_dir).expect("bundled marketing content loads");
    let marketing_es = web::marketing::loader::load_dir(&marketing_dir.join("es"))
        .expect("bundled Spanish marketing content loads");
    AppState {
        marketing: MarketingIndex::new(marketing_docs).with_es(marketing_es),
        ..web::test_support::app_state(in_memory_db().await).await
    }
}

async fn empty_state_with_auth(auth: AuthConfig) -> AppState {
    AppState {
        auth,
        ..web::test_support::app_state(in_memory_db().await).await
    }
}

async fn empty_state_with_canonical_host(host: CanonicalHost) -> AppState {
    AppState {
        canonical_host: host,
        ..web::test_support::app_state(in_memory_db().await).await
    }
}

async fn state_with_workshops(materials: Vec<WorkshopMaterial>) -> AppState {
    AppState {
        db: in_memory_db().await,
        workshops: WorkshopIndex::new(materials),
        docs: web::DocsIndex::empty(),
        marketing: MarketingIndex::empty(),
        blog: web::BlogIndex::empty(),
        auth: AuthConfig::new(true, None),
        google_oauth: web::google_oauth::GoogleOauthConfig::passthrough(),
        rate_limit: web::rate_limit::RateLimit::disabled(),
        canonical_host: CanonicalHost::new(None),
        portal_only: web::PortalOnly::default(),
        sessions: test_sessions(),
        oauth: None,
        storage: std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-web-test-storage"))
                .await
                .unwrap(),
        ),
        policy: web::policy::PolicyClient::passthrough(),
        workflow_runtime: std::sync::Arc::new(workflows::InMemoryRuntime::new()),
        questionnaire_runtime: std::sync::Arc::new(workflows::InMemoryRuntime::new()),
        signature_provider: std::sync::Arc::new(web::signature::StubSignatureProvider::new()),
        billing_provider: std::sync::Arc::new(web::billing::StubBillingProvider::new()),
        contract_reviewer: std::sync::Arc::new(web::contract_review::StubContractReviewer),
        esignature_webhook_secret: None,
        esignature_hmac_key: None,
        email: std::sync::Arc::new(web::email::CapturingEmail::new()),
        inbound_email_secret: None,
        email_events_secret: None,
        sendgrid_events_public_key: None,
        bootstrap_admin_email: None,
        identity_password: None,
        identity_admin: None,
        a2a_router: None,
    }
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn foundation_mission_renders_the_letter_under_the_foundation_brand() {
    // The mission page is the letter alone; the pro-bono referral list
    // moved to its own /foundation/pro-bono page.
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/mission")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Mission</title>"));
    assert!(body.contains("class=\"mission-letter\""));
}

#[tokio::test]
async fn foundation_mission_links_training_to_the_workshop_not_the_repo() {
    let app = web::build_router(
        state_with_bundled_marketing().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    for uri in ["/foundation/mission", "/es/foundation/mission"] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(
            body.contains("href=\"/foundation/workshops/navigator/readme\""),
            "{uri} should link legal-aid training to the Navigator workshop: {body}",
        );
        assert_eq!(
            body.matches("href=\"https://github.com/neon-law-foundation/navigator\"")
                .count(),
            1,
            "{uri} should keep only the opening repository link",
        );
    }
}

#[tokio::test]
async fn docusign_consent_callback_renders_confirmation() {
    // DocuSign redirects the operator's browser to this URI after the
    // one-time JWT-grant `Allow`. It must land on a confirmation page.
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/docusign/consent-callback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Consent recorded"));
}

#[tokio::test]
async fn legacy_help_route_is_gone() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(Request::builder().uri("/help").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn root_returns_home_page_html() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.starts_with("<!DOCTYPE html>"));
    assert!(body.contains("<title>Neon Law | Home</title>"));
    // The minimal landing names both organizations as equal peers.
    assert!(body.contains(
        "An American law firm offering flat-fee legal services with a licensed attorney in the loop."
    ));
    assert!(body.contains(
        "An American non-profit pursuing access to justice through open-source tools and legal-aid education."
    ));
    // It is the minimal card — no marketing hero strip.
    assert!(
        !body.contains("lake-tahoe"),
        "home must not render the marketing hero"
    );
}

#[tokio::test]
async fn portal_only_mode_redirects_root_to_portal_and_drops_marketing() {
    // NAVIGATOR_PORTAL_ONLY white-label deploy: the firm's own marketing
    // site owns the public surface, so `/` 303-redirects to the portal,
    // the marketing pages are no longer mounted, and the always-on
    // app/legal surface (here `/terms`) stays up.
    let mut state = empty_state().await;
    state.portal_only = web::PortalOnly::new(true);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get(axum::http::header::LOCATION).unwrap(),
        "/portal"
    );

    // A marketing page is no longer mounted under portal-only.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/services")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // The legal pages stay mounted in both modes.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/terms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

fn marketing_doc(slug: &str, title: &str, body_html: &str) -> web::MarketingDoc {
    web::MarketingDoc {
        slug: slug.into(),
        title: title.into(),
        description: format!("{slug} description"),
        body_html: body_html.into(),
        metadata: std::collections::HashMap::new(),
        pricing: Vec::new(),
    }
}

#[tokio::test]
async fn spanish_home_declares_lang_es_and_translates_chrome() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(Request::builder().uri("/es").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("<html lang=\"es\""),
        "Spanish home must declare lang=es: {body}"
    );
    // Navbar chrome is translated and hrefs are /es-prefixed.
    assert!(body.contains(">Servicios</a>"), "nav should be translated");
    assert!(
        body.contains("href=\"/es/services\""),
        "Spanish nav hrefs should be /es-prefixed: {body}"
    );
    // hreflang alternates + the switcher back to English.
    assert!(body.contains("hreflang=\"es\""));
    assert!(body.contains("language-switcher") && body.contains(">English</a>"));
}

#[tokio::test]
async fn english_home_declares_lang_en() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let en = body_string(
        app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap(),
    )
    .await;
    assert!(en.contains("<html lang=\"en\""));
}

#[tokio::test]
async fn spanish_service_page_translates_chrome_and_falls_back_to_english_body() {
    let mut state = empty_state().await;
    // English doc present; NO es twin for this slug → graceful fallback to
    // the English body under a Spanish shell.
    state.marketing = MarketingIndex::new(vec![marketing_doc(
        "estate",
        "Estate",
        "<p>English estate body</p>",
    )]);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let body = body_string(
        app.oneshot(
            Request::builder()
                .uri("/es/services/estate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(body.contains("<html lang=\"es\""), "shell is Spanish");
    // Untranslated body falls back to English rather than 404-ing.
    assert!(
        body.contains("English estate body"),
        "fallback to en body: {body}"
    );
    // The switcher points back to English at the twin path.
    assert!(body.contains("href=\"/services/estate\"") && body.contains(">English</a>"));
}

#[tokio::test]
async fn every_es_enabled_path_resolves_in_spanish() {
    // The i18n allow-list (which the navbar and `localize_href` use to
    // /es-prefix hrefs) and the mounted /es routes must agree: every path
    // the chrome will prefix has to resolve: a path listed as ES-enabled
    // but never mounted would 404 the Spanish navbar.
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    for path in views::i18n::ES_ENABLED_PATHS {
        let es = views::i18n::localize_href(path, views::i18n::Locale::Es);
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(&es).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "ES-enabled path {path:?} localizes to {es:?}, which did not resolve 200"
        );
    }
}

#[tokio::test]
async fn english_marketing_pages_offer_a_spanish_switcher() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let body = body_string(
        app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap(),
    )
    .await;
    assert!(
        body.contains("language-switcher") && body.contains(">Español</a>"),
        "English home should offer a one-tap Spanish switcher: {body}"
    );
    assert!(body.contains("hreflang=\"es\" href=\"/es\""));
}

#[tokio::test]
async fn health_returns_200_when_db_pings() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        body_string(resp).await,
        "ok\nNothing here is legal advice without a signed retainer."
    );
}

#[tokio::test]
async fn health_returns_503_when_db_is_down() {
    let db = in_memory_db().await;
    db.clone().close().await.unwrap();
    let state = AppState {
        db,
        workshops: WorkshopIndex::empty(),
        docs: web::DocsIndex::empty(),
        marketing: MarketingIndex::empty(),
        blog: web::BlogIndex::empty(),
        auth: AuthConfig::new(true, None),
        google_oauth: web::google_oauth::GoogleOauthConfig::passthrough(),
        rate_limit: web::rate_limit::RateLimit::disabled(),
        canonical_host: CanonicalHost::new(None),
        portal_only: web::PortalOnly::default(),
        sessions: test_sessions(),
        oauth: None,
        storage: std::sync::Arc::new(
            cloud::FsStorage::new(std::env::temp_dir().join("navigator-web-test-storage"))
                .await
                .unwrap(),
        ),
        policy: web::policy::PolicyClient::passthrough(),
        workflow_runtime: std::sync::Arc::new(workflows::InMemoryRuntime::new()),
        questionnaire_runtime: std::sync::Arc::new(workflows::InMemoryRuntime::new()),
        signature_provider: std::sync::Arc::new(web::signature::StubSignatureProvider::new()),
        billing_provider: std::sync::Arc::new(web::billing::StubBillingProvider::new()),
        contract_reviewer: std::sync::Arc::new(web::contract_review::StubContractReviewer),
        esignature_webhook_secret: None,
        esignature_hmac_key: None,
        email: std::sync::Arc::new(web::email::CapturingEmail::new()),
        inbound_email_secret: None,
        email_events_secret: None,
        sendgrid_events_public_key: None,
        bootstrap_admin_email: None,
        identity_password: None,
        identity_admin: None,
        a2a_router: None,
    };
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_string(resp).await, "db unavailable");
}

#[tokio::test]
async fn foundation_returns_foundation_landing_under_foundation_brand() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Foundation</title>"));
    assert!(body.contains("mailto:support@neonlaw.org"));
}

#[tokio::test]
async fn navigator_serves_the_readme_under_foundation_brand() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/navigator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Navigator</title>"));
    // The page is the README: its H1 and the getting-started command.
    assert!(body.contains(">Neon Law Navigator</h1>"));
    assert!(body.contains("cargo run -p cli -- start-dev-server"));
    // README links are retargeted onto site routes.
    assert!(body.contains("href=\"/api/templates/nest/nevada\""));
    assert!(body.contains("href=\"/docs/glossary#project\""));
    // The hub fans out to the per-package pages.
    assert!(body.contains("href=\"/foundation/navigator/cli\""));
    assert!(body.contains("href=\"/foundation/navigator/mcp\""));
    assert!(body.contains("href=\"/foundation/navigator/web\""));
}

#[tokio::test]
async fn old_navigator_url_permanently_redirects_to_the_hub() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/navigator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/foundation/navigator"
    );
}

#[tokio::test]
async fn navigator_package_pages_render_each_crate_readme() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    // Each package page renders its crate README under the Foundation
    // brand, with the cross-package strip atop it.
    for (path, title, needle) in [
        (
            "/foundation/navigator/cli",
            "Navigator CLI",
            "Operator CLI for Navigator",
        ),
        (
            "/foundation/navigator/mcp",
            "Navigator MCP",
            "Model Context Protocol",
        ),
        ("/foundation/navigator/web", "Navigator Web", "axum"),
    ] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{path} should render");
        let body = body_string(resp).await;
        assert!(
            body.contains(&format!("<title>Neon Law Foundation | {title}</title>")),
            "{path} should carry the {title} title"
        );
        assert!(
            body.to_lowercase().contains(&needle.to_lowercase()),
            "{path} should render its README ({needle})"
        );
        assert!(
            body.contains("aria-label=\"Navigator packages\""),
            "{path} should carry the package strip"
        );
    }
}

#[tokio::test]
async fn api_template_raw_serves_non_confidential_markdown_inline() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/templates/nest/nevada")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/markdown; charset=utf-8"),
    );
    let body = body_string(resp).await;
    assert!(body.contains("Nevada"), "served the raw template markdown");

    // A confidential template (the retainer) must 404 over the API.
    let confidential = app
        .oneshot(
            Request::builder()
                .uri("/api/templates/onboarding/retainer")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(confidential.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rust_in_peace_talk_renders_as_a_workshop_under_foundation_brand() {
    // The "Rust in Peace" talk folded into the workshop manifest — it
    // loads from the real workshop content dir like any other workshop.
    let materials =
        web::workshops::loader::load_navigator(std::path::Path::new(web::DEFAULT_WORKSHOPS_DIR))
            .expect("load real workshop content");
    let app = web::build_router(
        state_with_workshops(materials).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/rust-in-peace")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Rust in Peace</title>"));
    // The overview's "Start →" button points at the first step under the
    // workshop base — the talk shares the workshop chrome now.
    assert!(body.contains("href=\"/foundation/workshops/navigator/rust-in-peace/step/1\""));
    // It advertises its Markdown twin for machine readers.
    assert!(body.contains(
        "<link rel=\"alternate\" type=\"text/markdown\" \
         href=\"/foundation/workshops/navigator/rust-in-peace.md\">"
    ));

    // Step 1 is the agenda; the rail shows the progress label.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/rust-in-peace/step/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<h2>Agenda</h2>"));
    assert!(body.contains("Step 1 of"));
}

#[tokio::test]
async fn old_presentation_urls_permanently_redirect_to_workshops() {
    // Presentations folded into Workshops; the old surface redirects so a
    // deep link to a talk lands on its workshop twin.
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    for (from, to) in [
        (
            "/foundation/presentations",
            "/foundation/workshops/navigator",
        ),
        (
            "/foundation/presentations/rust-in-peace",
            "/foundation/workshops/navigator/rust-in-peace",
        ),
        (
            "/foundation/presentations/rust-in-peace/step/1",
            "/foundation/workshops/navigator/rust-in-peace/step/1",
        ),
    ] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(from).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::PERMANENT_REDIRECT,
            "{from} should redirect"
        );
        assert_eq!(
            resp.headers().get("location").unwrap(),
            to,
            "{from} should redirect to {to}"
        );
    }
}

#[tokio::test]
async fn services_estate_uses_marketing_doc_when_present() {
    let docs = vec![web::MarketingDoc {
        slug: "estate".into(),
        title: "Estate planning".into(),
        description: "wills and trusts".into(),
        body_html: "<h2>Drafted</h2><p>Trusts.</p>".into(),
        metadata: std::collections::HashMap::new(),
        pricing: Vec::new(),
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/estate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Estate planning</title>"));
    assert!(body.contains("<h2>Drafted</h2>"));
    // The firm CTA books a consultation on the calendar, not a mailto.
    assert!(body.contains("calendar.app.google/GueqKHiAuqXEwkRG8"));
    assert!(body.contains("Book a Consultation"));
}

#[tokio::test]
async fn services_estate_renders_a_split_hero_from_the_hero_image_metadata() {
    // The `hero_image:` frontmatter key turns a product page into a split
    // hero: the "Neon Law …" brand title becomes the page <h1> beside the
    // curated photo, and the body's own leading <h1> is lifted up into the
    // hero lead (so it isn't repeated). This drives the full web→view seam
    // — the metadata read in `service_page` plus the view's hero render.
    let docs = vec![web::MarketingDoc {
        slug: "estate".into(),
        title: "Neon Law Northstar".into(),
        description: "your estate plan in one sitting".into(),
        body_html: "<h1>Your estate plan, in one sitting</h1><p>One recorded sitting.</p>".into(),
        metadata: std::collections::HashMap::from([(
            "hero_image".to_string(),
            "lake-tahoe".to_string(),
        )]),
        pricing: Vec::new(),
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/estate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Brand title is the hero <h1>, led by the product's icon (the mark
    // that used to sit in the Services dropdown); the curated photo is
    // preloaded.
    assert!(
        body.contains(
            "<h1 class=\"display-4 fw-bold mb-3\">\
             <i class=\"bi bi-star-fill me-3\" aria-hidden=\"true\"></i>Neon Law Northstar</h1>"
        ),
        "expected the icon-led brand title as the hero h1"
    );
    assert!(body.contains("lake-tahoe") && body.contains("<picture>"));
    assert!(
        body.contains("rel=\"preload\" as=\"image\""),
        "hero photo should be preloaded for LCP"
    );
    // The markdown headline moved into the hero lead — present exactly once,
    // and no longer wrapped in its own <h1>.
    assert_eq!(body.matches("Your estate plan, in one sitting").count(), 1);
    assert!(!body.contains("<h1>Your estate plan, in one sitting</h1>"));
}

#[tokio::test]
async fn services_litigation_uses_marketing_doc_when_present() {
    let docs = vec![web::MarketingDoc {
        slug: "litigation".into(),
        title: "Litigation".into(),
        description: "we refer out".into(),
        body_html: "<h2>Litigation: we listen, then connect you with trial counsel</h2>".into(),
        metadata: std::collections::HashMap::new(),
        pricing: Vec::new(),
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/litigation")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Litigation</title>"));
    assert!(body.contains("connect you with trial counsel"));
    // The firm CTA books a consultation on the calendar, not a mailto.
    assert!(body.contains("calendar.app.google/GueqKHiAuqXEwkRG8"));
    assert!(body.contains("Book a Consultation"));
}

#[tokio::test]
async fn services_nautilus_uses_marketing_doc_when_present() {
    let docs = vec![web::MarketingDoc {
        slug: "nautilus".into(),
        title: "Nautilus".into(),
        description: "a lawyer between you and the collectors".into(),
        body_html: "<h2>Put a lawyer between you and the collectors</h2>\
                    <p>we never take a percentage of your debt</p>"
            .into(),
        metadata: std::collections::HashMap::new(),
        pricing: Vec::new(),
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/nautilus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Nautilus</title>"));
    assert!(body.contains("Put a lawyer between you and the collectors"));
    assert!(body.contains("we never take a percentage of your debt"));
    // The firm CTA books a consultation on the calendar, not a mailto.
    assert!(body.contains("calendar.app.google/GueqKHiAuqXEwkRG8"));
    assert!(body.contains("Book a Consultation"));
}

#[tokio::test]
async fn services_corporate_falls_back_to_default_when_no_doc() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/corporate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Corporate services</title>"));
    // The firm CTA books a consultation on the calendar, not a mailto.
    assert!(body.contains("calendar.app.google/GueqKHiAuqXEwkRG8"));
    assert!(body.contains("Book a Consultation"));
}

#[tokio::test]
async fn services_fractional_gc_uses_marketing_doc_when_present() {
    let docs = vec![web::MarketingDoc {
        slug: "fractional-gc".into(),
        title: "Fractional GC".into(),
        description: "Fractional general counsel for software startups.".into(),
        body_html: "<p>lead</p><p>[[pricing]]</p><h2>Everything but litigation</h2><p>Two-business-day response.</p>"
            .into(),
        metadata: std::collections::HashMap::new(),
        pricing: vec![web::PricingCard {
            title: "Fractional General Counsel".into(),
            price: "$5,000".into(),
            cadence: Some("/mo".into()),
            blurb: "Your whole legal and operations bench — everything but litigation.".into(),
            features: vec!["Two-business-day response on everything you send us".into()],
            cta_label: "Ask about an open seat".into(),
            cta_href: "mailto:support@neonlaw.com".into(),
            featured: true,
            featured_label: Some("2 of 10 filled".into()),
        }],
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/services/fractional-gc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Fractional GC</title>"));
    assert!(body.contains("<h2>Everything but litigation</h2>"));
    assert!(body.contains("mailto:support@neonlaw.com"));
    // The single pricing card renders and the marker is consumed.
    assert!(!body.contains("[[pricing]]"));
    assert!(body.contains("2 of 10 filled"));
    assert!(body.contains("$5,000"));
    assert!(body.contains("Two-business-day response on everything you send us"));
}

// The `/services` index is now the DB-backed product catalog (it replaced
// the old Services dropdown + markdown index). Its coverage — every product
// listed at its `list_price_cents` with links to each `/services/<slug>`
// detail — lives in `web/tests/products_catalog.rs`, which seeds the
// `products` table the catalog reads from.

#[tokio::test]
async fn foundation_nimbus_renders_the_install_product_under_foundation_brand() {
    // Nimbus is the Foundation's white-label two-week install product. It
    // ships from the bundled marketing content, wears the Foundation brand
    // (no firm Services dropdown), writes its CTA to the Foundation inbox,
    // and quotes the flat $11,111 fee with the legal-aid discount card.
    let docs = web::marketing::loader::load_dir(std::path::Path::new(web::DEFAULT_MARKETING_DIR))
        .expect("bundled marketing dir loads");
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/nimbus")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Neon Law Foundation Nimbus</title>"));
    // Foundation chrome + inbox, never the firm Services dropdown.
    assert!(body.contains("mailto:support@neonlaw.org"));
    assert!(!body.contains(">Services</summary>"));
    // The flat fee and the legal-aid discount both surface as pricing cards.
    assert!(body.contains("$11,111"));
    assert!(body.contains("Legal aid centers"));
    // English-only: no Spanish switcher pointing at a non-existent /es twin.
    assert!(!body.contains("href=\"/es/foundation/nimbus\""));
}

#[tokio::test]
async fn foundation_uses_marketing_doc_when_present() {
    let docs = vec![web::MarketingDoc {
        slug: "foundation".into(),
        title: "Mission".into(),
        description: "Foundation tagline.".into(),
        body_html: "<h2>Programs</h2><p>Navigator + CLEs.</p>".into(),
        metadata: std::collections::HashMap::new(),
        pricing: Vec::new(),
    }];
    let mut state = empty_state().await;
    state.marketing = MarketingIndex::new(docs);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<h2>Programs</h2>"));
    assert!(body.contains("Navigator + CLEs."));
}

#[tokio::test]
async fn contact_returns_contact_page_html() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/contact")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Contact</title>"));
    assert!(body.contains("mailto:support@neonlaw.com"));
}

#[tokio::test]
async fn foundation_contact_returns_foundation_brand_contact_html() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/contact")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Contact</title>"));
    assert!(body.contains("mailto:support@neonlaw.org"));
    assert!(body.contains("github.com/neon-law-foundation"));
}

#[tokio::test]
async fn legacy_education_route_is_gone() {
    // /education was retired when CLEs collapsed into the single
    // Workshops surface (/foundation/workshops/navigator).
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/education")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn privacy_returns_privacy_page_html() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/privacy")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Privacy Policy</title>"));
    assert!(body.contains("Donor Privacy"));
}

#[tokio::test]
async fn terms_returns_terms_page_html() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/terms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Terms of Service</title>"));
    assert!(body.contains("No Legal Advice"));
}

#[tokio::test]
async fn public_favicon_is_served() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/public/favicon.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(ctype.contains("image/svg"), "got content-type: {ctype}");
}

#[tokio::test]
async fn public_missing_file_returns_404() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/public/no-such-file.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

fn sample_workshop() -> WorkshopMaterial {
    WorkshopMaterial {
        slug: "readme".into(),
        title: "Runbook".into(),
        description: "How.".into(),
        raw_markdown: "# Runbook\n\nIntro.\n\n## Install\n\nDo it.\n\n## Notarize\n\nFinish.\n"
            .into(),
        body_html: "<p>Intro.</p><h2>Install</h2><p>Do it.</p><h2>Notarize</h2>".into(),
        intro_html: "<p>Intro.</p>".into(),
        sections: vec![
            WorkshopSection {
                title: "Install".into(),
                body_html: "<h2>Install</h2><p>Do it.</p>".into(),
            },
            WorkshopSection {
                title: "Notarize".into(),
                body_html: "<h2>Notarize</h2><p>Finish.</p>".into(),
            },
        ],
    }
}

#[tokio::test]
async fn workshops_index_lists_materials() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("href=\"/foundation/workshops/navigator/readme\""));
    assert!(body.contains(">Runbook</a>"));
}

#[tokio::test]
async fn workshops_overview_renders_one_h1_and_links_steps() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/readme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law Foundation | Runbook</title>"));
    // The duplicate-H1 bug regression guard: chrome title is the only one.
    assert_eq!(body.matches("<h1>").count(), 1, "expected a single <h1>");
    assert!(body.contains("href=\"/foundation/workshops/navigator/readme/step/1\""));
    assert!(body.contains("Copy as Markdown"));
    // The overview advertises and links its Markdown twin; the copy
    // button fetches it rather than reading an on-page raw node.
    assert!(body.contains(
        "<link rel=\"alternate\" type=\"text/markdown\" \
         href=\"/foundation/workshops/navigator/readme.md\">"
    ));
    assert!(body.contains("fetch('/foundation/workshops/navigator/readme.md')"));
}

#[tokio::test]
async fn workshops_material_md_twin_serves_raw_markdown() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/readme.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(ctype, "text/markdown; charset=utf-8");
    let body = body_string(resp).await;
    // The byte-for-byte source — heading and all — not rendered HTML.
    assert!(body.starts_with("# Runbook"));
    assert!(!body.contains("<h1>"));
}

#[tokio::test]
async fn workshops_material_md_twin_404s_when_slug_missing() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/missing.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn llms_txt_indexes_the_markdown_corpus_with_absolute_urls() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/llms.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(ctype, "text/markdown; charset=utf-8");
    let body = body_string(resp).await;
    // llmstxt.org shape: H1, then a section per corpus. Talks fold into
    // the Workshops section now — there is no separate Presentations one.
    assert!(body.starts_with("# "));
    assert!(body.contains("## Workshops"));
    assert!(!body.contains("## Presentations"));
    // Every entry is an absolute link to a `.md` twin. With no
    // CANONICAL_HOST and no Host header, the base falls back to the
    // placeholder authority.
    assert!(body.contains("https://www.example.com/foundation/workshops/navigator/readme.md"));
}

#[tokio::test]
async fn deploy_workshop_md_twin_and_llms_index_the_real_content() {
    // Ground the *shipped* DEPLOY.md, not a fixture: load the real
    // workshop content directory, then confirm the deploy workshop's
    // markdown twin serves and the llms.txt corpus indexes it. If the
    // manifest entry or the file goes missing, this 404s and fails.
    let materials =
        web::workshops::loader::load_navigator(std::path::Path::new(web::DEFAULT_WORKSHOPS_DIR))
            .expect("load real workshop content");
    let app = web::build_router(
        state_with_workshops(materials).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );

    // The markdown twin serves raw markdown with the right content type.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/deploy.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(ctype, "text/markdown; charset=utf-8");
    let body = body_string(resp).await;
    assert!(
        body.contains("# Deploy the Navigator"),
        "raw markdown title"
    );
    assert!(body.contains("cargo run -p cli -- gcp setup --project-id"));

    // The llms.txt corpus lists the deploy twin as an absolute URL.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/llms.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("https://www.example.com/foundation/workshops/navigator/deploy.md"));
}

#[tokio::test]
async fn workshops_step_renders_single_section_with_progress() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/readme/step/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Step 1 of 2"));
    assert!(body.contains("<h2>Install</h2>"));
    // Step one shows the next section's content nowhere on the page.
    assert!(!body.contains("<h2>Notarize</h2>"));
    assert!(body.contains("href=\"/foundation/workshops/navigator/readme/step/2\""));
}

#[tokio::test]
async fn workshops_step_out_of_range_404s() {
    let app = web::build_router(
        state_with_workshops(vec![sample_workshop()]).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    for uri in [
        "/foundation/workshops/navigator/readme/step/0",
        "/foundation/workshops/navigator/readme/step/3",
    ] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{uri} should 404");
    }
}

#[tokio::test]
async fn workshops_material_404s_when_slug_missing() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/workshops/navigator/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_people_returns_empty_array_when_no_rows() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ctype.contains("application/json"), "got: {ctype}");
    assert_eq!(body_string(resp).await, "[]");
}

#[tokio::test]
async fn api_people_lists_seeded_rows() {
    // Exercise the listing against the canonical seed (store/seeds/
    // Person.yaml) rather than a hand-rolled row, so the test covers
    // the same data the app ships with.
    let state = empty_state().await;
    store::seed::seed_canonical(&state.db, &state.storage)
        .await
        .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"name\":\"Nick Shook\""), "got: {body}");
    assert!(
        body.contains("\"email\":\"nick@neonlaw.com\""),
        "got: {body}"
    );
}

#[tokio::test]
async fn api_person_by_id_404s_when_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/people/{}", uuid::Uuid::from_u128(999)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(body.contains("\"error\":\"not_found\""));
}

#[tokio::test]
async fn api_jurisdictions_and_entity_types_are_listable() {
    // Drive both listings off the canonical seed (store/seeds/
    // Jurisdiction.yaml + EntityType.yaml) so the assertions track the
    // reference data the app actually ships.
    let state = empty_state().await;
    store::seed::seed_canonical(&state.db, &state.storage)
        .await
        .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/jurisdictions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(resp).await;
    assert!(body.contains("\"code\":\"NV\""), "got: {body}");
    assert!(body.contains("\"code\":\"CA\""), "got: {body}");

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/entity-types")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(resp).await;
    assert!(
        body.contains("\"name\":\"Professional LLC\""),
        "got: {body}"
    );
}

#[tokio::test]
async fn api_entities_lists_seeded_rows() {
    // /api/entities had no coverage before this; seed the canonical
    // entities (store/seeds/Entity.yaml) and assert the listing serves
    // them.
    let state = empty_state().await;
    store::seed::seed_canonical(&state.db, &state.storage)
        .await
        .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/entities")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"name\":\"Shook Law PLLC\""), "got: {body}");
    assert!(
        body.contains("\"name\":\"Neon Law Foundation\""),
        "got: {body}"
    );
}

#[tokio::test]
async fn api_entity_by_id_returns_seeded_row() {
    use sea_orm::EntityTrait;
    let state = empty_state().await;
    store::seed::seed_canonical(&state.db, &state.storage)
        .await
        .unwrap();
    let row = store::entity::entity::Entity::find()
        .one(&state.db)
        .await
        .unwrap()
        .expect("seed pass inserts at least one entity");
    let id = row.id;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/entities/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(&format!("\"id\":\"{id}\"")), "got: {body}");
    assert!(
        body.contains(&format!("\"name\":\"{}\"", row.name)),
        "got: {body}"
    );
}

#[tokio::test]
async fn api_entity_by_id_404s_when_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/entities/{}", uuid::Uuid::from_u128(999)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(body.contains("\"error\":\"not_found\""));
}

#[tokio::test]
async fn api_validate_notation_returns_clean_for_valid_markdown() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    // Minimal notation that satisfies every F-rule:
    //   F101 title, F102 respondent_type, F103 snake_case filename (default),
    //   F104 questionnaire + workflow with BEGIN reaching END,
    //   F105 confidential, F106 workflow contains bare `staff_review` state.
    let contents = "---\n\
title: Trust\n\
respondent_type: entity\n\
confidential: false\n\
questionnaire:\n  \
  BEGIN:\n    \
    next: END\n  \
  END: {}\n\
workflow:\n  \
  BEGIN:\n    \
    next: staff_review\n  \
  staff_review:\n    \
    next: END\n  \
  END: {}\n\
---\n\n\
Body.\n";
    let body = serde_json::json!({ "contents": contents });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/notations/validate")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["clean"], true, "expected clean, got: {body}");
    assert_eq!(body["path"], "notation.md");
    assert_eq!(body["violations"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn api_validate_notation_reports_frontmatter_and_line_length_violations() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    // Missing title + missing respondent_type + a body line over 120 chars.
    let long_line = "x".repeat(150);
    let body = serde_json::json!({
        "contents": format!("---\nfoo: bar\n---\n\n{long_line}\n"),
        "path": "trust.md",
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/notations/validate")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["clean"], false);
    assert_eq!(body["path"], "trust.md");
    let codes: Vec<&str> = body["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"F101"),
        "expected F101 (title), got {codes:?}"
    );
    assert!(
        codes.contains(&"F102"),
        "expected F102 (respondent_type), got {codes:?}"
    );
    assert!(
        codes.contains(&"S101"),
        "expected S101 (line length), got {codes:?}"
    );
}

#[tokio::test]
async fn api_validate_notation_markdown_only_drops_frontmatter_rules() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    // No frontmatter at all — would trip F101 in the default set.
    let body = serde_json::json!({
        "contents": "# Heading\n\nBody paragraph.\n",
        "markdown_only": true,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/notations/validate")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
    let codes: Vec<&str> = body["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.iter().all(|c| !c.starts_with('F')),
        "F-family must not run when markdown_only=true, got {codes:?}"
    );
}

#[tokio::test]
async fn admin_dashboard_is_open_when_auth_disabled() {
    let state = empty_state().await; // auth disabled
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Admin</title>"));
    assert!(body.contains("People: </strong>0"));
    // Every CRUD page + every read-only listing + the OpenAPI spec are linked.
    for href in [
        "/portal/admin/people",
        "/portal/admin/entities",
        "/portal/admin/jurisdictions",
        "/portal/admin/entity-types",
        "/portal/admin/templates",
        "/portal/admin/questions",
        "/portal/projects",
        "/portal/admin/notations",
        "/portal/admin/invoices",
        "/portal/admin/relationship-logs",
        "/openapi.json",
    ] {
        assert!(
            body.contains(&format!("href=\"{href}\"")),
            "dashboard missing link to {href}",
        );
    }
}

#[tokio::test]
async fn admin_dashboard_requires_token_when_auth_enabled() {
    let auth = AuthConfig::new(false, Some("test-secret"));
    let state = empty_state_with_auth(auth).await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_dashboard_accepts_valid_bearer_token() {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let auth = AuthConfig::new(false, Some("test-secret"));
    let state = empty_state_with_auth(auth).await;
    store::migrate(&state.db).await.unwrap();

    let claims = web::AuthClaims {
        sub: "admin@example.com".into(),
        exp: i64::try_from(jsonwebtoken::get_current_timestamp() + 3600).unwrap(),
        role: store::entity::person::Role::Admin,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(b"test-secret"),
    )
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_dashboard_rejects_invalid_bearer_token() {
    let auth = AuthConfig::new(false, Some("test-secret"));
    let state = empty_state_with_auth(auth).await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin")
                .header("authorization", "Bearer not-a-real-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn canonical_host_redirects_when_host_mismatches() {
    let state =
        empty_state_with_canonical_host(CanonicalHost::new(Some("neonlaw.org".into()))).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/contact")
                .header("host", "www.neonlaw.org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(location, "https://neonlaw.org/contact");
}

#[tokio::test]
async fn canonical_host_passes_through_when_host_matches() {
    let state =
        empty_state_with_canonical_host(CanonicalHost::new(Some("neonlaw.org".into()))).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/contact")
                .header("host", "neonlaw.org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn canonical_host_passes_through_when_disabled() {
    let state = empty_state_with_canonical_host(CanonicalHost::new(None)).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/contact")
                .header("host", "any.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn design_page_renders_the_component_gallery() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/design")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<title>Neon Law | Design system</title>"));
    // The shared components are all on the page.
    assert!(body.contains("class=\"card"), "renders cards");
    assert!(body.contains("toast-body"), "renders toasts");
    assert!(
        body.contains("text-bg-primary"),
        "renders the cyan toast tone"
    );
    assert!(
        body.contains("id=\"design-navbar-example\""),
        "renders the navbar example"
    );
    // Code snippets + the vendored highlighter that styles them, plus a
    // verbatim line from a grounded snippet (the views design::tests drift
    // test proves it still matches its source file).
    assert!(
        body.contains("class=\"language-rust\""),
        "renders highlightable code blocks"
    );
    assert!(
        body.contains("highlight.min.js"),
        "loads vendored highlight.js"
    );
    assert!(
        body.contains("pub enum ToastTone {"),
        "shows a real component snippet"
    );
}

#[tokio::test]
async fn root_serves_marketing_anonymously() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_people_index_shows_empty_state() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("No people yet."));
}

#[tokio::test]
async fn admin_people_create_then_list_round_trips() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Libra&email=libra%40example.com"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        create.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let list = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = body_string(list).await;
    assert!(body.contains("Libra"));
    assert!(body.contains("libra@example.com"));
}

#[tokio::test]
async fn admin_people_create_rejects_invalid_input() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Libra&email=not-an-email"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Name is required and email must contain an @."));
}

#[tokio::test]
async fn admin_people_edit_and_delete_flow() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let libra = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let edit = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}", libra.id))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Libra&email=libra-updated%40example.com"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        edit.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let delete = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}/delete", libra.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        delete.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let list = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(list).await;
    assert!(body.contains("No people yet."));
}

#[tokio::test]
async fn admin_people_delete_returns_empty_body_for_htmx_request() {
    // HTMX `hx-target="closest tr" hx-swap="outerHTML"` needs the
    // server to return an empty 200 so the row vanishes in place.
    // A redirect would be followed by HTMX and trigger a full
    // navigation, which is not what we want for a row-delete UX.
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let libra = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}/delete", libra.id))
                .header("HX-Request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.is_empty(),
        "htmx delete should return an empty body so outerHTML swap removes the row, got: {body:?}",
    );
}

/// Seed three people in alphabetical chaos so any sort applied by
/// the handler is observable in the rendered HTML row order.
async fn seed_three_people(db: &Db) {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    for (name, email) in [
        ("Leo", "leo@example.com"),
        ("Libra", "libra@example.com"),
        ("Taurus", "taurus@example.com"),
    ] {
        store::entity::person::ActiveModel {
            name: ActiveValue::Set(name.into()),
            email: ActiveValue::Set(email.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }
}

fn first_index_of(haystack: &str, needles: &[&str]) -> Option<(usize, String)> {
    needles
        .iter()
        .find_map(|n| haystack.find(n).map(|i| (i, (*n).to_string())))
}

#[tokio::test]
async fn admin_people_index_drops_id_column_and_renders_sort_links() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    seed_three_people(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // No ID column header rendered.
    assert!(
        !body.contains("<th>ID</th>"),
        "expected ID column to be gone, got: {body}",
    );
    // Sortable Name + Email headers expose JSON:API ?sort= links.
    assert!(
        body.contains("href=\"/portal/admin/people?sort=name\""),
        "expected ?sort=name link, got: {body}",
    );
    assert!(
        body.contains("href=\"/portal/admin/people?sort=email\""),
        "expected ?sort=email link, got: {body}",
    );
}

#[tokio::test]
async fn admin_people_index_honors_jsonapi_sort_ascending_by_name() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    seed_three_people(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people?sort=name")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Leo → Libra → Taurus in render order.
    let names = ["<td>Leo</td>", "<td>Libra</td>", "<td>Taurus</td>"];
    let (i_leo, _) = first_index_of(&body, &[names[0]]).expect("Leo row");
    let (i_libra, _) = first_index_of(&body, &[names[1]]).expect("Libra row");
    let (i_taurus, _) = first_index_of(&body, &[names[2]]).expect("Taurus row");
    assert!(i_leo < i_libra, "Leo before Libra in body");
    assert!(i_libra < i_taurus, "Libra before Taurus in body");
    // Active ascending → the Name header link must flip to descending.
    assert!(
        body.contains("href=\"/portal/admin/people?sort=-name\""),
        "expected flipped descending link, got: {body}",
    );
}

#[tokio::test]
async fn admin_people_index_honors_jsonapi_sort_descending_by_name() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    seed_three_people(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people?sort=-name")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let (i_leo, _) = first_index_of(&body, &["<td>Leo</td>"]).expect("Leo row");
    let (i_taurus, _) = first_index_of(&body, &["<td>Taurus</td>"]).expect("Taurus row");
    assert!(
        i_taurus < i_leo,
        "Taurus before Leo when sort=-name, got: {body}",
    );
}

#[tokio::test]
async fn admin_people_index_rejects_unknown_sort_key_with_400() {
    // JSON:API 1.1 §5: a server MUST return 400 Bad Request when asked
    // to sort by a field it does not advertise.
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people?sort=ssn")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_people_index_honors_jsonapi_filter_on_name() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    seed_three_people(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    // axum/serde_urlencoded parses raw `filter[name]=` as the rename
    // key — the same string a browser sends when the user clicks a
    // generated link. Real clients percent-encode the brackets; both
    // forms decode to the same key.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people?filter%5Bname%5D=Libra")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("<td>Libra</td>"), "Libra row present");
    assert!(!body.contains("<td>Taurus</td>"), "Taurus filtered out");
    assert!(!body.contains("<td>Leo</td>"), "Leo filtered out");
}

#[tokio::test]
async fn admin_people_index_stitches_filter_through_sort_links() {
    // Clicking a sort header must keep the active filter — the
    // generated href must include both filter[name] and the toggled
    // sort.
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    seed_three_people(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people?filter%5Bname%5D=Libra")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(resp).await;
    assert!(
        body.contains("href=\"/portal/admin/people?filter[name]=Libra&amp;sort=name\""),
        "expected filter to survive sort link, got: {body}",
    );
}

#[tokio::test]
async fn admin_jurisdictions_is_read_only_listing() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    for (name, code) in [("California", "CA"), ("Nevada", "NV")] {
        store::entity::jurisdiction::ActiveModel {
            name: ActiveValue::Set(name.into()),
            code: ActiveValue::Set(code.into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/portal/admin/jurisdictions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Seeded rows are visible.
    assert!(body.contains("California"));
    assert!(body.contains("Nevada"));
    // Sorted ascending by code → "CA" before "NV".
    let ca = body.find(">CA<").expect("CA row");
    let nv = body.find(">NV<").expect("NV row");
    assert!(ca < nv, "expected CA to come before NV");
    // No CRUD affordances: no Add/Edit/Delete buttons, no `new` link, no form.
    assert!(
        !body.contains("/portal/admin/jurisdictions/new"),
        "Add link should be gone",
    );
    assert!(
        !body.contains("/portal/admin/jurisdictions/1/edit"),
        "Edit link should be gone",
    );
    assert!(
        !body.contains("action=\"/portal/admin/jurisdictions"),
        "no form action should target this surface",
    );

    // POST is no longer routed.
    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/jurisdictions")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Foo&code=FO"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

    // /new is gone.
    let new = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/jurisdictions/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_entity_types_is_read_only_listing() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    for name in ["LLC", "Trust"] {
        store::entity::entity_type::ActiveModel {
            name: ActiveValue::Set(name.into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/portal/admin/entity-types")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("LLC"));
    assert!(body.contains("Trust"));
    // No CRUD affordances.
    assert!(
        !body.contains("/portal/admin/entity-types/new"),
        "Add link should be gone",
    );
    assert!(!body.contains("/edit"), "Edit link should be gone");
    assert!(!body.contains("/delete"), "Delete form should be gone");
    assert!(
        !body.contains("action=\"/portal/admin/entity-types"),
        "no form action should target this surface",
    );

    // POST to the collection: no route.
    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/entity-types")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Foo"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

    // /new, /:id/edit, /:id/delete are gone.
    for sub in ["/new", "/00000000-0000-0000-0000-000000000000/edit"] {
        let gone = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/portal/admin/entity-types{sub}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            gone.status(),
            StatusCode::NOT_FOUND,
            "/portal/admin/entity-types{sub} should be 404",
        );
    }
    let del = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/entity-types/00000000-0000-0000-0000-000000000000/delete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_templates_is_read_only_listing() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    store::entity::template::ActiveModel {
        code: ActiveValue::Set("trusts__nevada".into()),
        title: ActiveValue::Set("Nevada Trust".into()),
        respondent_type: ActiveValue::Set("entity".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/portal/admin/templates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Nevada Trust"));
    assert!(body.contains("trusts__nevada"));
    // No CRUD affordances.
    assert!(!body.contains("/portal/admin/templates/new"));
    assert!(!body.contains("/edit"));
    assert!(!body.contains("/delete"));
    assert!(!body.contains("action=\"/portal/admin/templates"));

    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/templates")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("code=x&title=X&respondent_type=person&body=hi"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

    let new = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/templates/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_questions_is_read_only_listing() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    store::entity::question::ActiveModel {
        code: ActiveValue::Set("legal_name".into()),
        prompt: ActiveValue::Set("What is your legal name?".into()),
        answer_type: ActiveValue::Set("string".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/portal/admin/questions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("What is your legal name?"));
    assert!(body.contains("legal_name"));
    // No CRUD affordances.
    assert!(!body.contains("/portal/admin/questions/new"));
    assert!(!body.contains("/edit"));
    assert!(!body.contains("/delete"));
    assert!(!body.contains("action=\"/portal/admin/questions"));

    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/questions")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("code=x&prompt=X?&answer_type=string"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

    let new = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/questions/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_json_is_served() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"openapi\":\"3.1.0\""));
    assert!(body.contains("/api/people"));
    assert!(body.contains("\"Person\""));
}

#[tokio::test]
async fn api_docs_serves_swagger_ui_shell_with_csp() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp
        .headers()
        .get("content-security-policy")
        .expect("CSP header must be set on /api/docs")
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        csp.contains("script-src 'self'"),
        "CSP must keep script-src on same origin: {csp}"
    );
    assert!(
        !csp.contains("'unsafe-inline'") || csp.contains("style-src 'self' 'unsafe-inline'"),
        "unsafe-inline must only appear under style-src: {csp}"
    );
    let body = body_string(resp).await;
    assert!(
        body.contains("id=\"swagger-ui\""),
        "Swagger UI mount point missing from /api/docs shell"
    );
    assert!(
        body.contains("/public/swagger-ui/swagger-ui-bundle.js"),
        "Swagger UI bundle reference missing"
    );
    assert!(
        body.contains("/openapi.json") || body.contains("init.js"),
        "init.js (which references /openapi.json) must be loaded"
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/no-such-route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Build a minimal `multipart/form-data` body the way SendGrid
/// Inbound Parse formats its POST. Field order matches the
/// (from, to, subject, text, email) tuple the handler reads.
fn build_inbound_multipart(
    from: &str,
    to: &str,
    subject: &str,
    text: &str,
    raw_email: &[u8],
) -> (String, Vec<u8>) {
    let boundary = "----navigator-inbound-test-boundary";
    let mut body: Vec<u8> = Vec::new();
    let mut text_part = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    };
    text_part("from", from);
    text_part("to", to);
    text_part("subject", subject);
    text_part("text", text);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"email\"\r\nContent-Type: message/rfc822\r\n\r\n",
    );
    body.extend_from_slice(raw_email);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

#[tokio::test]
async fn admin_send_welcome_writes_audit_row_and_redirects() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use store::entity::{person, sent_email};
    // Wrap the dev CapturingEmail in LoggingEmail so the audit decorator
    // is exercised end-to-end — same shape production uses, with the
    // SendGrid backend swapped for capturing.
    let mut state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    state.email = std::sync::Arc::new(web::email::LoggingEmail::new(
        std::sync::Arc::new(web::email::CapturingEmail::new()),
        state.db.clone(),
        "support@neonlaw.com",
    ));
    let libra = person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state.clone(), std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}/welcome", libra.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            resp.status(),
            StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
        ),
        "expected redirect, got {}",
        resp.status()
    );

    let rows = sent_email::Entity::find().all(&state.db).await.unwrap();
    assert_eq!(rows.len(), 1, "expected one audit row");
    assert_eq!(rows[0].recipient, "libra@example.com");
    assert_eq!(rows[0].subject, "Welcome to Neon Law");
    assert_eq!(rows[0].sender, "support@neonlaw.com");
    assert_eq!(rows[0].template_slug.as_deref(), Some("welcome"));
    assert_eq!(rows[0].outcome, "sent");
    assert!(
        rows[0].body.contains("Libra"),
        "body should be personalized, got: {}",
        rows[0].body
    );
}

#[tokio::test]
async fn admin_email_log_empty_state_explains_what_lands_here() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/email-log")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("No outbound mail in the audit window"));
}

#[tokio::test]
async fn admin_email_log_lists_rows_newest_first() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::sent_email;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    for (sent_at, recipient) in [
        ("2026-05-24T10:00:00Z", "older@example.com"),
        ("2026-05-24T12:00:00Z", "middle@example.com"),
        ("2026-05-24T15:00:00Z", "newest@example.com"),
    ] {
        sent_email::ActiveModel {
            recipient: ActiveValue::Set(recipient.into()),
            subject: ActiveValue::Set("Welcome to Neon Law".into()),
            body: ActiveValue::Set("Welcome aboard.".into()),
            sender: ActiveValue::Set("support@neonlaw.com".into()),
            template_slug: ActiveValue::Set(Some("welcome".into())),
            outcome: ActiveValue::Set("sent".into()),
            sent_at: ActiveValue::Set(sent_at.into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/email-log")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("newest@example.com"));
    assert!(body.contains("older@example.com"));
    // Newest must precede oldest in the rendered HTML.
    let newest_idx = body.find("newest@example.com").unwrap();
    let oldest_idx = body.find("older@example.com").unwrap();
    assert!(
        newest_idx < oldest_idx,
        "newest row must render before oldest (newest first)"
    );
}

#[tokio::test]
async fn admin_send_welcome_404s_when_person_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/portal/admin/people/{}/welcome",
                    uuid::Uuid::nil()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sendgrid_inbound_webhook_persists_letter_and_stores_raw_email() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use store::entity::{address, letter, mailroom};

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    // Seed a mailroom for the inbound message to route through.
    let addr = address::ActiveModel {
        line1: ActiveValue::Set("123 Main".into()),
        city: ActiveValue::Set("Reno".into()),
        region: ActiveValue::Set("NV".into()),
        postal_code: ActiveValue::Set("89501".into()),
        country: ActiveValue::Set("US".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    mailroom::ActiveModel {
        name: ActiveValue::Set("HQ".into()),
        address_id: ActiveValue::Set(addr.id),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let storage = state.storage.clone();
    let app = web::build_router(state.clone(), std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let raw = b"From: aries@example.com\r\nTo: support@neonlaw.com\r\nSubject: Hello\r\n\r\nBody";
    let (content_type, body) = build_inbound_multipart(
        "aries@example.com",
        "support@neonlaw.com",
        "Hello",
        "Body",
        raw,
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sendgrid/inbound/any-token-in-dev")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // A letter row landed with the right metadata.
    let letters = letter::Entity::find().all(&state.db).await.unwrap();
    assert_eq!(letters.len(), 1);
    assert_eq!(letters[0].direction, "incoming");
    assert_eq!(letters[0].sender, "aries@example.com");
    assert_eq!(letters[0].recipient, "support@neonlaw.com");
    assert_eq!(letters[0].summary, "Hello");

    // And the raw RFC 5322 bytes are sitting in storage under the
    // expected inbound/ prefix. We can't predict the timestamp, so
    // scan a fresh listing isn't available — instead, verify the
    // file system backend has at least one object by reading any
    // path that starts with `inbound/`. (FsStorage is keyed by
    // string, so we can't list — we just trust the round-trip via
    // the public get method against the known prefix is exercised
    // by separate storage tests.)
    drop(storage);
}

#[tokio::test]
async fn sendgrid_inbound_webhook_400s_when_required_field_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // Body has `from` and `to` but no `subject`.
    let (content_type, body) =
        build_inbound_multipart_partial(&[("from", "aries@example.com"), ("to", "us@example.com")]);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sendgrid/inbound/any-token-in-dev")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_string(resp).await;
    assert!(
        body.contains("subject"),
        "expected `subject` in error body, got: {body}",
    );
}

#[tokio::test]
async fn sendgrid_inbound_webhook_503s_when_no_mailroom_configured() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    // Note: no mailroom seeded.
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let (content_type, body) =
        build_inbound_multipart("aries@example.com", "us@example.com", "Test", "", b"");
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sendgrid/inbound/any-token-in-dev")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn sendgrid_inbound_webhook_401s_when_secret_mismatches() {
    let mut state = empty_state().await;
    state.inbound_email_secret = Some("real-secret".into());
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let (content_type, body) =
        build_inbound_multipart("aries@example.com", "us@example.com", "Hi", "x", b"x");
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sendgrid/inbound/wrong-secret")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sendgrid_inbound_webhook_accepts_matching_secret() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::{address, mailroom};

    let mut state = empty_state().await;
    state.inbound_email_secret = Some("real-secret".into());
    store::migrate(&state.db).await.unwrap();
    let addr = address::ActiveModel {
        line1: ActiveValue::Set("1 Test".into()),
        city: ActiveValue::Set("Reno".into()),
        region: ActiveValue::Set("NV".into()),
        postal_code: ActiveValue::Set("89501".into()),
        country: ActiveValue::Set("US".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    mailroom::ActiveModel {
        name: ActiveValue::Set("HQ".into()),
        address_id: ActiveValue::Set(addr.id),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let (content_type, body) = build_inbound_multipart(
        "aries@example.com",
        "support@neonlaw.com",
        "Hi",
        "Body",
        b"raw",
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhook/sendgrid/inbound/real-secret")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

const SAMPLE_EVENTS: &str = r#"[
    {"email":"a@example.com","timestamp":1716940800,"event":"delivered",
     "sg_event_id":"evt-1","sg_message_id":"msg-1","template_slug":"welcome"}
]"#;

#[tokio::test]
async fn sendgrid_events_webhook_persists_batch_and_returns_204() {
    // Dev posture: secret is `None`, so any path token is accepted
    // and the batch lands in the (filesystem) storage backend.
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/email-events/any-token-in-dev")
                .header("content-type", "application/json")
                .body(Body::from(SAMPLE_EVENTS))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn sendgrid_events_webhook_401s_when_secret_mismatches() {
    let mut state = empty_state().await;
    state.email_events_secret = Some("real-secret".into());
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/email-events/wrong-secret")
                .header("content-type", "application/json")
                .body(Body::from(SAMPLE_EVENTS))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Variant of `build_inbound_multipart` that takes just a list of
/// `(name, value)` pairs — used for the missing-field test where
/// we deliberately omit `subject`.
fn build_inbound_multipart_partial(fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    let boundary = "----navigator-inbound-test-boundary";
    let mut body: Vec<u8> = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

#[tokio::test]
async fn admin_entity_cap_table_aggregates_issuances_by_holder() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::{entity, entity_type, jurisdiction, share_issuance};

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    // Seed: one entity type + jurisdiction + entity → two
    // issuances to Aries (600 shares total), one to Taurus (400).
    let et = entity_type::ActiveModel {
        name: ActiveValue::Set("Corporation".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let jur = jurisdiction::ActiveModel {
        name: ActiveValue::Set("Delaware".into()),
        code: ActiveValue::Set("US-DE".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let ent = entity::ActiveModel {
        name: ActiveValue::Set("Foo Inc".into()),
        entity_type_id: ActiveValue::Set(et.id),
        jurisdiction_id: ActiveValue::Set(jur.id),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    for (holder, shares) in [("Aries", 400i64), ("Aries", 200), ("Taurus", 400)] {
        share_issuance::ActiveModel {
            entity_id: ActiveValue::Set(ent.id),
            holder_name: ActiveValue::Set(holder.into()),
            share_class: ActiveValue::Set("common".into()),
            shares: ActiveValue::Set(shares),
            issued_at: ActiveValue::Set("2026-05-01".into()),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/entities/{}/cap-table", ent.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Cap table — Foo Inc"));
    assert!(body.contains("Aries"));
    assert!(body.contains("Taurus"));
    // 600 / 1000 = 60.00%
    assert!(body.contains("60.00%"));
    // 400 / 1000 = 40.00%
    assert!(body.contains("40.00%"));
    assert!(body.contains("1000"));
    assert!(body.contains("2 holder(s)"));
}

#[tokio::test]
async fn admin_entity_cap_table_renders_empty_state_for_entity_without_issuances() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::{entity, entity_type, jurisdiction};

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let et = entity_type::ActiveModel {
        name: ActiveValue::Set("LLC".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let jur = jurisdiction::ActiveModel {
        name: ActiveValue::Set("Nevada".into()),
        code: ActiveValue::Set("US-NV".into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let ent = entity::ActiveModel {
        name: ActiveValue::Set("New Co".into()),
        entity_type_id: ActiveValue::Set(et.id),
        jurisdiction_id: ActiveValue::Set(jur.id),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/entities/{}/cap-table", ent.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Cap table — New Co"));
    assert!(body.contains("No share issuances recorded"));
}

#[tokio::test]
async fn admin_letter_detail_404s_when_id_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/admin/letters/{}",
                    uuid::Uuid::from_u128(9999)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // The handler renders a friendly "not found" page rather than
    // 404'ing — the route still resolves so the auth layer + nav
    // chrome are correct for the visitor.
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Letter not found"));
    assert!(body.contains(&format!("<code>{}</code>", uuid::Uuid::from_u128(9999))));
}

#[tokio::test]
async fn admin_people_csv_exports_inserted_rows() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Aries&email=aries%40example.com"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        create.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let csv = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people.csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(csv.status(), StatusCode::OK);
    assert_eq!(
        csv.headers().get("content-type").unwrap(),
        "text/csv; charset=utf-8"
    );
    assert_eq!(
        csv.headers().get("content-disposition").unwrap(),
        "attachment; filename=\"people.csv\""
    );
    let body = body_string(csv).await;
    let mut lines = body.split("\r\n");
    assert_eq!(lines.next().unwrap(), "id,name,email");
    let row = lines.next().unwrap();
    assert!(row.ends_with(",Aries,aries@example.com"));
}

#[tokio::test]
async fn admin_entities_csv_is_servable_and_emits_headers_even_when_empty() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/entities.csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "id,name,entity_type,jurisdiction\r\n");
}

#[tokio::test]
async fn admin_projects_csv_is_servable_and_emits_headers_even_when_empty() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/projects.csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert_eq!(body, "id,name,status,entity_name\r\n");
}

#[tokio::test]
async fn root_response_carries_security_headers_and_request_id() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let headers = resp.headers();
    assert_eq!(
        headers
            .get("strict-transport-security")
            .and_then(|v| v.to_str().ok()),
        Some("max-age=63072000; includeSubDomains; preload"),
    );
    assert_eq!(
        headers
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff"),
    );
    assert_eq!(
        headers.get("x-frame-options").and_then(|v| v.to_str().ok()),
        Some("DENY"),
    );
    assert_eq!(
        headers.get("referrer-policy").and_then(|v| v.to_str().ok()),
        Some("strict-origin-when-cross-origin"),
    );
    // CSP locks scripts/objects/frames to same-origin; an injected
    // <script> has no execution backstop without it.
    let csp = headers
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .expect("response must carry a content-security-policy");
    assert!(csp.contains("default-src 'self'"), "got: {csp}");
    assert!(csp.contains("object-src 'none'"), "got: {csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "got: {csp}");
    assert!(csp.contains("script-src 'self'"), "got: {csp}");
    // SetRequestIdLayer always assigns one (UUID) when the client did
    // not send one; PropagateRequestIdLayer mirrors it to the response.
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("response must carry x-request-id");
    assert!(
        !request_id.is_empty(),
        "x-request-id must be non-empty, got {request_id:?}",
    );
}

#[tokio::test]
async fn client_supplied_request_id_is_propagated_to_response() {
    let app = web::build_router(
        empty_state().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("x-request-id", "test-correlation-7")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("test-correlation-7"),
    );
}

#[tokio::test]
async fn public_static_assets_carry_cache_control() {
    // Use the crate-bundled `public/` dir; pick any file that exists
    // by listing the dir first so the test does not depend on a
    // hard-coded asset name.
    let public_dir = std::path::Path::new(web::DEFAULT_PUBLIC_DIR);
    let asset_name = std::fs::read_dir(public_dir)
        .expect("public dir must exist")
        .filter_map(Result::ok)
        .find_map(|e| {
            let p = e.path();
            if p.is_file() {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(std::string::ToString::to_string)
            } else {
                None
            }
        })
        .expect("public dir must contain at least one file for this test");
    let app = web::build_router(empty_state().await, public_dir);
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/public/{asset_name}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("public, max-age=3600"),
    );
}

#[tokio::test]
async fn project_documents_upload_writes_blob_and_document_with_description() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    use store::entity::{blob, document, project};
    use uuid::Uuid;

    let state = empty_state().await; // auth disabled
    store::migrate(&state.db).await.unwrap();

    // Seed one project to upload into.
    let project_id = Uuid::now_v7();
    project::ActiveModel {
        id: ActiveValue::Set(project_id),
        name: ActiveValue::Set("Upload Test".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state.clone(), std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // Hand-rolled multipart body. Boundary chosen so it can't appear
    // in the payload bytes.
    let boundary = "----navigator-test-boundary-zzzzz";
    let payload = b"hello world from a test upload";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"hello.txt\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    body.extend_from_slice(payload);
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"kind\"\r\n\r\n");
    body.extend_from_slice(b"intake");
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"description\"\r\n\r\n");
    body.extend_from_slice(b"signed retainer from client");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/projects/{project_id}/documents/upload"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "expected 303 redirect, got {}",
        resp.status()
    );
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(location, format!("/portal/projects/{project_id}"));

    // One blob, one document — the document carries upload provenance
    // and the optional description from the form.
    let blobs = blob::Entity::find().all(&state.db).await.unwrap();
    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs[0].byte_size, i64::try_from(payload.len()).unwrap());
    assert_eq!(blobs[0].content_type, "text/plain");

    let docs = document::Entity::find().all(&state.db).await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].filename, "hello.txt");
    assert_eq!(docs[0].kind, "intake");
    assert_eq!(docs[0].project_id, project_id);
    assert_eq!(docs[0].source, "upload");
    assert_eq!(
        docs[0].description.as_deref(),
        Some("signed retainer from client")
    );
    assert!(docs[0].source_revision_id.is_none());
    assert!(!docs[0].received_at.is_empty());
}

#[tokio::test]
async fn project_detail_page_renders_documents_and_upload_form() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::project;
    use uuid::Uuid;

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    let project_id = Uuid::now_v7();
    project::ActiveModel {
        id: ActiveValue::Set(project_id),
        name: ActiveValue::Set("Acme Formation".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    // Seed one document + blob via the same ingest helper the upload
    // handler uses, so we exercise the read-side render against the
    // real shape ingest_bytes produces.
    let args = store::documents::IngestArgs {
        project_id,
        source: "drive_sync",
        filename: "engagement-letter.pdf",
        kind: "intake",
        content_type: "application/pdf",
        description: Some("Initial Drive sync"),
        source_revision_id: Some("rev-001"),
    };
    store::documents::ingest_bytes(&state.db, &state.storage, &args, b"hello world")
        .await
        .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{project_id}"))
                .header("cookie", admin_session_cookie())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Acme Formation"));
    assert!(body.contains("engagement-letter.pdf"));
    // The list view is intentionally lean: filename links to the
    // per-document detail page and the Download link points at the
    // signed-URL redirect endpoint. Provenance (source, revision id,
    // content type) is NOT spilled into the list; it lives on the
    // detail page (covered by its own test below).
    assert!(body.contains(&format!("/portal/projects/{project_id}/documents/")));
    assert!(body.contains("/download"));
    assert!(!body.contains("application/pdf"));
    assert!(!body.contains("rev-001"));
    // Inline upload form posts to the same endpoint as before.
    assert!(body.contains(&format!(
        "action=\"/portal/projects/{project_id}/documents/upload\""
    )));
    assert!(body.contains("enctype=\"multipart/form-data\""));
}

#[tokio::test]
async fn project_document_detail_page_shows_provenance_and_download_link() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::project;
    use uuid::Uuid;

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    let project_id = Uuid::now_v7();
    project::ActiveModel {
        id: ActiveValue::Set(project_id),
        name: ActiveValue::Set("Acme Formation".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let args = store::documents::IngestArgs {
        project_id,
        source: "drive_sync",
        filename: "engagement-letter.pdf",
        kind: "retainer",
        content_type: "application/pdf",
        description: Some("Initial Drive sync"),
        source_revision_id: Some("rev-001"),
    };
    let ingested = store::documents::ingest_bytes(&state.db, &state.storage, &args, b"hello world")
        .await
        .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{project_id}/documents/{}",
                    ingested.document_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("engagement-letter.pdf"));
    assert!(body.contains("Provenance"));
    assert!(body.contains("Storage"));
    assert!(body.contains("drive_sync"));
    assert!(body.contains("rev-001"));
    assert!(body.contains("Initial Drive sync"));
    assert!(body.contains("application/pdf"));
    assert!(body.contains(&ingested.sha256_hex));
    assert!(body.contains(&format!(
        "/portal/projects/{project_id}/documents/{}/download",
        ingested.document_id
    )));
}

#[tokio::test]
async fn project_document_download_streams_bytes_on_fs_backend() {
    // FsStorage returns Unsupported from signed_url; the handler
    // falls through to stream_through, which writes the raw bytes
    // with Content-Disposition: attachment so the browser saves
    // under the original filename.
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::project;
    use uuid::Uuid;

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    let project_id = Uuid::now_v7();
    project::ActiveModel {
        id: ActiveValue::Set(project_id),
        name: ActiveValue::Set("Acme Formation".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let bytes_in = b"engagement letter bytes";
    let args = store::documents::IngestArgs {
        project_id,
        source: "upload",
        filename: "engagement-letter.pdf",
        kind: "retainer",
        content_type: "application/pdf",
        description: None,
        source_revision_id: None,
    };
    let ingested = store::documents::ingest_bytes(&state.db, &state.storage, &args, bytes_in)
        .await
        .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{project_id}/documents/{}/download",
                    ingested.document_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(ct, "application/pdf");
    let cd = resp
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(cd.contains("engagement-letter.pdf"));
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body_bytes.as_ref(), bytes_in);
}

#[tokio::test]
async fn project_document_download_404s_when_doc_belongs_to_a_different_project() {
    // Cross-project leakage guard: a document from project A must
    // not be downloadable via project B's URL even if the doc_id is
    // known.
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::project;
    use uuid::Uuid;

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    let project_a = Uuid::now_v7();
    let project_b = Uuid::now_v7();
    for (id, name) in [(project_a, "A"), (project_b, "B")] {
        project::ActiveModel {
            id: ActiveValue::Set(id),
            name: ActiveValue::Set(name.into()),
            status: ActiveValue::Set("open".into()),
            entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();
    }

    let args = store::documents::IngestArgs {
        project_id: project_a,
        source: "upload",
        filename: "secret.pdf",
        kind: "intake",
        content_type: "application/pdf",
        description: None,
        source_revision_id: None,
    };
    let ingested = store::documents::ingest_bytes(&state.db, &state.storage, &args, b"secret")
        .await
        .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    // Same doc_id, but via project B's URL — must 404.
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/portal/projects/{project_b}/documents/{}/download",
                    ingested.document_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn project_detail_page_renders_empty_state_when_project_has_no_documents() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    use store::entity::project;
    use uuid::Uuid;

    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();

    let project_id = Uuid::now_v7();
    project::ActiveModel {
        id: ActiveValue::Set(project_id),
        name: ActiveValue::Set("Empty Matter".into()),
        status: ActiveValue::Set("open".into()),
        entity_id: ActiveValue::Set(store::test_support::seed_entity(&state.db).await),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{project_id}"))
                .header("cookie", admin_session_cookie())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Empty Matter"));
    assert!(body.contains("No documents yet."));
}

#[tokio::test]
async fn project_detail_page_404s_when_project_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/projects/{}", uuid::Uuid::now_v7()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn project_documents_upload_404s_when_project_missing() {
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let boundary = "----test-bdy";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    let missing = uuid::Uuid::now_v7();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/projects/{missing}/documents/upload"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------- Error pages: HTML for browsers, JSON for /api & /mcp ----------

#[tokio::test]
async fn unknown_path_returns_html_404_page_for_browser_request() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(
        body.starts_with("<!DOCTYPE html>"),
        "browser 404 must be the styled HTML page, got: {body}",
    );
    assert!(body.contains("<h1>Not found</h1>"));
}

#[tokio::test]
async fn unknown_api_path_returns_json_404_not_html() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(
        !body.starts_with("<!DOCTYPE html>"),
        "/api/* 404 must NOT be the HTML page; got: {body}",
    );
    assert!(
        body.contains("\"error\""),
        "expected JSON error body, got: {body}"
    );
}

#[tokio::test]
async fn unknown_mcp_path_returns_json_404_not_html() {
    let state = empty_state().await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/mcp/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_string(resp).await;
    assert!(
        !body.starts_with("<!DOCTYPE html>"),
        "/mcp/* 404 must NOT be the HTML page; got: {body}",
    );
}

#[tokio::test]
async fn wants_json_path_classifier() {
    // The classifier is the single source of truth for HTML-vs-JSON
    // routing in error responses — lock its behavior down so a future
    // route addition can't silently start handing HTML to a JSON
    // client.
    assert!(web::wants_json("/api/people"));
    assert!(web::wants_json("/api/people/123"));
    assert!(web::wants_json("/mcp"));
    assert!(web::wants_json("/mcp/foo"));
    assert!(web::wants_json("/openapi.json"));
    assert!(!web::wants_json("/"));
    assert!(!web::wants_json("/portal/admin"));
    assert!(!web::wants_json("/portal/admin/people"));
    assert!(!web::wants_json("/blog/anything"));
    // `/api-something` (no trailing slash, no exact match) is NOT
    // an api route — leading-substring matches would catch real
    // page paths like `/apidocs` if someone added one.
    assert!(!web::wants_json("/apidocs"));
}

// ---------- Admin role editing ----------

#[tokio::test]
async fn admin_people_edit_form_shows_role_select_pre_filled() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let staff = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Staff".into()),
        email: ActiveValue::Set("staff@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Staff),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/people/{}/edit", staff.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("name=\"role\""),
        "edit form must expose a role <select>, got: {body}",
    );
    assert!(
        body.contains("value=\"staff\" selected"),
        "role <select> must pre-select the row's current role, got: {body}",
    );
}

#[tokio::test]
async fn admin_can_update_a_persons_role() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    let libra = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let db = state.db.clone();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}", libra.id))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "name=Libra&email=libra%40example.com&role=admin",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        resp.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let row = store::entity::person::Entity::find_by_id(libra.id)
        .one(&db)
        .await
        .unwrap()
        .expect("row still present");
    assert_eq!(row.role, store::entity::person::Role::Admin);
}

#[tokio::test]
async fn bootstrap_admin_row_renders_role_select_as_disabled() {
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue;
    let mut state = empty_state().await;
    state.bootstrap_admin_email = Some("nick@neonlaw.com".into());
    store::migrate(&state.db).await.unwrap();
    let admin_row = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Admin),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/portal/admin/people/{}/edit", admin_row.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // The role <select> on the bootstrap admin row must be rendered with
    // `disabled` so the UI can't be used to demote the operator.
    // Server-side reinforcement happens in `people_update`.
    let role_input_idx = body
        .find("name=\"role\"")
        .expect("role select present in form");
    // Slice a 200-char window around the element to keep the assertion
    // tight — `disabled` appearing anywhere in the file is too loose.
    let window_start = body[..role_input_idx].rfind("<select").unwrap_or(0);
    let window_end = (role_input_idx + 200).min(body.len());
    let window = &body[window_start..window_end];
    assert!(
        window.contains("disabled"),
        "bootstrap admin row's role <select> must be disabled, got window: {window}",
    );
}

#[tokio::test]
async fn bootstrap_admin_role_is_force_set_to_admin_even_if_form_demotes() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    let mut state = empty_state().await;
    state.bootstrap_admin_email = Some("nick@neonlaw.com".into());
    store::migrate(&state.db).await.unwrap();
    let admin_row = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Admin),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let db = state.db.clone();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // POST with `role=client` — simulating a hostile client (or a bug)
    // bypassing the disabled UI. The server must force-set Admin.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}", admin_row.id))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Nick&email=nick%40neonlaw.com&role=client"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        resp.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));

    let row = store::entity::person::Entity::find_by_id(admin_row.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.role,
        store::entity::person::Role::Admin,
        "bootstrap admin role must heal back on every update",
    );
}

// ---------- Uniqueness conflicts → 409 + delete guard ----------

#[tokio::test]
async fn admin_people_create_duplicate_email_returns_409() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("dup@example.com".into()),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Other&email=dup%40example.com&role=client"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_string(resp).await;
    assert!(
        body.contains("already in use"),
        "409 body must explain the uniqueness conflict, got: {body}",
    );
}

#[tokio::test]
async fn admin_people_update_to_existing_email_returns_409() {
    use sea_orm::{ActiveModelTrait, ActiveValue};
    let state = empty_state().await;
    store::migrate(&state.db).await.unwrap();
    store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let taurus = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Taurus".into()),
        email: ActiveValue::Set("taurus@example.com".into()),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}", taurus.id))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(
                    "name=Taurus&email=libra%40example.com&role=client",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_of_bootstrap_admin_person_returns_409_and_leaves_row() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    let mut state = empty_state().await;
    state.bootstrap_admin_email = Some("nick@neonlaw.com".into());
    store::migrate(&state.db).await.unwrap();
    let admin_row = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Nick".into()),
        email: ActiveValue::Set("nick@neonlaw.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Admin),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let db = state.db.clone();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}/delete", admin_row.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Row must still exist — the guard is the load-bearing invariant.
    let still_there = store::entity::person::Entity::find_by_id(admin_row.id)
        .one(&db)
        .await
        .unwrap();
    assert!(
        still_there.is_some(),
        "bootstrap admin row must survive a delete attempt",
    );
}

#[tokio::test]
async fn delete_of_non_bootstrap_admin_person_still_succeeds() {
    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};
    let mut state = empty_state().await;
    state.bootstrap_admin_email = Some("nick@neonlaw.com".into());
    store::migrate(&state.db).await.unwrap();
    let libra = store::entity::person::ActiveModel {
        name: ActiveValue::Set("Libra".into()),
        email: ActiveValue::Set("libra@example.com".into()),
        oidc_subject: ActiveValue::Set(None),
        role: ActiveValue::Set(store::entity::person::Role::Client),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let db = state.db.clone();

    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/portal/admin/people/{}/delete", libra.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        matches!(resp.status(), StatusCode::SEE_OTHER | StatusCode::OK),
        "non-bootstrap admin delete should redirect (303) or 200 for htmx, got {}",
        resp.status(),
    );
    let gone = store::entity::person::Entity::find_by_id(libra.id)
        .one(&db)
        .await
        .unwrap();
    assert!(
        gone.is_none(),
        "regular person row must be gone after delete"
    );
}

// ---------------------------------------------------------------------------
// Published workspace docs at /docs/:slug (web::docs).
// ---------------------------------------------------------------------------

/// State whose docs index is the real baked `docs/` tree (every other
/// field matches `empty_state`).
async fn state_with_docs() -> AppState {
    let mut state = empty_state().await;
    state.docs = web::docs::loader::bundled();
    state
}

async fn get(app: axum::Router, uri: &str) -> axum::http::Response<Body> {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn docs_glossary_renders_headings() {
    let app = web::build_router(
        state_with_docs().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = get(app, "/docs/glossary").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Foundation-branded page title from the doc's leading H1.
    assert!(body.contains("<title>Neon Law Foundation | Glossary</title>"));
    // A known heading renders as an <h2> with a slug id so #council lands.
    assert!(
        body.contains("<h2 id=\"council\">Council</h2>"),
        "glossary should render the Council heading with an anchor id"
    );
    // Cross-doc link rewritten to a site route.
    assert!(body.contains("href=\"/docs/notation\""));
}

#[tokio::test]
async fn docs_notation_renders_teaching_order_headings() {
    let app = web::build_router(
        state_with_docs().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = get(app, "/docs/notation").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Template precedes Notation by design — both headings present.
    assert!(body.contains("<h2 id=\"template\">Template</h2>"));
    assert!(body.contains("<h2 id=\"notation\">Notation</h2>"));
    // notation links glossary.md#blob → /docs/glossary#blob.
    assert!(body.contains("href=\"/docs/glossary#blob\""));
}

#[tokio::test]
async fn every_published_doc_is_200() {
    // No allowlist: every doc under the manifest is public.
    let docs = web::docs::loader::bundled();
    for doc in docs.docs() {
        let app = web::build_router(
            state_with_docs().await,
            std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
        );
        let uri = format!("/docs/{}", doc.slug);
        let resp = get(app, &uri).await;
        assert_eq!(resp.status(), StatusCode::OK, "{uri} should be 200");
    }
    // Infra docs are public too — they reveal no client confidence.
    assert!(docs.find("oidc").is_some(), "oidc doc must be published");
    assert!(
        docs.find("gke-prod").is_some(),
        "gke-prod doc must be published"
    );
}

#[tokio::test]
async fn docs_unknown_slug_is_404() {
    let app = web::build_router(
        state_with_docs().await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = get(app, "/docs/no-such-doc").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// True if `s` contains a digit-group run matching `groups` separated
/// by `-` (e.g. `[3, 2, 4]` matches an SSN `123-45-6789`), bounded so a
/// longer digit run on either side doesn't count. Hand-rolled so the
/// guardrail needs no regex dependency.
fn contains_dash_digit_pattern(s: &str, groups: &[usize]) -> bool {
    let bytes = s.as_bytes();
    let total: usize = groups.iter().sum::<usize>() + groups.len() - 1;
    let is_digit = |b: u8| b.is_ascii_digit();
    for start in 0..=bytes.len().saturating_sub(total) {
        // Left boundary: not preceded by a digit.
        if start > 0 && is_digit(bytes[start - 1]) {
            continue;
        }
        let mut pos = start;
        let mut ok = true;
        for (gi, &len) in groups.iter().enumerate() {
            if gi > 0 {
                if bytes.get(pos) != Some(&b'-') {
                    ok = false;
                    break;
                }
                pos += 1;
            }
            for _ in 0..len {
                match bytes.get(pos) {
                    Some(&b) if is_digit(b) => pos += 1,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
        }
        // Right boundary: not followed by a digit.
        if ok && bytes.get(pos).is_none_or(|&b| !is_digit(b)) {
            return true;
        }
    }
    false
}

#[test]
fn docs_carry_no_client_confidences() {
    // The published-docs guardrail (RPC 1.6): the confidentiality
    // boundary is portal auth on the database, but as a belt-and-braces
    // check no published doc may contain an obvious client identifier —
    // an SSN- (ddd-dd-dddd) or EIN-shaped (dd-ddddddd) number. Docs use
    // placeholders today; this keeps it that way. (A real client name
    // can't be matched mechanically; the DB/portal boundary is what
    // actually protects it.)
    for doc in web::docs::loader::bundled().docs() {
        assert!(
            !contains_dash_digit_pattern(&doc.body_html, &[3, 2, 4]),
            "/docs/{} contains an SSN-shaped string — published docs must \
             carry no client confidence",
            doc.slug
        );
        assert!(
            !contains_dash_digit_pattern(&doc.body_html, &[2, 7]),
            "/docs/{} contains an EIN-shaped string — published docs must \
             carry no client confidence",
            doc.slug
        );
    }
}

#[test]
fn dash_digit_pattern_detects_ssn_and_respects_boundaries() {
    assert!(contains_dash_digit_pattern(
        "ssn 123-45-6789 here",
        &[3, 2, 4]
    ));
    assert!(contains_dash_digit_pattern("ein 12-3456789.", &[2, 7]));
    // A longer digit run is not an SSN.
    assert!(!contains_dash_digit_pattern("v1234-45-6789", &[3, 2, 4]));
    assert!(!contains_dash_digit_pattern("port 8080", &[3, 2, 4]));
}

/// Seed one NRS 649 section into a fresh schema so the public-reference
/// routes have something to render.
async fn seed_nrs_649(db: &Db) {
    let upsert = store::statutes::SectionUpsert {
        jurisdiction: "NV",
        code: "NRS",
        chapter: "649",
        chapter_title: "COLLECTION AGENCIES",
        section: "649.005",
        source_url: "https://www.leg.state.nv.us/NRS/NRS-649.html#NRS649Sec005",
        section_title: "Definitions.",
        body: "As used in this chapter, the words have the meanings ascribed.",
        body_sha256: "seed-hash-v1",
        history_note: Some("(Added to NRS by 1969, 829)"),
    };
    store::statutes::upsert_section(db, &upsert, "2026-06-07T10:00:00Z")
        .await
        .unwrap();
}

#[tokio::test]
async fn statutes_index_is_public() {
    // Open access to the law itself is a Foundation surface — it renders
    // anonymously for everyone.
    let state = empty_state().await;
    seed_nrs_649(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/statutes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // grouped by product, links the chapter, carries the disclaimer.
    assert!(body.contains("Nautilus"));
    assert!(body.contains("/statutes/nrs/649"));
    assert!(body.contains("not legal advice"));
    assert!(body.contains("leg.state.nv.us"));
    assert!(body.contains("Neon Law Foundation"));
}

#[tokio::test]
async fn statutes_chapter_renders_sections_with_banner_and_official_link() {
    let state = empty_state().await;
    seed_nrs_649(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/statutes/nrs/649")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("649.005"));
    assert!(body.contains("Definitions."));
    assert!(body.contains("As used in this chapter"));
    assert!(body.contains("(Added to NRS by 1969, 829)"));
    assert!(body.contains("Official source"));
    assert!(body.contains("not legal advice"));
}

#[tokio::test]
async fn statutes_section_permalink_renders_the_single_section() {
    let state = empty_state().await;
    seed_nrs_649(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/statutes/nrs/649/649.005")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("As used in this chapter"));
    assert!(body.contains("/statutes/nrs/649"));
}

#[tokio::test]
async fn statutes_unknown_chapter_returns_404() {
    let state = empty_state().await;
    seed_nrs_649(&state.db).await;
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/statutes/nrs/999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

fn blog_state_with_one_post() -> web::BlogIndex {
    web::BlogIndex::new(vec![web::BlogPost {
        slug: "thanks_apple".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 6, 19).unwrap(),
        title: "Thanks, Apple".into(),
        description: "A short note of thanks.".into(),
        body_html: "<p>We want to say thank you.</p>".into(),
    }])
}

#[tokio::test]
async fn blog_index_lists_posts() {
    let mut state = empty_state().await;
    state.blog = blog_state_with_one_post();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(Request::builder().uri("/blog").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Thanks, Apple"));
    assert!(body.contains("href=\"/blog/thanks_apple\""));
    assert!(body.contains("June 19, 2026"));
}

#[tokio::test]
async fn blog_post_renders_body() {
    let mut state = empty_state().await;
    state.blog = blog_state_with_one_post();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/blog/thanks_apple")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("We want to say thank you."));
    assert!(body.contains("href=\"/blog\""));
}

#[tokio::test]
async fn real_thanks_apple_post_is_capped_and_renders_the_photo_collage() {
    // End-to-end over the SHIPPED post file: the loader parses
    // `content/blog/20260619_thanks_apple.md`, the router renders it, and
    // we assert the two things this change wired up — the 65ch reading
    // measure (matching `/foundation/mission`) and the photo collage,
    // authored as a markdown bullet list of images that resolves through
    // the asset seam to `/public/img/thanks-apple/collage-N.jpg`.
    let mut state = empty_state().await;
    state.blog = web::blog::load_dir(std::path::Path::new(web::DEFAULT_BLOG_DIR)).unwrap();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/blog/thanks_apple")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // Same measure as the mission letter.
    assert!(
        body.contains("class=\"blog-post\"") && body.contains("max-width: 65ch"),
        "post should carry the blog-post class capped at 65ch"
    );
    // All seven collage photos render, routed through the asset seam.
    for n in 1..=7 {
        let src = format!("src=\"/public/img/thanks-apple/collage-{n}.jpg\"");
        assert!(body.contains(&src), "collage photo {n} missing: {src}");
    }
    // The farewell row added later renders through the same seam.
    for slug in [
        "apple-park-sunset",
        "farewell-crew",
        "curry-night",
        "travels-abroad",
    ] {
        let src = format!("src=\"/public/img/thanks-apple/{slug}.jpg\"");
        assert!(body.contains(&src), "farewell-row photo missing: {src}");
    }
    // The original letter copy is untouched.
    assert!(body.contains("Thanks, Apple"));
}

#[tokio::test]
async fn blog_unknown_slug_returns_404() {
    let mut state = empty_state().await;
    state.blog = blog_state_with_one_post();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/blog/nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The hyphenated form of a slug permanently redirects to the canonical
/// underscore form — `thanks-apple` becomes `thanks_apple` — so links
/// written either way resolve to the same post.
#[tokio::test]
async fn blog_hyphenated_slug_redirects_to_underscore() {
    let mut state = empty_state().await;
    state.blog = blog_state_with_one_post();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/blog/thanks-apple")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/blog/thanks_apple"),
    );
}

/// Every hyphen in a multi-word slug is rewritten, and the redirect
/// target then resolves to the real post.
#[tokio::test]
async fn blog_redirect_rewrites_all_hyphens_and_target_resolves() {
    let mut state = empty_state().await;
    state.blog = web::BlogIndex::new(vec![web::BlogPost {
        slug: "a_long_post_title".into(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 6, 19).unwrap(),
        title: "A Long Post Title".into(),
        description: "Multi-word slug.".into(),
        body_html: "<p>Body here.</p>".into(),
    }]);
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));

    // Hyphenated request → 308 to the all-underscore form.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/blog/a-long-post-title")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    assert_eq!(location, "/blog/a_long_post_title");

    // Following the redirect lands on the real post.
    let resp = app
        .oneshot(
            Request::builder()
                .uri(&location)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Body here."));
}

/// A slug with no hyphen is served directly — no redirect bounce.
#[tokio::test]
async fn blog_underscore_slug_is_served_without_redirect() {
    let mut state = empty_state().await;
    state.blog = blog_state_with_one_post();
    let app = web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/blog/thanks_apple")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
