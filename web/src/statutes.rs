//! `/statutes` — the public Nevada Revised Statutes reference surface.
//!
//! Thin handlers over the insert-only `store::statutes` reads, rendering
//! the Foundation-branded views in [`views::pages::statutes`]. Open
//! access to the law itself is the point: verbatim text only — no
//! commentary — with a not-legal-advice banner and an official-source
//! link on every page.

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use maud::Markup;

use store::statutes::CurrentSection;
use store::Db;
use views::pages::statutes as view;

use crate::MaybeAuth;

/// Code we publish. Single jurisdiction / code in v1.
const CODE: &str = "NRS";

/// `GET /statutes` — index of published chapters, grouped by the product
/// each supports, in the configured product order.
pub async fn index(State(db): State<Db>, MaybeAuth(auth): MaybeAuth) -> Markup {
    let summaries = store::statutes::chapters(&db, CODE)
        .await
        .unwrap_or_default();

    // Product order = first appearance in the scraper's chapter config,
    // so the index reads in a stable, intentional order.
    let mut product_order: Vec<&'static str> = Vec::new();
    for spec in statutes::CHAPTERS {
        if !product_order.contains(&spec.product) {
            product_order.push(spec.product);
        }
    }

    let groups: Vec<view::ProductGroup<'_>> = product_order
        .iter()
        .filter_map(|&product| {
            let chapters: Vec<view::ChapterLink<'_>> = summaries
                .iter()
                .filter(|s| statutes::product_for(&s.chapter) == Some(product))
                .map(|s| view::ChapterLink {
                    code: &s.code,
                    number: &s.chapter,
                    title: &s.chapter_title,
                    section_count: s.section_count,
                })
                .collect();
            (!chapters.is_empty()).then_some(view::ProductGroup { product, chapters })
        })
        .collect();

    view::index(&groups, auth)
}

/// `GET /statutes/nrs/:chapter` — every section of one chapter. Unknown
/// or not-yet-scraped chapters 404.
pub async fn chapter(
    State(db): State<Db>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath(chapter): AxumPath<String>,
) -> impl IntoResponse {
    let sections = store::statutes::sections_in_chapter(&db, CODE, &chapter)
        .await
        .unwrap_or_default();
    if sections.is_empty() {
        return (StatusCode::NOT_FOUND, views::not_found_page()).into_response();
    }

    let chapter_title = sections[0].statute.chapter_title.as_str();
    // The chapter-level official link is a section permalink with its
    // `#anchor` stripped.
    let chapter_source = sections[0]
        .statute
        .source_url
        .split('#')
        .next()
        .unwrap_or(&sections[0].statute.source_url);

    let section_views: Vec<view::SectionView<'_>> = sections.iter().map(to_section_view).collect();
    let chapter_view = view::ChapterView {
        code: CODE,
        number: &chapter,
        chapter_title,
        source_url: chapter_source,
        sections: section_views,
    };
    view::chapter(&chapter_view, auth).into_response()
}

/// `GET /statutes/nrs/:chapter/:section` — a single-section permalink.
/// 404s when the section is unknown or doesn't belong to `:chapter`.
pub async fn section(
    State(db): State<Db>,
    MaybeAuth(auth): MaybeAuth,
    AxumPath((chapter, section)): AxumPath<(String, String)>,
) -> impl IntoResponse {
    match store::statutes::section(&db, CODE, &section).await {
        Ok(Some(cur)) if cur.statute.chapter == chapter => {
            let v = to_section_view(&cur);
            view::section(CODE, &chapter, &v, auth).into_response()
        }
        _ => (StatusCode::NOT_FOUND, views::not_found_page()).into_response(),
    }
}

/// Borrow a store [`CurrentSection`] into the view's [`view::SectionView`].
fn to_section_view(cur: &CurrentSection) -> view::SectionView<'_> {
    view::SectionView {
        number: &cur.statute.section,
        title: &cur.revision.section_title,
        body: &cur.revision.body,
        history_note: cur.revision.history_note.as_deref(),
        source_url: &cur.statute.source_url,
        status: cur.statute.status.as_str(),
        last_checked_at: date_part(&cur.statute.last_checked_at),
    }
}

/// The `YYYY-MM-DD` head of an RFC 3339 timestamp, for a clean
/// "last checked" date. Falls back to the whole string if it's shorter.
fn date_part(rfc3339: &str) -> &str {
    rfc3339.get(..10).unwrap_or(rfc3339)
}
