#![allow(clippy::doc_markdown)]
//! `/templates` — the public, no-login template gallery.
//!
//! The conversion centerpiece of the "our legal documents are plain
//! markdown" pitch: a stretched nonprofit staffer can browse a curated,
//! client-safe subset of the workspace `templates/` tree, see the
//! notation format itself (the YAML frontmatter is rendered verbatim),
//! and download the raw `.md` to take with them. Every page carries the
//! shared [`legal_blueprint_disclaimer`] UPL guardrail and ends with a
//! "start a matter" call to action so a download is never a dead end.
//!
//! Firm-branded — this is a firm document-services surface that routes a
//! serious prospect into an opened matter. The `web` crate owns the
//! curated allow-list ([`web::template_gallery`]); these render
//! functions only see borrowed, already-vetted data.

use maud::{html, Markup};

use crate::brand::FIRM_BRAND;
use crate::components::{legal_blueprint_disclaimer, Card};
use crate::{AuthState, PageLayout};

/// One template's display fields, borrowed from the `web` crate's owned
/// gallery entry for the duration of the render.
pub struct TemplateCard<'a> {
    /// Public detail URL for this template.
    pub href: String,
    /// File stem (`form990_annual_report`) — kebab-cased for the
    /// `:name` path segment, and shown verbatim as the download
    /// filename base.
    pub name: &'a str,
    /// Human title, parsed from the template's frontmatter `title`.
    pub title: &'a str,
    /// Plain-language "what it's for".
    pub blurb: &'a str,
    /// Loud jurisdiction label (`Federal · United States`, `Nevada`).
    pub jurisdiction_label: &'a str,
    /// Bootstrap badge class denoting the jurisdiction's weight.
    pub jurisdiction_badge_class: &'a str,
}

impl TemplateCard<'_> {
    fn detail_href(&self) -> String {
        self.href.clone()
    }

    fn badge(&self) -> Markup {
        html! {
            span class={ "badge " (self.jurisdiction_badge_class) } {
                (self.jurisdiction_label)
            }
        }
    }
}

/// A single template's detail page payload.
pub struct TemplateDetail<'a> {
    pub card: TemplateCard<'a>,
    /// The YAML frontmatter block (inner, between the `---` fences),
    /// shown verbatim so the visitor sees the notation contract.
    pub frontmatter: &'a str,
    /// `/templates/:category/:name/download` — the raw `.md` route.
    pub download_href: &'a str,
    /// Where "start a matter" routes a serious prospect.
    pub start_matter_href: &'a str,
}

/// The gallery index: a short pitch, the disclaimer, and a card per
/// curated template.
#[must_use]
pub fn index(cards: &[TemplateCard<'_>], auth: AuthState) -> Markup {
    let body = html! {
        article {
            header {
                h1 { (crate::i18n::t(crate::Locale::En, "portal.template_gallery")) }
                p.lead {
                    "Our legal documents are plain-markdown "
                    em { "notation" }
                    " — no proprietary format, no lock-in. Open one to "
                    "see the format, then download the raw "
                    code { ".md" }
                    " and take it with you."
                }
                p {
                    "Want the editor experience? "
                    a href="/foundation/navigator/lsp" {
                        "Install the Neon Law Navigator language server"
                    }
                    " for live diagnostics and one-click fixes on any "
                    code { ".md" }
                    " template."
                }
            }
            (legal_blueprint_disclaimer())
            div.row."row-cols-1"."row-cols-md-2"."g-4"."mt-1" {
                @for card in cards {
                    div.col {
                        div.card."h-100" {
                            div."card-body" {
                                (card.badge())
                                h2."h5"."card-title"."mt-2" {
                                    a href=(card.detail_href()) { (card.title) }
                                }
                                p."card-text" { (card.blurb) }
                            }
                            div."card-footer"."bg-transparent" {
                                a."btn"."btn-outline-primary"."btn-sm" href=(card.detail_href()) {
                                    "View notation"
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    PageLayout::new(&crate::i18n::t(
        crate::Locale::En,
        "portal.template_gallery",
    ))
    .with_description(
        "Browse and download Neon Law's legal templates — plain-markdown \
             notation you can take with you.",
    )
    .with_brand(*FIRM_BRAND)
    .with_auth(auth)
    .render(&body)
}

/// One template's detail page: jurisdiction, plain-language summary, the
/// rendered frontmatter, a download button, and the start-a-matter CTA.
#[must_use]
pub fn detail(detail: &TemplateDetail<'_>, auth: AuthState) -> Markup {
    let card = &detail.card;
    let body = html! {
        article {
            p { a href="/templates" { "← All templates" } }
            header {
                (card.badge())
                h1."mt-2" { (card.title) }
                p.lead { (card.blurb) }
            }
            (legal_blueprint_disclaimer())
            section."mt-4" {
                h2."h5" { "The notation format" }
                p {
                    "Every Neon Law Navigator template is plain markdown with a YAML "
                    "header — the machine-readable contract the questionnaire "
                    "and workflow run on. Here is this template's, verbatim:"
                }
                pre { code { "---\n" (detail.frontmatter) "\n---" } }
            }
            div."d-flex"."gap-2"."flex-wrap"."mt-4" {
                a."btn"."btn-primary" href=(detail.download_href) {
                    "Download " (card.name) ".md"
                }
            }
            section."mt-5" {
                (Card::new(html! {
                    h2."h5" { "Want a lawyer to stand behind it?" }
                    p."mb-3" {
                        "A template is a blueprint. To have a licensed attorney "
                        "prepare, review, and sign a document for your situation, "
                        "start a matter with the firm."
                    }
                    a."btn"."btn-outline-primary" href=(detail.start_matter_href) {
                        "Start a matter"
                    }
                })
                .render())
            }
        }
    };
    PageLayout::new(card.title)
        .with_description(card.blurb)
        .with_brand(*FIRM_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{detail, index, TemplateCard, TemplateDetail};
    use crate::AuthState;

    fn card() -> TemplateCard<'static> {
        TemplateCard {
            href: "/templates/united-states/federal/irs/taxation/form990-annual-report".into(),
            name: "form990_annual_report",
            title: "IRS Form 990",
            blurb: "The annual federal information return.",
            jurisdiction_label: "Federal · United States",
            jurisdiction_badge_class: "bg-primary",
        }
    }

    #[test]
    fn index_lists_cards_with_disclaimer() {
        let html = index(&[card()], AuthState::Anonymous).into_string();
        assert!(html.contains("Template gallery"));
        // The detail link is kebab-cased even though the file stem keeps
        // its underscores.
        assert!(
            html.contains("/templates/united-states/federal/irs/taxation/form990-annual-report")
        );
        assert!(!html.contains("form990_annual_report</a>"));
        assert!(html.contains("Federal · United States"));
        // The shared disclaimer rides every gallery page.
        assert!(html.contains("not legal advice"));
    }

    #[test]
    fn detail_shows_frontmatter_disclaimer_download_and_cta() {
        let d = TemplateDetail {
            card: card(),
            frontmatter: "title: IRS Form 990\ncode: us__form_990",
            download_href:
                "/templates/united-states/federal/irs/taxation/form990-annual-report/download",
            start_matter_href: "/contact",
        };
        let html = detail(&d, AuthState::Anonymous).into_string();
        // The notation format payload.
        assert!(html.contains("code: us__form_990"));
        // The disclaimer.
        assert!(html.contains("does not create an attorney"));
        // The raw download link is the kebab-cased route…
        assert!(html.contains(
            "/templates/united-states/federal/irs/taxation/form990-annual-report/download"
        ));
        // …while the downloaded filename keeps the on-disk underscore stem.
        assert!(html.contains("Download form990_annual_report.md"));
        // The not-a-dead-end CTA.
        assert!(html.contains("Start a matter"));
        assert!(html.contains("href=\"/contact\""));
        assert!(
            !html.contains("bg-light"),
            "CTA should inherit dark-mode card tokens instead of forcing bg-light: {html}"
        );
    }
}
