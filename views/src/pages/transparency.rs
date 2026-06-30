//! `/foundation/transparency` — the Foundation's public-disclosure hub, and
//! `/foundation/transparency/:slug` — one governance document or quarterly
//! board-minutes page.
//!
//! Rendered under the Foundation brand. The page separates the documents a
//! 501(c)(3) must make public under IRC §6104(d) — the exemption application,
//! the IRS determination letter, and the annual returns — from the records the
//! Foundation publishes *voluntarily* (bylaws, the conflict of interest
//! policy, and board minutes). Federal law does not require those latter
//! documents to be public, so the page is careful never to claim it does.
//!
//! These functions render loaded view models only; the content loader lives in
//! [`web::transparency`](../../../web/src/transparency.rs).

use maud::{html, Markup, PreEscaped};

use crate::brand::FOUNDATION_BRAND;
use crate::{AuthState, PageLayout};

/// One document as it appears in a list — a title, a one-line description, and
/// the link to open it. Owns its strings so the handler can build it from the
/// loaded index without lifetime juggling.
pub struct DocLink {
    pub href: String,
    pub title: String,
    pub description: String,
}

/// View model for the transparency index.
pub struct IndexContent<'a> {
    /// Path to the IRS determination letter PDF served from `/public/`.
    pub determination_letter_href: &'a str,
    /// Governance documents (bylaws, conflict policy), in display order.
    pub governance: &'a [DocLink],
    /// Quarterly board minutes, newest first.
    pub minutes: &'a [DocLink],
}

/// The full content of one transparency document page.
pub struct DocContent<'a> {
    pub title: &'a str,
    /// One-line front-matter summary, used for the page's
    /// `<meta name="description">` (falls back to a generic line when empty).
    pub description: &'a str,
    /// Canonical path for this document, e.g.
    /// `/foundation/transparency/bylaws` — emitted as `<link rel="canonical">`.
    pub canonical_path: &'a str,
    /// Rendered HTML body (NOT raw markdown).
    pub body_html: &'a str,
}

#[must_use]
pub fn render_index(content: &IndexContent<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.transparency style="max-width: 70ch; margin-inline: auto;" {
            h1 { "Transparency" }
            p {
                "The " (FOUNDATION_BRAND.site_name) " is a Nevada nonprofit corporation "
                "recognized by the IRS as a 501(c)(3) tax-exempt organization. We publish "
                "here the documents the law requires us to make public — and, going further, "
                "the governance records we choose to share."
            }

            section.transparency-required {
                h2 { "Required public disclosures" }
                p.text-body-secondary {
                    "Federal law (Internal Revenue Code §6104(d)) requires a 501(c)(3) to make "
                    "these available for public inspection. Posting them here also satisfies "
                    "that duty for anyone who asks."
                }
                ul.transparency-list {
                    li {
                        a href=(content.determination_letter_href) { "IRS determination letter (PDF)" }
                        " — the letter recognizing the Foundation's 501(c)(3) status."
                    }
                    li {
                        "Exemption application (IRS Form 1023) and supporting documents — "
                        "available on request through the "
                        a href="/contact" { "contact page" }
                        " while we prepare the filing for publication here."
                    }
                    li {
                        "Annual returns (IRS Form 990-series) — the three most recent returns "
                        "will be posted here once filed."
                    }
                }
            }

            section.transparency-voluntary {
                h2 { "Published voluntarily" }
                p.text-body-secondary {
                    "The documents below are " em { "not" } " required to be public. The "
                    "Foundation publishes them because transparency about how it governs "
                    "itself is part of the mission."
                }
                @if content.governance.is_empty() {
                    p { "Governance documents will be posted here soon." }
                } @else {
                    ul.transparency-list {
                        @for doc in content.governance {
                            li {
                                a href=(doc.href) { (doc.title) }
                                @if !doc.description.is_empty() { " — " (doc.description) }
                            }
                        }
                    }
                }
                p {
                    "The Foundation also publishes the standard agreements it uses to engage "
                    "its team — an at-will employment agreement and an independent-contractor "
                    "agreement — as open "
                    a href="/foundation/notations" { "Notations" }
                    " any nonprofit can reuse."
                }
            }

            section.transparency-minutes id="minutes" {
                h2 { "Board meeting minutes" }
                p.text-body-secondary {
                    "Minutes of the Foundation's regular quarterly board meetings. Approved "
                    "minutes are published as they are finalized; quarters not yet recorded "
                    "appear as reserved placeholders."
                }
                @if content.minutes.is_empty() {
                    p { "Board minutes will be posted here soon." }
                } @else {
                    ul.transparency-list.transparency-minutes-list {
                        @for doc in content.minutes {
                            li { a href=(doc.href) { (doc.title) } }
                        }
                    }
                }
            }

            p.text-body-secondary {
                "Looking for a record that isn't posted here? Request it through the "
                a href="/contact" { "contact page" } "."
            }
        }
    };
    PageLayout::new("Transparency")
        .with_description(
            "Public disclosures of the Neon Law Foundation — the IRS determination letter, \
             bylaws, conflict of interest policy, and board meeting minutes.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_canonical_path("/foundation/transparency")
        .render(&body)
}

#[must_use]
pub fn render_doc(content: &DocContent<'_>, auth: AuthState) -> Markup {
    let body = html! {
        article.transparency-doc style="max-width: 65ch; margin-inline: auto;" {
            p { a href="/foundation/transparency" { "← All Foundation documents" } }
            h1 { (content.title) }
            (PreEscaped(content.body_html))
        }
    };
    PageLayout::new(content.title)
        .with_description(if content.description.is_empty() {
            "A Neon Law Foundation transparency document."
        } else {
            content.description
        })
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .with_canonical_path(content.canonical_path)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render_doc, render_index, DocContent, DocLink, IndexContent};
    use crate::brand::FOUNDATION_BRAND;

    fn link(slug: &str, title: &str, desc: &str) -> DocLink {
        DocLink {
            href: format!("/foundation/transparency/{slug}"),
            title: title.to_string(),
            description: desc.to_string(),
        }
    }

    #[test]
    fn index_renders_under_foundation_brand() {
        let content = IndexContent {
            determination_letter_href: "/public/foundation/determination-letter.pdf",
            governance: &[],
            minutes: &[],
        };
        let html = render_index(&content, crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Transparency</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn index_separates_required_from_voluntary_and_does_not_overclaim() {
        let content = IndexContent {
            determination_letter_href: "/public/foundation/determination-letter.pdf",
            governance: &[],
            minutes: &[],
        };
        let html = render_index(&content, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("Required public disclosures"));
        assert!(html.contains("Published voluntarily"));
        // The determination letter is linked as a required disclosure.
        assert!(html.contains("href=\"/public/foundation/determination-letter.pdf\""));
        // The voluntary section must say these are NOT required — never claim
        // bylaws/minutes are legally mandated.
        assert!(html.contains("<em>not</em> required to be public"));
    }

    #[test]
    fn index_lists_governance_and_minutes_links() {
        let governance = vec![
            link("bylaws", "Bylaws", "How the board operates."),
            link(
                "conflict-of-interest",
                "Conflict of Interest Policy",
                "Competing interests.",
            ),
        ];
        let minutes = vec![
            link("minutes-2026-q2", "Board Meeting Minutes — Q2 2026", ""),
            link("minutes-2021-q1", "Board Meeting Minutes — Q1 2021", ""),
        ];
        let content = IndexContent {
            determination_letter_href: "/public/foundation/determination-letter.pdf",
            governance: &governance,
            minutes: &minutes,
        };
        let html = render_index(&content, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("href=\"/foundation/transparency/bylaws\""));
        assert!(html.contains("href=\"/foundation/transparency/conflict-of-interest\""));
        assert!(html.contains("href=\"/foundation/transparency/minutes-2026-q2\""));
        assert!(html.contains("Board Meeting Minutes — Q1 2021"));
        // Links out to the reusable agreement Notations.
        assert!(html.contains("href=\"/foundation/notations\""));
    }

    #[test]
    fn doc_renders_title_body_and_backlink() {
        let doc = DocContent {
            title: "Bylaws",
            description: "How the board governs the Foundation.",
            canonical_path: "/foundation/transparency/bylaws",
            body_html: "<p>Article I. Purpose.</p>",
        };
        let html = render_doc(&doc, crate::AuthState::Anonymous).into_string();
        assert!(html.contains(&format!(
            "<title>{} | Bylaws</title>",
            FOUNDATION_BRAND.site_name
        )));
        assert!(html.contains("<p>Article I. Purpose.</p>"));
        assert!(html.contains("href=\"/foundation/transparency\""));
    }

    #[test]
    fn doc_sets_front_matter_description_and_canonical_path() {
        // Per-document pages get the richer front-matter description as their
        // `<meta description>` and a canonical link to their own slug — not the
        // bare title (regression guard for the meta/canonical review fixes).
        let doc = DocContent {
            title: "Bylaws",
            description: "How the board governs the Foundation.",
            canonical_path: "/foundation/transparency/bylaws",
            body_html: "<p>body</p>",
        };
        let html = render_doc(&doc, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(
                "<meta name=\"description\" content=\"How the board governs the Foundation.\">"
            ),
            "per-doc meta description should be the front-matter summary, got: {html}"
        );
        // `with_canonical_path` declares the page's canonical URL through the
        // hreflang-alternate links (the same mechanism `render_index` uses).
        assert!(
            html.contains(
                "<link rel=\"alternate\" hreflang=\"en\" href=\"/foundation/transparency/bylaws\">"
            ),
            "per-doc page should declare its own canonical URL, got: {html}"
        );
    }
}
