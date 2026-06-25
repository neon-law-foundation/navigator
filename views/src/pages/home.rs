//! `/` — the firm landing page.
//!
//! The root is firm-branded: it leads with Neon Law's flat-fee legal
//! work, then explains how the firm uses Navigator and supports the
//! Foundation's access-to-justice mission. The Foundation's full mission
//! letter now lives at `/foundation`.

use maud::{html, Markup};

use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};
use crate::components::{testimonial_section, ExternalLink, TestimonialCard};
use crate::{i18n, AuthState, Locale, PageLayout};

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
        "to self-host Navigator's rule engine, CLI, MCP server, and web app",
        "Apache-2.0 / MIT",
    ),
];

/// Render `/` in English. The layout supplies the surrounding chrome and
/// the firm brand by default.
#[must_use]
pub fn render(auth: AuthState) -> Markup {
    render_in(auth, Locale::En, &[])
}

/// Render `/` in `locale`. The page body is English; `locale`
/// localizes only the surrounding chrome, and `/` stays the canonical
/// twin of `/es`.
#[must_use]
pub fn render_in(auth: AuthState, locale: Locale, testimonials: &[TestimonialCard<'_>]) -> Markup {
    let body = html! {
        section."hero-neon"."mb-5" {
            // Decorative neon scene (grid horizon + glow + sweep). Hidden
            // from assistive tech; all meaning lives in the content block.
            div."hero-neon__bg" aria-hidden="true" {
                div."hero-neon__glow" {}
                div."hero-neon__grid" {}
                div."hero-neon__horizon" {}
                div."hero-neon__sweep" {}
            }
            div."hero-neon__content" {
                p."hero-neon__eyebrow"."mb-2" {
                    "Everything we can toward access to justice"
                }
                h1."hero-neon__mark"."display-3"."fw-bold"."mb-3" {
                    @if let Some(url) = FIRM_BRAND.trademark_registration_url {
                        (ExternalLink::new(url)
                            .with_class("link-body-emphasis text-decoration-none")
                            .with_title(
                                "NEON LAW is a registered trademark — \
                                 U.S. Reg. No. 6,325,650",
                            )
                            .render(html! { (FIRM_BRAND.site_name) sup { "®" } }))
                    } @else {
                        (FIRM_BRAND.site_name)
                    }
                }
                p."hero-neon__lead"."mb-4" {
                    "Building something, protecting it, or exercising a right you already hold — \
                     a licensed attorney works with you, with transparent pricing before the work \
                     begins. We believe that everyone in America should exercise their legal rights."
                }
                div."d-flex"."flex-wrap"."gap-2" {
                    a."btn"."btn-primary"."btn-lg" href="/services" { "View Services" }
                    a."btn"."btn-lg"."hero-neon__btn-ghost" href="/foundation" { "Read the Mission" }
                }
            }
        }

        (testimonial_section(
            "What clients say",
            "Published only after client consent and staff review.",
            testimonials,
        ))

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

        section."mb-5" {
            div."row"."g-4"."align-items-start" {
                div."col-lg-6" {
                    h2."h3" { "Legal work in an auditable workflow" }
                    p {
                        "Navigator turns intake questions, legal templates, and workflow states into plain-text \
                         Notations. The firm uses that system in its own matters so each engagement has a clear path \
                         from first answer to attorney review."
                    }
                    p."mb-0" {
                        a href="/foundation/notations" { "Read about Notations" }
                    }
                }
                div."col-lg-6" {
                    h2."h3" { "The Foundation carries the public mission" }
                    p {
                        (FOUNDATION_BRAND.site_name) " publishes Navigator as open-source software and trains lawyers \
                         to adapt it for legal-aid and public-interest work. The firm and the Foundation share a \
                         mission, but the Foundation's work is public-interest work, not legal representation."
                    }
                    p."mb-0" {
                        a href="/foundation" { "Read the Foundation mission" }
                    }
                }
            }
        }
    };
    // English keeps the literal "Home"; Spanish gets a localized title.
    let title = i18n::nav_label("Home", locale);
    PageLayout::new(&title)
        .with_description(
            "Neon Law offers flat-fee legal services with a licensed attorney in the loop, \
             using Navigator to keep intake, drafting, review, and delivery auditable.",
        )
        .with_auth(auth)
        .with_locale(locale)
        .with_canonical_path("/")
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn home_renders_doctype_and_layout_chrome() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!("<title>{} | Home</title>", FIRM_BRAND.site_name)));
        // Footer wears the Bootstrap container + spacing + top border.
        assert!(
            html.contains("<footer class=\"container py-4 border-top mt-4\">"),
            "footer should be a Bootstrap container, got: {html}",
        );
    }

    #[test]
    fn home_leads_with_the_firm_and_names_the_foundation_mission() {
        let html = render(crate::AuthState::Anonymous).into_string();
        // Firm brand mark with the registered-trademark symbol.
        assert!(html.contains(FIRM_BRAND.site_name));
        assert!(
            html.contains("<sup>®</sup>"),
            "brand should carry the ® mark: {html}"
        );
        // The hero leads on the access-to-justice mission, not on price.
        assert!(html.contains("Everything we can toward access to justice"));
        assert!(html.contains(
            "a licensed attorney works with you, with transparent pricing before the work"
        ));
        assert!(html.contains("everyone in America should exercise their legal rights"));
        assert!(
            html.contains(FOUNDATION_BRAND.site_name),
            "home should name the Foundation: {html}"
        );
        assert!(html.contains("Read the Foundation mission"));
        assert!(html.contains("href=\"/foundation/notations\""));
    }

    #[test]
    fn home_carries_the_justice_gap_stats_from_the_foundation_landing() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.contains(">92%</p>"));
        assert!(html.contains("LSC Justice Gap Report, 2022"));
        assert!(html.contains(">5.1B</p>"));
        assert!(html.contains("World Justice Project, 2023"));
        assert!(html.contains(">0</p>") || html.contains(">$0</p>"));
        assert!(
            html.contains("to self-host Navigator"),
            "home should carry the Foundation open-source stat: {html}"
        );
    }

    #[test]
    fn home_is_firm_branded_without_the_old_photo_hero() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(
            html.contains("<footer class=\"container py-4 border-top mt-4\">"),
            "home should carry the standard footer, got: {html}",
        );
        // It is firm-branded prose and cards — no old marketing hero strip.
        assert!(
            !html.contains("lake-tahoe"),
            "home must not render the marketing hero: {html}",
        );
    }
}
