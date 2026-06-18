//! `/statutes` — the public legal-code reference.
//!
//! A free, open mirror of the practice-relevant Nevada Revised Statutes
//! the firm works in, published under the Foundation brand as an
//! access-to-justice surface (open access to the law itself). Verbatim
//! text only — no commentary or interpretation (UPL safety) — wrapped in
//! a not-legal-advice banner with a link to the official source.
//!
//! The `web` crate maps `store::statutes` rows into the borrowed view
//! structs here; this module owns only presentation.

use maud::{html, Markup};

use crate::brand::FOUNDATION_BRAND;
use crate::{AuthState, PageLayout};

/// One chapter link on the index, grouped under its product.
pub struct ChapterLink<'a> {
    pub code: &'a str,
    pub number: &'a str,
    pub title: &'a str,
    pub section_count: u64,
}

/// A product heading with its chapters, for the `/statutes` index.
pub struct ProductGroup<'a> {
    pub product: &'a str,
    pub chapters: Vec<ChapterLink<'a>>,
}

/// One section rendered in a chapter or on its permalink page.
pub struct SectionView<'a> {
    /// Section number (`649.005`).
    pub number: &'a str,
    pub title: &'a str,
    /// Normalized body; subsections are `\n`-separated.
    pub body: &'a str,
    pub history_note: Option<&'a str>,
    /// Permalink to the section on the official source.
    pub source_url: &'a str,
    /// `active` or `repealed`.
    pub status: &'a str,
    /// RFC 3339 date this section was last checked against the source.
    pub last_checked_at: &'a str,
}

/// A whole chapter to render at `/statutes/nrs/:chapter`.
pub struct ChapterView<'a> {
    pub code: &'a str,
    pub number: &'a str,
    pub chapter_title: &'a str,
    /// Official-source link for the chapter as a whole.
    pub source_url: &'a str,
    pub sections: Vec<SectionView<'a>>,
}

/// The not-legal-advice / official-source banner that heads every page.
/// `source_label` + `source_url` point at the official publication.
///
/// Copy reviewed by the Legal Council (2026-06-06): states plainly that
/// this is a free public mirror of the official text, not legal advice,
/// and to verify against the official source. No comparative claims, no
/// guarantees — `marketing-copy` rules hold on this public surface.
fn disclaimer_banner(source_url: &str) -> Markup {
    html! {
        aside.statute-disclaimer role="note" {
            p {
                strong { "Reference only — not legal advice." }
                " This is a free public copy of the law, published by the "
                (crate::brand::FOUNDATION_BRAND.site_name)
                " from the text of the Nevada Revised Statutes as enacted by "
                "the Nevada Legislature. It is provided for open access to the law itself "
                "and is not a substitute for the official publication or for advice from "
                "a lawyer about your situation. Always confirm against the "
                a href=(source_url) rel="nofollow noopener" target="_blank" {
                    "official Nevada Legislature source"
                }
                " before relying on it."
            }
        }
    }
}

/// Render a chapter's sections as `\n`-separated lines into paragraphs,
/// preserving the source's subsection breaks without adding any chrome.
fn body_lines(body: &str) -> Markup {
    html! {
        @for line in body.split('\n') {
            @if !line.is_empty() {
                p.statute-line { (line) }
            }
        }
    }
}

/// `GET /statutes` — index of available chapters, grouped by product.
#[must_use]
pub fn index(groups: &[ProductGroup<'_>], auth: AuthState) -> Markup {
    let body = html! {
        header.statutes-header {
            h1 { "Nevada law, in the open" }
            p.lead {
                "A free, public copy of the Nevada Revised Statutes chapters the firm "
                "works in — the law itself, open to everyone."
            }
        }
        (disclaimer_banner("https://www.leg.state.nv.us/NRS/"))
        @if groups.is_empty() {
            p { "No chapters have been published yet. Check back soon." }
        }
        @for group in groups {
            section.statute-product-group {
                h2 { (group.product) }
                ul.statute-chapter-list {
                    @for ch in &group.chapters {
                        li {
                            a href=(format!("/statutes/nrs/{}", ch.number)) {
                                (ch.code) " " (ch.number) " — " (ch.title)
                            }
                            " "
                            span.statute-section-count {
                                "(" (ch.section_count) " sections)"
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new("Nevada Revised Statutes")
        .with_description("A free public reference copy of the Nevada Revised Statutes.")
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// `GET /statutes/nrs/:chapter` — every section of one chapter.
#[must_use]
pub fn chapter(view: &ChapterView<'_>, auth: AuthState) -> Markup {
    let heading = format!("{} {} — {}", view.code, view.number, view.chapter_title);
    let body = html! {
        nav.statute-breadcrumb aria-label="Breadcrumb" {
            a href="/statutes" { "Statutes" }
            " / "
            span { (view.code) " " (view.number) }
        }
        header.statutes-header {
            h1 { (heading) }
        }
        (disclaimer_banner(view.source_url))
        @for s in &view.sections {
            (section_article(s))
        }
    };
    PageLayout::new(&heading)
        .with_description("Verbatim Nevada Revised Statutes — reference only, not legal advice.")
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// `GET /statutes/nrs/:chapter/:section` — a single-section permalink.
#[must_use]
pub fn section(
    code: &str,
    chapter_number: &str,
    view: &SectionView<'_>,
    auth: AuthState,
) -> Markup {
    let heading = format!("{code} {}", view.number);
    let body = html! {
        nav.statute-breadcrumb aria-label="Breadcrumb" {
            a href="/statutes" { "Statutes" }
            " / "
            a href=(format!("/statutes/nrs/{chapter_number}")) { (code) " " (chapter_number) }
            " / "
            span { (view.number) }
        }
        (disclaimer_banner(view.source_url))
        (section_article(view))
    };
    PageLayout::new(&heading)
        .with_description("A single Nevada Revised Statutes section — reference only.")
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// One section as an `<article>` — number + title, body, history note,
/// status, official-source link, and the last-checked date.
fn section_article(s: &SectionView<'_>) -> Markup {
    let anchor = format!("sec-{}", s.number);
    html! {
        article.statute-section id=(anchor) {
            h2.statute-section-heading {
                a href=(format!("#{anchor}")) { (s.number) }
                " "
                (s.title)
                @if s.status == "repealed" {
                    " "
                    span.statute-repealed { "[repealed / no longer in chapter]" }
                }
            }
            div.statute-body {
                (body_lines(s.body))
            }
            @if let Some(note) = s.history_note {
                p.statute-history { (note) }
            }
            footer.statute-section-meta {
                a href=(s.source_url) rel="nofollow noopener" target="_blank" {
                    "Official source"
                }
                " · last checked " (s.last_checked_at)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{chapter, index, section, ChapterLink, ChapterView, ProductGroup, SectionView};
    use crate::AuthState;

    fn sample_section() -> SectionView<'static> {
        SectionView {
            number: "649.005",
            title: "Definitions.",
            body: "As used in this chapter, the words have the meanings ascribed.",
            history_note: Some("(Added to NRS by 1969, 829)"),
            source_url: "https://www.leg.state.nv.us/NRS/NRS-649.html#NRS649Sec005",
            status: "active",
            last_checked_at: "2026-06-07T10:00:00Z",
        }
    }

    #[test]
    fn index_groups_by_product_and_carries_banner() {
        let groups = [ProductGroup {
            product: "Nautilus",
            chapters: vec![ChapterLink {
                code: "NRS",
                number: "649",
                title: "COLLECTION AGENCIES",
                section_count: 87,
            }],
        }];
        let html = index(&groups, AuthState::Anonymous).into_string();
        assert!(html.contains("Nautilus"));
        assert!(html.contains("/statutes/nrs/649"));
        assert!(html.contains("not legal advice"));
        assert!(html.contains("leg.state.nv.us"));
    }

    #[test]
    fn chapter_renders_sections_with_official_link_and_history() {
        let view = ChapterView {
            code: "NRS",
            number: "649",
            chapter_title: "COLLECTION AGENCIES",
            source_url: "https://www.leg.state.nv.us/NRS/NRS-649.html",
            sections: vec![sample_section()],
        };
        let html = chapter(&view, AuthState::Anonymous).into_string();
        assert!(html.contains("649.005"));
        assert!(html.contains("Definitions."));
        assert!(html.contains("Official source"));
        assert!(html.contains("(Added to NRS by 1969, 829)"));
        assert!(html.contains("not legal advice"));
    }

    #[test]
    fn section_permalink_breadcrumbs_back_to_chapter() {
        let view = sample_section();
        let html = section("NRS", "649", &view, AuthState::Anonymous).into_string();
        assert!(html.contains("/statutes/nrs/649"));
        assert!(html.contains("649.005"));
        assert!(html.contains("Official source"));
    }
}
