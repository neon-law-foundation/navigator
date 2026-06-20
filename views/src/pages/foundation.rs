//! `/foundation` — the foundation landing page.
//!
//! Same shape as the home page: caller supplies a pre-rendered
//! HTML body (loaded from `content/marketing/foundation.md`). The
//! difference is the brand: this page renders under
//! [`FOUNDATION_BRAND`], so the header swaps to the foundation
//! nav and the title reads "… — <foundation brand name>".
//!
//! [`FOUNDATION_BRAND`]: crate::brand::FOUNDATION_BRAND

use maud::{html, Markup, PreEscaped};

use crate::brand::{foundation_email, foundation_github_url, FOUNDATION_BRAND};
use crate::components::ExternalLink;
use crate::{AuthState, PageLayout};

pub struct FoundationContent<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub body_html: &'a str,
}

/// Lazily-leaked body HTML so the env-driven foundation brand name
/// and email are resolved once per process, not once per `default()`
/// call.
static DEFAULT_BODY_HTML: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    let email = foundation_email();
    Box::leak(
        format!(
            "<p>The {} is a 501(c)(3) nonprofit. \
             Email <a href=\"mailto:{email}\">{email}</a>.</p>",
            FOUNDATION_BRAND.site_name,
        )
        .into_boxed_str(),
    )
});

/// Default `FoundationContent` used by tests and as a safety net when
/// the marketing markdown fails to load. Body HTML is built from the
/// foundation brand name so the OSS default reads "Navigator
/// Foundation" rather than the historical `NeonLaw` brand.
impl Default for FoundationContent<'_> {
    fn default() -> Self {
        Self {
            title: FOUNDATION_BRAND.site_name,
            description: "Open-source access-to-justice tools and attorney CLEs.",
            body_html: *DEFAULT_BODY_HTML,
        }
    }
}

/// The three figures that carry the page's "why": the scale of the
/// unmet legal need (92% / 5.1B) set against the Foundation's answer —
/// tooling anyone can run for free. These are public-interest research
/// figures and the Foundation's own open-source posture, not
/// per-deployment values, so they live beside the hero that frames
/// them; `content/marketing/foundation.md` cites the same numbers in
/// prose for the long-form narrative and SEO text.
const JUSTICE_GAP_STATS: &[(&str, &str, &str)] = &[
    (
        "92%",
        "of low-income Americans' civil legal problems get inadequate or no legal help",
        "LSC Justice Gap Report, 2022",
    ),
    (
        "5.1B",
        "people worldwide lack meaningful access to justice",
        "World Justice Project, 2023",
    ),
    (
        "$0",
        "to self-host every Navigator tool — the rule engine, CLI, and MCP server are free and open source",
        "Apache-2.0 / MIT",
    ),
];

#[must_use]
pub fn render(content: &FoundationContent<'_>, auth: AuthState) -> Markup {
    let email = foundation_email();
    let mailto = format!("mailto:{email}");
    let body = html! {
        // Hero — the "why" before anything scrolls. A Bootstrap
        // `bg-body-tertiary` band so it tracks the page surface and
        // stays dark-mode-safe via `data-bs-theme="auto"`.
        section."p-4"."p-md-5"."mb-4"."rounded-3"."bg-body-tertiary" {
            p."text-uppercase"."fw-semibold"."text-body-secondary"."small"."mb-2" {
                (FOUNDATION_BRAND.site_name) " · 501(c)(3)"
            }
            h1."display-5"."fw-bold" { (content.title) }
            p."lead"."col-lg-9"."mb-4" {
                "We build open-source software — and train attorneys to wield it — so that "
                "the rights already written into law actually reach the people they belong to."
            }
            div."d-flex"."flex-wrap"."gap-2" {
                a."btn"."btn-primary"."btn-lg" href=(mailto) { "Email the Foundation" }
                @if let Some(gh) = foundation_github_url() {
                    (ExternalLink::new(gh)
                        .with_class("btn btn-outline-secondary btn-lg")
                        .render(html! { "Browse the open-source code" }))
                }
                a."btn"."btn-outline-secondary"."btn-lg" href="/foundation/workshops/navigator" {
                    "Open the Navigator workshop"
                }
            }
        }

        // Why it matters — the justice gap as scannable stat cards:
        // the scale of the need, then our free, open answer.
        section."mb-5" {
            div."row"."row-cols-1"."row-cols-md-3"."g-4" {
                @for (figure, claim, source) in JUSTICE_GAP_STATS {
                    div."col" {
                        div."card"."h-100"."border-0"."shadow-sm" {
                            div."card-body" {
                                p."display-6"."fw-bold"."text-primary"."mb-2" { (figure) }
                                p."card-text"."mb-0" { (claim) }
                            }
                            div."card-footer"."bg-transparent"."border-0"."small"."text-body-secondary" {
                                (source)
                            }
                        }
                    }
                }
            }
        }

        // The full narrative, loaded verbatim from the marketing
        // markdown — markdown stays the content source of record.
        article."foundation-prose" {
            (PreEscaped(content.body_html))
        }
    };
    PageLayout::new("Foundation")
        .with_description(content.description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, FoundationContent};
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn foundation_renders_layout_under_foundation_brand() {
        let html = render(&FoundationContent::default(), crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Foundation</title>",
            FOUNDATION_BRAND.site_name
        )));
    }

    #[test]
    fn foundation_uses_caller_body_html_verbatim() {
        let content = FoundationContent {
            title: "T",
            description: "D",
            body_html: "<h2>Mission</h2><p>Para.</p>",
        };
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(html.contains("<h2>Mission</h2>"));
        assert!(html.contains("<p>Para.</p>"));
    }

    #[test]
    fn foundation_links_foundation_support_email() {
        let html = render(&FoundationContent::default(), crate::AuthState::Anonymous).into_string();
        let email = crate::brand::foundation_email();
        assert!(html.contains(&format!("mailto:{email}")));
    }

    #[test]
    fn foundation_header_uses_foundation_brand_nav() {
        let html = render(&FoundationContent::default(), crate::AuthState::Anonymous).into_string();
        // Foundation brand nav includes a back-link to the firm at "/".
        assert!(
            html.contains(&format!(">{}</a>", FIRM_BRAND.site_name)),
            "got: {html}"
        );
        // And does NOT carry the firm's Services dropdown.
        assert!(!html.contains(">Services</summary>"), "got: {html}");
    }
}
