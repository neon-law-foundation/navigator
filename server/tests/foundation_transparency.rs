//! `/transparency` — the Foundation's disclosure surface.
//!
//! The index, the governance documents, and the quarterly board minutes are
//! served from the bundled `server/content/foundation/` tree. These tests drive
//! the real router with the real content, so a document that stops loading —
//! or a category that stops being reachable at its own path — fails here rather
//! than 404ing in production.
//!
//! The surface reads for a signed-in visitor. It moved behind the session
//! boundary when the Foundation host narrowed its anonymous face to its talks,
//! so every request here carries a session; `an_anonymous_reader_meets_the_login_door`
//! is what pins the boundary itself.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use portal::{AppState, TransparencyIndex};
use store::test_support::mem_surreal;
use tower::ServiceExt;

/// Router state carrying the bundled Foundation documents, which is what the
/// real process loads at boot (`portal::hosting`).
async fn state_with_bundled_foundation_docs() -> AppState {
    let transparency =
        portal::transparency::load_dir(std::path::Path::new(portal::DEFAULT_FOUNDATION_DIR))
            .expect("bundled foundation content loads");
    AppState {
        transparency,
        ..portal::test_support::app_state(mem_surreal().await).await
    }
}

/// A session for the disclosure surface. Deliberately a `client` — these
/// pages read for any authenticated person, and the weakest role proves it.
fn reader_cookie() -> String {
    let session = portal::SessionData::fresh("transparency-reader", store::persons::Role::Client);
    format!(
        "{}={}",
        portal::session::SESSION_COOKIE_NAME,
        portal::SessionStore::new(portal::test_support::TEST_SESSION_KEY).encode(&session)
    )
}

async fn get(uri: &str) -> (StatusCode, String) {
    let app = server::neon_router(
        state_with_bundled_foundation_docs().await,
        std::path::Path::new(portal::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(axum::http::header::COOKIE, reader_cookie())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn an_anonymous_reader_meets_the_login_door() {
    // The boundary itself. Every other test here carries a session, so this
    // is the one that would fail if the surface silently went public again.
    let app = server::neon_router(
        state_with_bundled_foundation_docs().await,
        std::path::Path::new(portal::DEFAULT_PUBLIC_DIR),
    );
    for uri in [
        "/foundation/transparency",
        "/foundation/transparency/bylaws",
        "/foundation/transparency/minutes/26q2",
    ] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER, "{uri}");
    }
}

#[tokio::test]
async fn the_transparency_index_lists_every_bundled_document() {
    let (status, body) = get("/foundation/transparency").await;
    assert_eq!(status, StatusCode::OK);

    // The required §6104(d) disclosures and the voluntary ones are separate
    // sections; the page must never imply the voluntary set is compelled.
    assert!(body.contains("Required public disclosures"), "{body}");
    assert!(body.contains("Published voluntarily"), "{body}");
    // The required disclosures are named, and offered on request. None of
    // them is linked, because none of them is published yet — a §6104(d)
    // page that links to a 404 is worse than one that says "ask us".
    assert!(body.contains("IRS determination letter"), "{body}");
    assert!(!body.contains("determination-letter.pdf"), "{body}");

    // Every governance document in the bundled tree is linked by its own path.
    assert!(
        body.contains("href=\"/foundation/transparency/bylaws\""),
        "{body}"
    );
    assert!(
        body.contains("href=\"/foundation/transparency/conflict-of-interest\""),
        "{body}"
    );
    // Minutes live under their own prefix so a quarter can never collide with
    // a governance slug.
    assert!(
        body.contains("href=\"/foundation/transparency/minutes/26q2\""),
        "{body}"
    );

    // Foundation chrome, not the firm's.
    assert!(!body.contains(">Services</summary>"), "{body}");
}

#[tokio::test]
async fn each_governance_document_serves_its_own_page() {
    for (slug, title) in [
        ("bylaws", "Bylaws"),
        ("conflict-of-interest", "Conflict of Interest Policy"),
    ] {
        let (status, body) = get(&format!("/foundation/transparency/{slug}")).await;
        assert_eq!(status, StatusCode::OK, "{slug}");
        // The title is dynamic, so Dioxus SSR wraps it in hydration comments
        // (`<h1><!--node-id7-->Bylaws<!--#--></h1>`). Assert the heading element
        // and its text separately rather than as one `<h1>{title}</h1>` run.
        assert!(body.contains("<h1>"), "{slug}: {body}");
        assert!(body.contains(&format!(">{title}<")), "{slug}: {body}");
        // The rendered markdown body, not the raw source.
        assert!(body.contains("What the"), "{slug}: {body}");
        // Every document offers the way back to the hub.
        assert!(
            body.contains("href=\"/foundation/transparency\""),
            "{slug}: {body}"
        );
    }
}

#[tokio::test]
async fn the_quarterly_minutes_serve_under_the_minutes_prefix() {
    let (status, body) = get("/foundation/transparency/minutes/26q2").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Board Meeting Minutes — Q2 2026"), "{body}");
    // Directors are recorded by initial in the public minutes.
    assert!(body.contains("Directors present:"), "{body}");
}

#[tokio::test]
async fn a_category_never_answers_for_the_other_category() {
    // A minutes slug is not reachable as a governance document, and a
    // governance slug is not reachable under `minutes/`. Without the category
    // check both handlers would answer for any slug in the index.
    let (status, _) = get("/foundation/transparency/26q2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = get("/foundation/transparency/minutes/bylaws").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_unknown_slug_is_not_found() {
    let (status, _) = get("/foundation/transparency/no-such-document").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = get("/foundation/transparency/minutes/99q9").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_sitemap_leaves_the_gated_disclosures_out() {
    // They read only for a signed-in visitor, so advertising them would point
    // every crawler at a login redirect.
    let (status, body) = get("/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    for path in [
        "/foundation/transparency",
        "/foundation/transparency/bylaws",
        "/foundation/transparency/conflict-of-interest",
        "/foundation/transparency/minutes/26q2",
    ] {
        assert!(
            !body.contains(&format!("{path}<")),
            "{path} must not be advertised: {body}"
        );
    }
}

#[tokio::test]
async fn an_empty_index_still_serves_the_hub() {
    // A fork with no bundled Foundation content boots and serves the required
    // disclosures; only the voluntary lists go quiet.
    let state = AppState {
        transparency: TransparencyIndex::empty(),
        ..portal::test_support::app_state(mem_surreal().await).await
    };
    let app = server::neon_router(state, std::path::Path::new(portal::DEFAULT_PUBLIC_DIR));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/foundation/transparency")
                .header(axum::http::header::COOKIE, reader_cookie())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        body.contains("Governance documents will be posted here soon."),
        "{body}"
    );
}
