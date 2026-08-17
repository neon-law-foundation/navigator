//! What the site's retired-path table answers, and where each hop lands.
//!
//! The table is only half of the public surface — the pages themselves are
//! Dioxus routers in `neon::pages` and `neon::firm_pages`. This file pins the
//! half that is a table: every URL that was live before the consolidation and
//! now `301`s to its replacement. The two halves together are covered against
//! the real composition in `server/tests/routes.rs`.
//!
//! A redirect is only worth keeping if it lands somewhere real, so these
//! assert the `Location` rather than merely that the path is not a `404`. A
//! `301` to a page this binary does not serve is a dead end wearing a
//! redirect's clothes, and that is exactly the failure a consolidation
//! introduces.

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use store::test_support::mem_surreal;
use tower::ServiceExt;

async fn state() -> portal::AppState {
    portal::test_support::app_state(mem_surreal().await).await
}

async fn anonymous_get(app: &axum::Router, path: &str) -> Response<Body> {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// The `Location` a response redirects to, or `None` if it is not a redirect.
fn location(response: &Response<Body>) -> Option<&str> {
    response
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|value| value.to_str().ok())
}

/// The Foundation's former root URLs each `301` to their `/foundation`
/// replacement.
///
/// These were live pages for as long as the Foundation had a host of its own,
/// so they are the most-linked retired URLs on the site. The firm holds the
/// root now, so a visitor who follows an old `neonlaw.org/notations` link has
/// to be carried across rather than dropped on a firm page or a `404`.
#[tokio::test]
async fn the_foundations_former_root_urls_redirect_beneath_foundation() {
    let app = neon::retired_path_routes().with_state(state().await);

    for (from, to) in [
        ("/mission", "/foundation/mission"),
        ("/education", "/foundation/education"),
        ("/legal-aid", "/foundation/legal-aid"),
        ("/attorneys", "/foundation/attorneys"),
        ("/notations", "/foundation/notations"),
        ("/transparency", "/foundation/transparency"),
        ("/transparency/bylaws", "/foundation/transparency/bylaws"),
        (
            "/transparency/minutes/2026-q1",
            "/foundation/transparency/minutes/2026-q1",
        ),
        ("/show-and-tell", "/foundation/show-and-tell"),
        ("/show-and-tell/june", "/foundation/show-and-tell/june"),
    ] {
        let response = anonymous_get(&app, from).await;
        assert_eq!(
            response.status(),
            StatusCode::PERMANENT_REDIRECT,
            "{from} must be answered as a permanent redirect"
        );
        assert_eq!(
            location(&response),
            Some(to),
            "{from} must land on {to}, not on a firm page or a 404"
        );
    }
}

/// `/foundation` is a page now, not a redirect.
///
/// It `301`ed to `/` for as long as the Foundation was canonical at the site
/// root. Reinstating that redirect would bounce the Foundation's own home page
/// onto the firm's, which is the single most damaging way this consolidation
/// could regress: the nonprofit would silently stop having a front door.
#[tokio::test]
async fn the_foundation_home_is_not_a_redirect() {
    let app = neon::retired_path_routes().with_state(state().await);

    assert_eq!(
        anonymous_get(&app, "/foundation").await.status(),
        StatusCode::NOT_FOUND,
        "/foundation belongs to the Dioxus half of the surface, not the redirect table"
    );
}

/// The retired Nebula surface lands on the catalogs that replaced it, and every
/// destination is relative.
///
/// While the firm and the Foundation were separate deployments, a hop between
/// them had to be an absolute URL onto the other host. One binary serves both
/// now, so an absolute redirect here would send a visitor out to DNS and back
/// for a page already in front of them — and would break outright on any
/// deployment not answering to that hostname.
#[tokio::test]
async fn the_retired_nebula_surface_redirects_relatively() {
    let app = neon::retired_path_routes().with_state(state().await);

    for (from, to) in [
        ("/foundation/nebula", "/foundation"),
        ("/foundation/workshops", "/workshops"),
        (
            "/foundation/workshops/navigator",
            "/workshops/use-the-navigator",
        ),
        (
            "/foundation/nebula/presentations/rust-in-peace",
            "/presentations/rust-in-peace",
        ),
        (
            "/foundation/nebula/show-and-tell/june",
            "/foundation/show-and-tell/june",
        ),
    ] {
        let response = anonymous_get(&app, from).await;
        assert_eq!(
            response.status(),
            StatusCode::PERMANENT_REDIRECT,
            "{from} must be answered as a permanent redirect"
        );
        let destination = location(&response).expect("a redirect carries a Location");
        assert_eq!(destination, to, "{from} must land on {to}");
        assert!(
            destination.starts_with('/'),
            "one host serves everything, so {from} redirects relatively: {destination}"
        );
    }
}

/// The redirect table owns retired URLs and nothing else. A live page that
/// appeared here would shadow the Dioxus router that actually renders it, and
/// the visitor would get a redirect loop instead of the page.
#[tokio::test]
async fn the_redirect_table_owns_no_live_page() {
    let app = neon::retired_path_routes().with_state(state().await);

    for path in [
        "/",
        "/services",
        "/litigation",
        "/blog",
        "/contact",
        "/team",
    ] {
        assert_eq!(
            anonymous_get(&app, path).await.status(),
            StatusCode::NOT_FOUND,
            "{path} is a live page, so the redirect table must not own it"
        );
    }
}
