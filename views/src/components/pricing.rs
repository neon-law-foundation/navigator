//! Bootstrap pricing / offer cards for the firm's service pages.
//!
//! Flat-fee cards share one visual treatment: the cyan header band and
//! solid CTA formerly reserved for a featured tier. Some legacy content
//! still sets `featured`; the renderer keeps the field for schema
//! compatibility but no longer branches the card style on it.
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
    /// Legacy marker for the old tiered-plan treatment. Pricing cards
    /// are all rendered with the highlighted flat-fee style now.
    pub featured: bool,
    /// Label for the cyan band (e.g. `"$3,333, once"`). Falls back to
    /// the card price when omitted.
    pub featured_label: Option<&'a str>,
}

/// Render a responsive row of pricing cards.
///
/// `cols_lg` is the desktop column count (up to 4 for flat-fee menus);
/// the grid collapses to two columns on tablets and one on phones.
/// `cols_lg == 1` is the explicit "stack them" request — one card per
/// row at every breakpoint, never side-by-side (e.g. Nimbus's two
/// offers). Cards are equal height regardless of how many feature
/// bullets each carries.
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
    let band_label = card.featured_label.unwrap_or(card.price);
    let card_class = "card h-100 shadow-sm border-primary";
    let cta_class = "btn btn-primary w-100 mt-auto";
    html! {
        div class=(card_class) {
            div."card-header"."bg-primary"."text-white"."text-center"."fw-semibold" {
                (band_label)
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

    fn fee_card<'a>(title: &'a str, price: &'a str, featured: bool) -> PricingCard<'a> {
        PricingCard {
            title,
            price,
            cadence: Some("/mo"),
            blurb: "Flat monthly counsel for operating the company.",
            features: vec!["Priority support"],
            cta_label: "Get started",
            cta_href: "mailto:support@neonlaw.com",
            featured,
            featured_label: featured.then_some("$2,222 a month, all in"),
        }
    }

    #[test]
    fn renders_one_card_per_entry_in_a_responsive_row() {
        let cards = [
            fee_card("Northstar", "$3,333", false),
            fee_card("Nexus", "$2,222", true),
            fee_card("Nimbus", "$11,111", false),
        ];
        let html = pricing_section(&cards, 3).into_string();
        assert_eq!(html.matches("class=\"card h-100").count(), 3);
        assert!(html.contains("row-cols-lg-3"));
        assert!(html.contains("Northstar"));
        assert!(html.contains("$11,111"));
    }

    #[test]
    fn cols_lg_one_stacks_cards_one_per_row_at_every_breakpoint() {
        // `pricing_cols: 1` (e.g. Nimbus) must never put two cards
        // side-by-side: the row is a plain single column, with no
        // `row-cols-md-2` to break it into two on a tablet.
        let cards = [
            fee_card("Nimbus", "$11,111", true),
            fee_card("Legal aid", "Discounted", false),
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
    fn every_card_gets_primary_band_and_solid_cta() {
        let cards = [fee_card("Nexus", "$2,222", true)];
        let html = pricing_section(&cards, 3).into_string();
        assert!(html.contains("card h-100 shadow-sm border-primary"));
        assert!(html.contains("card-header"));
        assert!(html.contains("$2,222 a month, all in"));
        assert!(html.contains("btn btn-primary"));
    }

    #[test]
    fn unfeatured_legacy_card_still_uses_highlighted_flat_fee_style() {
        let cards = [fee_card("Northstar", "$3,333", false)];
        let html = pricing_section(&cards, 3).into_string();
        assert!(html.contains("card h-100 shadow-sm border-primary"));
        assert!(html.contains("card-header"));
        assert!(html.contains("$3,333"));
        assert!(html.contains("btn btn-primary"));
        assert!(!html.contains("btn btn-outline-primary"));
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
        assert!(html.contains("card-header bg-primary text-white text-center fw-semibold"));
    }

    #[test]
    fn renders_check_icon_per_feature() {
        let card = PricingCard {
            features: vec!["a", "b", "c"],
            ..fee_card("Nexus", "$2,222", false)
        };
        let html = pricing_section(&[card], 3).into_string();
        assert_eq!(html.matches("bi-check-lg").count(), 3);
    }
}
