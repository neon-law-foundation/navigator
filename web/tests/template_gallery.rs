#![allow(clippy::doc_markdown)]
//! Route tests for the public template gallery + the LSP showcase.
//!
//! Drives the router via `tower::ServiceExt::oneshot` (no socket). The
//! load-bearing claims:
//!
//! - the gallery index renders for an **unauthenticated** visitor (no
//!   login);
//! - a template downloads as verbatim `text/markdown` bytes with an
//!   attachment filename;
//! - the curated allow-list is enforced — a `confidential: true`
//!   template (Retainer) 404s rather than leaking;
//! - a detail page carries the disclaimer partial + the start-a-matter
//!   CTA;
//! - the LSP showcase renders with the install command + disclaimer.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use store::test_support::pg;
use store::Db;
use tower::ServiceExt;
use web::AppState;

async fn in_memory_db() -> Db {
    pg().await
}

async fn empty_state() -> AppState {
    web::test_support::app_state(in_memory_db().await).await
}

async fn get(state: AppState, uri: &str) -> axum::http::Response<Body> {
    web::build_router(state, std::path::Path::new(web::DEFAULT_PUBLIC_DIR))
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_string(resp: axum::http::Response<Body>) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn gallery_index_renders_for_an_anonymous_visitor() {
    // No session cookie — the conversion centerpiece must be browsable
    // without a login.
    let resp = get(empty_state().await, "/templates").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Template gallery"));
    // Leads with the federal Form 990, labeled federal.
    assert!(body.contains("IRS Form 990"));
    assert!(body.contains("Federal · United States"));
    // The two Nevada filings are loudly labeled.
    assert!(body.contains("Nevada"));
    // The disclaimer rides the page.
    assert!(body.contains("not legal advice"));
}

#[tokio::test]
async fn template_detail_has_frontmatter_disclaimer_and_start_a_matter_cta() {
    let resp = get(
        empty_state().await,
        "/templates/united-states/federal/irs/taxation/form990-annual-report",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    // The notation format itself — the rendered frontmatter.
    assert!(body.contains("code: form_990__annual_report"));
    // The UPL disclaimer partial.
    assert!(body.contains("does not create an attorney"));
    // A download must not be a dead end.
    assert!(body.contains("Start a matter"));
    assert!(body.contains("href=\"/contact\""));
    // And the raw-download link — kebab-cased, like every asset URL.
    assert!(body
        .contains("/templates/united-states/federal/irs/taxation/form990-annual-report/download"));
}

#[tokio::test]
async fn template_underscore_url_redirects_to_kebab() {
    // The on-disk stem keeps its underscores; the URL is kebab-case. A
    // request for the legacy underscore form permanently redirects to the
    // hyphenated home.
    let resp = get(
        empty_state().await,
        "/templates/united_states/federal/irs/taxation/form990_annual_report",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/templates/united-states/federal/irs/taxation/form990-annual-report"),
    );

    // The download route redirects too, preserving the trailing segment.
    let resp = get(
        empty_state().await,
        "/templates/united_states/federal/irs/taxation/form990_annual_report/download",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/templates/united-states/federal/irs/taxation/form990-annual-report/download"),
    );
}

#[tokio::test]
async fn legacy_gallery_url_redirects_to_deep_taxonomy_path() {
    let resp = get(
        empty_state().await,
        "/templates/nonprofit/form990-annual-report",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/templates/united-states/federal/irs/taxation/form990-annual-report"),
    );
}

#[tokio::test]
async fn template_downloads_verbatim_markdown_as_an_attachment() {
    let resp = get(
        empty_state().await,
        "/templates/united-states/federal/irs/taxation/form990-annual-report/download",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "text/markdown; charset=utf-8"
    );
    // The downloaded file keeps its on-disk underscore name (the bytes a
    // git reader sees), even though the route that serves it is kebab.
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CONTENT_DISPOSITION)
            .unwrap(),
        "attachment; filename=\"form990_annual_report.md\""
    );
    let body = body_string(resp).await;
    // Verbatim bytes: the same source the git reader sees, frontmatter
    // fence and all.
    let source = include_str!(
        "../../notation_templates/united_states/federal/irs/taxation/form990_annual_report.md"
    );
    assert_eq!(body, source);
}

#[tokio::test]
async fn confidential_template_404s_rather_than_leaking() {
    // Retainer is `confidential: true` and not on the allow-list. The
    // route must 404 — never serve it by guessing the path.
    let resp = get(empty_state().await, "/templates/engagements/retainer").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = get(
        empty_state().await,
        "/templates/engagements/retainer/download",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn off_list_template_path_404s() {
    let resp = get(empty_state().await, "/templates/nonprofit/MadeUp").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn lsp_showcase_renders_with_install_command_and_disclaimer() {
    // The LSP page lives under the Neon Law Navigator package hub now.
    let resp = get(empty_state().await, "/foundation/navigator/lsp").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("cargo install --path lsp"));
    assert!(body.contains("source.fixAll"));
    assert!(body.contains("Zed"));
    for editor in ["VS Code", "Neovim", "Helix", "Emacs"] {
        assert!(!body.contains(editor), "unexpected editor {editor}");
    }
    // Disclaimer rides this page too.
    assert!(body.contains("not legal advice"));
}

#[tokio::test]
async fn old_lsp_url_permanently_redirects_to_the_package_page() {
    // `/lsp` was the old top-level URL; keep it as a permanent redirect so
    // existing links never dead-end.
    let resp = get(empty_state().await, "/lsp").await;
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers().get("location").unwrap(),
        "/foundation/navigator/lsp"
    );
}
