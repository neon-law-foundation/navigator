//! `/` — a deliberately minimal landing card.
//!
//! The root names both organizations as equal peers: the firm's
//! registered brand mark over its flat-fee tagline, then the
//! Foundation's name — rendered at the same `display-5` size so neither
//! reads as subordinate — over its own non-profit tagline. Both
//! taglines state the mission plainly (mirroring `/foundation/mission`),
//! never a guarantee of results or a comparison. The standard layout
//! supplies the surrounding chrome and the firm brand.
//!
//! This is the simpler landing that previously shipped as the
//! private-mode root (`render_closed`); it is now the public home page.

use maud::{html, Markup};

use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};
use crate::components::ExternalLink;
use crate::{i18n, AuthState, Locale, PageLayout};

/// Render `/` in English. The layout supplies the surrounding chrome and
/// the firm brand by default.
#[must_use]
pub fn render(auth: AuthState) -> Markup {
    render_in(auth, Locale::En)
}

/// Render `/` in `locale`. The minimal card body is English (the binding
/// brand marks and the two mission taglines); `locale` localizes only
/// the surrounding chrome, and `/` stays the canonical twin of `/es`.
#[must_use]
pub fn render_in(auth: AuthState, locale: Locale) -> Markup {
    let body = html! {
        section."text-center"."py-5" {
            h1."display-5"."mb-3" {
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
            p."lead"."mb-5" {
                "An American law firm offering flat-fee legal services with a licensed attorney in the loop."
            }
            p."fw-semibold"."mb-3" {
                "This is the website for Neon Law. You can contact us about legal services."
            }
            a."btn"."btn-primary"."btn-lg"."mb-5" href="/contact" { "Contact us" }
            h1."display-5"."mb-3" { (FOUNDATION_BRAND.site_name) }
            p."lead"."mb-0" {
                "An American non-profit pursuing access to justice through open-source tools and legal-aid education."
            }
        }
    };
    // English keeps the literal "Home"; Spanish gets a localized title.
    let title = i18n::nav_label("Home", locale);
    PageLayout::new(&title)
        .with_description(
            "Neon Law is an American law firm offering flat-fee legal services with a licensed \
             attorney in the loop. This is the website for Neon Law; contact us about legal \
             services. The Neon Law Foundation pursues access to justice through open-source \
             tools and legal-aid education.",
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
    fn home_names_both_orgs_as_equal_peers() {
        let html = render(crate::AuthState::Anonymous).into_string();
        // Firm brand mark with the registered-trademark symbol.
        assert!(html.contains(FIRM_BRAND.site_name));
        assert!(
            html.contains("<sup>®</sup>"),
            "brand should carry the ® mark: {html}"
        );
        // Each org's mission tagline, stated plainly: the firm leads with its
        // flat-fee value, the Foundation owns the access-to-justice mission via
        // its open-source + legal-aid-education work.
        assert!(html.contains(
            "An American law firm offering flat-fee legal services with a licensed attorney in the loop."
        ));
        assert!(html.contains(
            "This is the website for Neon Law. You can contact us about legal services."
        ));
        assert!(html.contains("href=\"/contact\""));
        assert!(html.contains(
            "An American non-profit pursuing access to justice through open-source tools and legal-aid education."
        ));
        // The Foundation is named at the same display-5 size as the firm —
        // both headings carry the same class, so neither reads subordinate.
        assert!(
            html.contains(FOUNDATION_BRAND.site_name),
            "home should name the Foundation: {html}"
        );
        assert_eq!(
            html.matches("h1 class=\"display-5 mb-3\"").count(),
            2,
            "firm and Foundation names must share the display-5 heading size: {html}"
        );
    }

    #[test]
    fn home_is_the_minimal_card_without_a_marketing_hero() {
        let html = render(crate::AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(
            html.contains("<footer class=\"container py-4 border-top mt-4\">"),
            "home should carry the standard footer, got: {html}",
        );
        // It is the minimal page — no marketing hero strip.
        assert!(
            !html.contains("lake-tahoe"),
            "home must not render the marketing hero: {html}",
        );
    }
}
