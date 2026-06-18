#![allow(clippy::doc_markdown)]
//! Integration tests for the CSRF middleware that gates admin
//! form-encoded POSTs.
//!
//! The pattern: encode a valid session cookie ourselves (no IdP
//! round-trip needed), then exercise the middleware paths — happy
//! POST with matching `_csrf`, missing `_csrf`, mismatched `_csrf`,
//! and missing session (passthrough). The form HTML itself
//! includes the hidden input when the session is attached.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use web::{session::SESSION_COOKIE_NAME, AppState, SessionData, SessionStore};

fn sessions() -> SessionStore {
    SessionStore::new("csrf-test-session-key")
}

async fn state(s: SessionStore) -> AppState {
    let db = store::test_support::pg().await;
    store::migrate(&db).await.unwrap();
    AppState {
        sessions: s,
        ..web::test_support::app_state(db).await
    }
}

async fn body(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Build a `Cookie:` header value carrying a freshly-encoded
/// session, and return the encoded session's CSRF token so the
/// caller can include it in form bodies.
fn fresh_session_cookie(s: &SessionStore) -> (String, String) {
    let session = SessionData::fresh("nick@neonlaw.com", store::entity::person::Role::Admin);
    let token = session.csrf_token.clone();
    let cookie_value = s.encode(&session);
    (format!("{SESSION_COOKIE_NAME}={cookie_value}"), token)
}

#[tokio::test]
async fn admin_form_renders_csrf_hidden_input_when_session_present() {
    let store = sessions();
    let app = web::build_router(
        state(store.clone()).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let (cookie, token) = fresh_session_cookie(&store);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/portal/admin/people/new")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body(resp).await;
    assert!(html.contains("name=\"_csrf\""));
    assert!(html.contains(&format!("value=\"{token}\"")));
    assert!(html.contains("type=\"hidden\""));
}

#[tokio::test]
async fn admin_post_with_session_and_matching_csrf_redirects() {
    let store = sessions();
    let app = web::build_router(
        state(store.clone()).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let (cookie, token) = fresh_session_cookie(&store);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .body(Body::from(format!(
                    "_csrf={token}&name=Libra&email=libra%40example.com"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        resp.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));
}

#[tokio::test]
async fn admin_post_with_session_and_missing_csrf_returns_403() {
    let store = sessions();
    let app = web::build_router(
        state(store.clone()).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let (cookie, _token) = fresh_session_cookie(&store);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .body(Body::from("name=Libra&email=libra%40example.com"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_post_with_session_and_wrong_csrf_returns_403() {
    let store = sessions();
    let app = web::build_router(
        state(store.clone()).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let (cookie, _token) = fresh_session_cookie(&store);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .body(Body::from(
                    "_csrf=NOT_THE_REAL_TOKEN&name=Libra&email=libra%40example.com",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_post_without_session_passes_through_csrf_layer() {
    // The previous suite already proves this works — we re-assert
    // here so a future regression of the "no session = passthrough"
    // behavior fails this file too.
    let store = sessions();
    let app = web::build_router(
        state(store).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
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
        resp.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));
}

#[tokio::test]
async fn admin_post_with_tampered_session_cookie_passes_through() {
    // A tampered/expired session cookie fails to decode → middleware
    // treats request as anonymous → CSRF layer no-ops → handler
    // succeeds in the dev/test path (no auth enforced).
    let store = sessions();
    let app = web::build_router(
        state(store).await,
        std::path::Path::new(web::DEFAULT_PUBLIC_DIR),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/portal/admin/people")
                .header("content-type", "application/x-www-form-urlencoded")
                .header(
                    "cookie",
                    format!("{SESSION_COOKIE_NAME}=this-is-not-a-valid-signed-cookie"),
                )
                .body(Body::from("name=Libra&email=libra%40example.com"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        resp.status(),
        StatusCode::SEE_OTHER | StatusCode::TEMPORARY_REDIRECT
    ));
}
