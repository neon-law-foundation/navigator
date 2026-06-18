//! Bootstrap pricing / offer cards for the firm's service pages.
//!
//! Two shapes share one component:
//!
//! - **Tiered plans** (fractional-GC: Seed / Growth / Scale) — each
//!   card carries a `cadence` (`/mo`), a feature list, and one card may
//!   be `featured` to anchor the reader on the firm's recommended tier.
//! - **Flat-fee menus** (estate, corporate) — `cadence` is `None`, the
//!   feature list is usually empty, and no card is featured.
//!
//! The data is borrowed view input; the owning strings live in the
//! marketing content the `web` crate loads and maps onto these structs
//! per request — same boundary as [`crate::pages::service::ServiceContent`].
//!
//! There is intentionally no "most popular" badge: a popularity claim
//! the firm cannot substantiate trips the attorney-advertising rules.
//! The anchor card carries the firm's own recommendation label instead.

use maud::{html, Markup};

use crate::components::ExternalLink;

/// One pricing / offer card.
pub struct PricingCard<'a> {
    /// Outcome-led title — what the client gets ("Living trust"), not
    /// the work we do.
    pub title: &'a str,
    /// The headline number, verbatim including any range marker:
    /// `"$3,500"`, `"from $1,000"`.
    pub price: &'a str,
    /// Billing cadence shown small after the price (`"/mo"`). `None`
    /// for one-time flat fees.
    pub cadence: Option<&'a str>,
    /// One line answering "is this for someone like me?".
    pub blurb: &'a str,
    /// Feature / inclusion bullets. May be empty (flat-fee menu cards).
    pub features: Vec<&'a str>,
    pub cta_label: &'a str,
    pub cta_href: &'a str,
    /// The anchor card — visually emphasized with a header band and a
    /// solid CTA.
    pub featured: bool,
    /// Label for the featured band (e.g. `"Recommended"`). Falls back
    /// to `"Recommended"` when `featured` is set without a label.
    pub featured_label: Option<&'a str>,
}

/// Render a responsive row of pricing cards.
///
/// `cols_lg` is the desktop column count (3 for tiered plans, up to 4
/// for flat-fee menus); the grid collapses to two columns on tablets
/// and one on phones. `cols_lg == 1` is the explicit "stack them"
/// request — one card per row at every breakpoint, never side-by-side
/// (e.g. Nimbus's two offers). Cards are equal height regardless of how
/// many feature bullets each carries.
#[must_use]
pub fn pricing_section(cards: &[PricingCard<'_>], cols_lg: u8) -> Markup {
    let row_class = if cols_lg <= 1 {
        "row row-cols-1 g-4 my-2".to_string()
    } else {
        format!("row row-cols-1 row-cols-md-2 row-cols-lg-{cols_lg} g-4 my-2")
    };
    html! {
        div class=(row_class) {
            @for card in cards {
                div."col" { (card_markup(card)) }
            }
        }
    }
}

fn card_markup(card: &PricingCard<'_>) -> Markup {
    let card_class = if card.featured {
        "card h-100 shadow-sm border-primary"
    } else {
        "card h-100 shadow-sm"
    };
    let cta_class = if card.featured {
        "btn btn-primary w-100 mt-auto"
    } else {
        "btn btn-outline-primary w-100 mt-auto"
    };
    html! {
        div class=(card_class) {
            @if card.featured {
                div."card-header"."bg-primary"."text-white"."text-center"."fw-semibold" {
                    (card.featured_label.unwrap_or("Recommended"))
                }
            }
            div."card-body"."d-flex"."flex-column" {
                h3."card-title"."h5"."mb-2" { (card.title) }
                p."mb-2" {
                    span."display-6"."fw-bold" { (card.price) }
                    @if let Some(cadence) = card.cadence {
                        " "
                        span."fs-6"."fw-normal"."text-body-secondary" { (cadence) }
                    }
                }
                p."text-body-secondary" { (card.blurb) }
                @if !card.features.is_empty() {
                    ul."list-unstyled"."mb-4" {
                        @for feature in &card.features {
                            li."mb-1" {
                                i."bi"."bi-check-lg"."text-primary"."me-2" {}
                                (feature)
                            }
                        }
                    }
                }
                // A firm card's CTA now points at the off-site booking
                // calendar; route any `http(s)` target through
                // `ExternalLink` for the new-tab + OWASP `rel` pair. A
                // `mailto:` / on-site target stays a plain styled anchor.
                @if card.cta_href.starts_with("http://") || card.cta_href.starts_with("https://") {
                    (ExternalLink::new(card.cta_href)
                        .with_class(cta_class)
                        .render(html! { (card.cta_label) }))
                } @else {
                    a class=(cta_class) href=(card.cta_href) { (card.cta_label) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pricing_section, PricingCard};

    fn tier<'a>(title: &'a str, price: &'a str, featured: bool) -> PricingCard<'a> {
        PricingCard {
            title,
            price,
            cadence: Some("/mo"),
            blurb: "For teams signing deals every week.",
            features: vec!["20 contract reviews each month"],
            cta_label: "Get your tier recommendation",
            cta_href: "mailto:support@neonlaw.com",
            featured,
            featured_label: featured.then_some("Recommended"),
        }
    }

    #[test]
    fn renders_one_card_per_entry_in_a_responsive_row() {
        let cards = [
            tier("Seed", "$3,500", false),
            tier("Growth", "$7,500", true),
            tier("Scale", "$12,500", false),
        ];
        let html = pricing_section(&cards, 3).into_string();
        assert_eq!(html.matches("class=\"card h-100").count(), 3);
        assert!(html.contains("row-cols-lg-3"));
        assert!(html.contains("Seed"));
        assert!(html.contains("$12,500"));
    }

    #[test]
    fn cols_lg_one_stacks_cards_one_per_row_at_every_breakpoint() {
        // `pricing_cols: 1` (e.g. Nimbus) must never put two cards
        // side-by-side: the row is a plain single column, with no
        // `row-cols-md-2` to break it into two on a tablet.
        let cards = [
            tier("Nimbus", "$11,111", true),
            tier("Legal aid", "Discounted", false),
        ];
        let html = pricing_section(&cards, 1).into_string();
        assert!(
            html.contains("class=\"row row-cols-1 g-4 my-2\""),
            "got: {html}"
        );
        assert!(
            !html.contains("row-cols-md-2"),
            "stacked row must not collapse to two-up: {html}"
        );
        assert_eq!(html.matches("class=\"card h-100").count(), 2);
    }

    #[test]
    fn featured_card_gets_primary_band_and_solid_cta() {
        let cards = [tier("Growth", "$7,500", true)];
        let html = pricing_section(&cards, 3).into_string();
        assert!(html.contains("card h-100 shadow-sm border-primary"));
        assert!(html.contains("card-header"));
        assert!(html.contains("Recommended"));
        assert!(html.contains("btn btn-primary"));
    }

    #[test]
    fn unfeatured_card_uses_outline_cta_and_no_band() {
        let cards = [tier("Seed", "$3,500", false)];
        let html = pricing_section(&cards, 3).into_string();
        assert!(html.contains("btn btn-outline-primary"));
        assert!(!html.contains("card-header"));
    }

    #[test]
    fn flat_fee_card_has_no_cadence_and_no_bullets() {
        let card = PricingCard {
            title: "Simple will",
            price: "$500",
            cadence: None,
            blurb: "Decide who inherits and who decides for you.",
            features: Vec::new(),
            cta_label: "Get started",
            cta_href: "mailto:support@neonlaw.com",
            featured: false,
            featured_label: None,
        };
        let html = pricing_section(&[card], 4).into_string();
        assert!(html.contains("row-cols-lg-4"));
        assert!(html.contains("$500"));
        assert!(!html.contains("/mo"));
        assert!(!html.contains("<ul"));
    }

    #[test]
    fn renders_check_icon_per_feature() {
        let card = PricingCard {
            features: vec!["a", "b", "c"],
            ..tier("Scale", "$12,500", false)
        };
        let html = pricing_section(&[card], 3).into_string();
        assert_eq!(html.matches("bi-check-lg").count(), 3);
    }
}
