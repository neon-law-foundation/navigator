//! `/services` — the public, no-login product catalog.
//!
//! One card per active product, rendered **from the `products` database
//! table** (not from hand-written marketing frontmatter): the price a
//! prospect reads here is the same `list_price_cents` row Xero invoices,
//! so the two can never drift. The `web` handler loads
//! [`store::products::list_active`], formats each price from its cents
//! ([`store::products::format_price`]), and hands the borrowed display
//! fields to [`index`] — this view never holds a hard-coded number.
//!
//! Firm-branded and English-first, with an `/es` twin via [`crate::i18n`].

use maud::{html, Markup};

use crate::brand::FIRM_BRAND;
use crate::{i18n, AuthState, Locale, PageLayout};

/// One product's display fields, borrowed from the `web` crate's owned
/// catalog rows for the duration of the render. **Every monetary value is
/// derived from the product's `list_price_cents`** by the handler — the
/// view formats nothing itself, so the page can never show a price that
/// differs from what Xero bills.
pub struct ProductCard<'a> {
    /// Human product name (`Neon Law Nautilus`).
    pub display_name: &'a str,
    /// List price already formatted from `list_price_cents` (`$44`).
    pub price: &'a str,
    /// Cadence suffix shown after the price (`/month`, `/year`, `/hour`,
    /// ` once` for a one-time fee).
    pub cadence_suffix: &'a str,
    /// A one-sentence, plain-language description of the service, shown
    /// under the price.
    pub description: &'a str,
    /// Where the card's "Learn more" link routes — the product's
    /// `/services/<slug>` page, already locale-prefixed by the handler.
    pub learn_href: &'a str,
    /// The product's Bootstrap Icon (glyph name without the `bi-` prefix,
    /// e.g. `"shield-fill-check"`), the same mark its detail page wears.
    /// `None` renders no icon.
    pub icon: Option<&'a str>,
}

/// The catalog index: a short pitch and a card per active product. The
/// English path (`canonical_path: None`) is byte-identical to the
/// pre-i18n shape; an `/es` request passes `Locale::Es` +
/// `Some("/services")` for the language switcher and `hreflang` alternates.
#[must_use]
pub fn index(cards: &[ProductCard<'_>], auth: AuthState) -> Markup {
    index_in(cards, auth, Locale::En, None)
}

/// Render the catalog in `locale`. See [`index`].
#[must_use]
pub fn index_in(
    cards: &[ProductCard<'_>],
    auth: AuthState,
    locale: Locale,
    canonical_path: Option<&str>,
) -> Markup {
    let heading = i18n::t(locale, "products.heading");
    let lead = i18n::t(locale, "products.lead");
    let learn_more = i18n::t(locale, "products.learn_more");
    // The contact page is English-only (no `/es/contact` route), so the
    // CTA links `/contact` in both locales; only the label localizes.
    let contact_label = i18n::t(locale, "products.contact");
    let body = html! {
        article {
            header {
                h1 { (heading) }
                p.lead { (lead) }
            }
            div.row."row-cols-1"."row-cols-md-2"."g-4"."mt-1" {
                @for card in cards {
                    div.col {
                        div.card."h-100" {
                            div."card-body" {
                                h2."h5"."card-title" {
                                    (crate::components::product_icon(card.icon, "me-2"))
                                    (card.display_name)
                                }
                                p."display-6"."fw-bold"."mb-1" {
                                    (card.price)
                                    @if !card.cadence_suffix.is_empty() {
                                        span."fs-6"."fw-normal"."text-muted" { (card.cadence_suffix) }
                                    }
                                }
                                p."card-text"."text-muted" { (card.description) }
                            }
                            div."card-footer"."bg-transparent" {
                                a."btn"."btn-outline-primary"."btn-sm" href=(card.learn_href) {
                                    (learn_more)
                                }
                            }
                        }
                    }
                }
            }
            section."mt-5"."p-4"."bg-light".rounded {
                p."mb-3" { (i18n::t(locale, "products.cta_blurb")) }
                a."btn"."btn-primary" href="/contact" { (contact_label) }
            }
        }
    };
    let mut layout = PageLayout::new(&heading)
        .with_description(&lead)
        .with_brand(*FIRM_BRAND)
        .with_auth(auth)
        .with_locale(locale);
    if let Some(path) = canonical_path {
        layout = layout.with_canonical_path(path);
    }
    layout.render(&body)
}

#[cfg(test)]
mod tests {
    use super::{index, index_in, ProductCard};
    use crate::{AuthState, Locale};

    fn cards() -> Vec<ProductCard<'static>> {
        vec![
            ProductCard {
                display_name: "Neon Law Nautilus",
                price: "$44",
                cadence_suffix: "/month",
                description: "A lawyer between you and the collectors.",
                learn_href: "/services/nautilus",
                icon: Some("shield-fill-check"),
            },
            ProductCard {
                display_name: "Neon Law Nexus",
                price: "$2,222",
                cadence_suffix: "/month",
                description: "A full legal retainer for a scaling company.",
                learn_href: "/services/fractional-gc",
                icon: None,
            },
        ]
    }

    #[test]
    fn index_lists_each_product_with_db_derived_price() {
        let html = index(&cards(), AuthState::Anonymous).into_string();
        assert!(html.contains("Neon Law Nautilus"));
        assert!(html.contains("$44"));
        assert!(html.contains("Neon Law Nexus"));
        assert!(html.contains("$2,222"));
        // The one-sentence description renders under the price.
        assert!(html.contains("A lawyer between you and the collectors."));
        // Each card links to its service page.
        assert!(html.contains("href=\"/services/nautilus\""));
        assert!(html.contains("href=\"/services/fractional-gc\""));
        // A card with an icon renders its Bootstrap glyph on the title; a
        // card with `icon: None` renders none.
        assert!(
            html.contains("<i class=\"bi bi-shield-fill-check me-2\" aria-hidden=\"true\"></i>"),
            "icon card should render its glyph, got: {html}"
        );
    }

    #[test]
    fn spanish_index_carries_localized_chrome_and_contact() {
        let html = index_in(
            &cards(),
            AuthState::Anonymous,
            Locale::Es,
            Some("/services"),
        )
        .into_string();
        // Spanish heading from the catalog, and the contact CTA points at
        // the `/es` contact page.
        assert!(html.contains("Servicios"), "got: {html}");
        // The contact CTA is English-only — same `/contact` in both
        // locales — but the button label localizes.
        assert!(html.contains("href=\"/contact\""), "got: {html}");
    }
}
