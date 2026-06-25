//! Shared rendering for the firm's service pages (`/services/nexus`,
//! `/services/northstar`, `/services/nest`) and the Foundation's product
//! pages (`/foundation/nimbus`).
//!
//! Each route hands a `ServiceContent` to [`render`]; the helper wraps the
//! body in the page's own brand chrome (firm or Foundation, carried on
//! `ServiceContent::brand`) and tacks on a CTA so the page never ships
//! without a way to make contact — a "Book a Consultation" link to the
//! firm calendar on firm pages, a mailto to the Foundation inbox on
//! Foundation product pages.

use maud::{html, Markup, PreEscaped};

use crate::assets::{self, Priority};
use crate::brand::SiteBrand;
use crate::components::{
    pricing_section, testimonial_section, ExternalLink, PricingCard, TestimonialCard,
};
use crate::{i18n, AuthState, Locale, PageLayout};

/// Where the pricing cards are spliced into the prose. Content authors
/// drop a bare `[[pricing]]` paragraph in the markdown; pulldown-cmark
/// renders it verbatim as this token, and [`render`] swaps the cards
/// in for it. No marker (or no cards) → the body renders unchanged.
const PRICING_MARKER: &str = "<p>[[pricing]]</p>";

/// Split a product page's rendered body into its leading headline and
/// the prose that follows. Product markdown opens with `# …`, which
/// pulldown-cmark renders as `<h1>…</h1>`; we lift that headline up into
/// the page header (as the `.lead` tagline under the "Neon Law …" brand
/// title) so it is not repeated. Returns `(None, body)` unchanged when
/// the body doesn't open with an `<h1>`.
fn split_leading_h1(body: &str) -> (Option<&str>, &str) {
    let trimmed = body.trim_start();
    let Some(after_open) = trimmed.strip_prefix("<h1>") else {
        return (None, body);
    };
    let Some(close) = after_open.find("</h1>") else {
        return (None, body);
    };
    let headline = &after_open[..close];
    let rest = after_open[close + "</h1>".len()..].trim_start();
    (Some(headline), rest)
}

/// Render a CTA anchor. An off-site `http(s)` target (the firm
/// consultation calendar, and the booking-linked firm pricing cards)
/// routes through [`ExternalLink`] for the new-tab + OWASP `rel` pair
/// and the "leaves the site" glyph; an on-site or `mailto:` target (the
/// Foundation inbox fallback) stays a plain styled anchor.
fn cta_button(class: &str, label: &str, href: &str) -> Markup {
    if href.starts_with("http://") || href.starts_with("https://") {
        ExternalLink::new(href)
            .with_class(class)
            .render(html! { (label) })
    } else {
        html! { a class=(class) href=(href) { (label) } }
    }
}

pub struct ServiceContent<'a> {
    /// Used both for the `<title>` and as the page heading hook —
    /// callers supply the body markdown that includes its own
    /// `<h1>` or `<h2>` (rendered ahead of this view).
    pub title: &'a str,
    pub description: &'a str,
    /// Pre-rendered HTML body. NOT raw markdown.
    pub body_html: &'a str,
    /// Pricing / offer cards, mapped from the page's `pricing:`
    /// frontmatter. Empty for pages that don't advertise a price.
    pub pricing: Vec<PricingCard<'a>>,
    /// Desktop column count for the pricing grid (3 for tiered plans,
    /// up to 4 for flat-fee menus).
    pub pricing_cols: u8,
    /// Curated gallery slug for the product banner photo (the `hero_image:`
    /// frontmatter key, e.g. `lake-tahoe`). `Some` leads the page with a
    /// wide Notion-style banner above the brand title. `None` (a fallback
    /// page with no marketing doc) renders the stacked layout with no
    /// banner. Either way the body's leading `<h1>` is lifted into the
    /// header tagline.
    pub hero_image: Option<&'a str>,
    /// Brand chrome for the page: `FIRM_BRAND` for `/services/*`,
    /// `FOUNDATION_BRAND` for a Foundation product page like
    /// `/foundation/nimbus`. `SiteBrand` is `Copy`.
    pub brand: SiteBrand,
    /// Contact inbox for the page. On Foundation product pages this is
    /// the mailto target + "Email …" button label for the footer and
    /// hero-fallback CTAs. On firm pages those CTAs instead book a
    /// consultation (see [`crate::brand::consultation_url`]), so this is
    /// unused there.
    pub cta_email: &'a str,
    /// The product's mark, rendered before the brand title — a Bootstrap
    /// Icon glyph name without the `bi-` prefix (e.g. `"diagram-3-fill"`),
    /// or the `"libra-scales"` sentinel for the inline scales-of-justice
    /// SVG (litigation). Resolved by [`crate::components::product_icon`].
    /// These are the marks that used to denote each product in the Services
    /// dropdown; with the dropdown gone, each page keeps its own mark.
    /// `None` renders no icon (the Foundation product pages).
    pub icon: Option<&'a str>,
    /// Public testimonials selected by the web layer for this service's
    /// product code. Empty keeps the page on the no-proof path.
    pub testimonials: &'a [TestimonialCard<'a>],
}

/// Render a service page in English (no declared twin).
#[must_use]
pub fn render(content: &ServiceContent<'_>, auth: AuthState) -> Markup {
    render_in(content, auth, Locale::En, None)
}

/// Render a service page in `locale`. `canonical_path` (e.g.
/// `/services/northstar`) is the locale-less path; when `Some`, the layout
/// emits the `hreflang` alternates and the navbar language switcher. The
/// English path with `None` is byte-identical to the pre-i18n page.
#[must_use]
pub fn render_in(
    content: &ServiceContent<'_>,
    auth: AuthState,
    locale: Locale,
    canonical_path: Option<&str>,
) -> Markup {
    let cards = || pricing_section(&content.pricing, content.pricing_cols);
    let cta = i18n::t_args(locale, "cta.email", &[("email", content.cta_email)]);
    // The footer and the hero fallback button both write to the page's
    // brand inbox (firm vs Foundation); owned here so the `&str` lives
    // through the `body` build below.
    let cta_mailto = format!("mailto:{}", content.cta_email);
    // Firm pages drive every CTA to the consultation calendar; the
    // Foundation product pages (Nimbus) keep their own inbox. `cta` /
    // `cta_mailto` above remain the Foundation path.
    let books_consultation = content.brand.is_law_firm;
    let consultation_label = i18n::t(locale, "cta.consultation");
    let (footer_label, footer_href) = if books_consultation {
        (
            consultation_label.as_str(),
            crate::brand::consultation_url(),
        )
    } else {
        (cta.as_str(), cta_mailto.as_str())
    };
    // Notion-style stacked layout: a wide banner image leads the page,
    // then the product mark + brand title, then the product card, then
    // the prose outline, and finally the booking CTA. We always lift the
    // body's leading `<h1>` into the header lead so the brand title is the
    // page's single `<h1>` and the headline isn't repeated.
    let (headline, prose_body) = split_leading_h1(content.body_html);
    // The card now sits in its own section above the prose, so drop the
    // inline `[[pricing]]` splice marker if the author left one.
    let prose = prose_body.replace(PRICING_MARKER, "");
    let body = html! {
        // 1. The wide banner image across the top, like a Notion cover.
        //    A sibling of `.service-prose`, so the content-image cap in
        //    brand.css doesn't touch it.
        @if let Some(slug) = content.hero_image {
            figure."service-banner"."mb-4"."mb-lg-5" {
                (assets::banner(slug, Priority::Eager))
            }
        }
        article.service-page.service-prose {
            // 2. The product mark, then the brand title as the page `<h1>`,
            //    with the lifted headline as a centered lead beneath it.
            header."text-center"."mb-4"."mb-lg-5" {
                h1."display-4"."fw-bold"."mb-3" {
                    (crate::components::product_icon(content.icon, "me-3"))
                    (content.title)
                }
                @if let Some(h) = headline {
                    p."lead"."mb-0"."mx-auto"."service-tagline" { (PreEscaped(h)) }
                }
            }
            // 3. The product card.
            @if !content.pricing.is_empty() {
                div."mb-4"."mb-lg-5" { (cards()) }
            }
            // 4. The short outline.
            (PreEscaped(&prose))
            (testimonial_section(
                "Client proof",
                "Matter-linked testimonials approved for this service.",
                content.testimonials,
            ))
            // 5. The call to action — the firm's booking calendar (a
            //    mailto inbox on a Foundation product page).
            footer."text-center"."mt-4"."mt-lg-5" {
                (cta_button("btn btn-primary btn-lg", footer_label, footer_href))
            }
        }
    };
    // The browser `<title>` on a firm `/services` page brands once, then
    // names the catalog and the bare product: "Neon Law | Services | Nest"
    // — never the redundant "Neon Law | Neon Law Nest". The layout prepends
    // the brand, so strip the firm-brand prefix off the product mark (so the
    // brand isn't stated twice) and slot it behind a localized "Services".
    // The visible `<h1>` keeps the full `content.title` ("Neon Law Nest").
    // Foundation product pages (Nimbus) aren't under `/services`, so they
    // keep the plain product title.
    let head_title = if content.brand.is_law_firm {
        let product = content
            .title
            .strip_prefix(&format!("{} ", content.brand.site_name))
            .unwrap_or(content.title);
        format!("{} | {product}", i18n::t(locale, "nav.services"))
    } else {
        content.title.to_string()
    };
    let mut layout = PageLayout::new(&head_title)
        .with_description(content.description)
        .with_brand(content.brand)
        .with_auth(auth)
        .with_locale(locale);
    if let Some(path) = canonical_path {
        layout = layout.with_canonical_path(path);
    }
    // Preload the hero photo so it leads the Largest Contentful Paint,
    // the same as the home hero. `href` must outlive the borrow.
    match content.hero_image.and_then(assets::preload_href) {
        Some(href) => layout.with_preload_image(&href).render(&body),
        None => layout.render(&body),
    }
}

#[cfg(test)]
mod tests {
    use super::{render, PricingCard, ServiceContent};
    use crate::brand::{firm_email, FIRM_BRAND, FOUNDATION_BRAND};

    fn fixture<'a>(title: &'a str, body: &'a str) -> ServiceContent<'a> {
        ServiceContent {
            title,
            description: "desc",
            body_html: body,
            pricing: Vec::new(),
            pricing_cols: 3,
            hero_image: None,
            brand: *FIRM_BRAND,
            cta_email: firm_email(),
            icon: None,
            testimonials: &[],
        }
    }

    #[test]
    fn render_uses_title_in_browser_title_under_firm_brand() {
        // A firm `/services` page brands once, then slots the product
        // behind a "Services" segment: "Neon Law | Services | …".
        let html = render(
            &fixture("Estate planning", "<p>body</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | Services | Estate planning</title>",
            FIRM_BRAND.site_name
        )));
    }

    #[test]
    fn firm_browser_title_strips_the_redundant_brand_prefix_from_the_product() {
        // The product mark "Neon Law Nest" must not double the brand in the
        // tab title — it reads "Neon Law | Services | Nest", not the
        // redundant "Neon Law | Neon Law Nest". The visible <h1> keeps the
        // full mark.
        let html = render(
            &fixture("Neon Law Nest", "<h1>Headline</h1><p>body</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(
            html.contains(&format!(
                "<title>{} | Services | Nest</title>",
                FIRM_BRAND.site_name
            )),
            "tab title should brand once, then Services | Nest, got: {html}"
        );
        assert!(
            !html.contains(&format!("{0} | {0} Nest", FIRM_BRAND.site_name)),
            "tab title must not double the brand, got: {html}"
        );
        // The on-page heading still carries the full product mark.
        assert!(
            html.contains(">Neon Law Nest</h1>"),
            "the <h1> keeps the full product mark, got: {html}"
        );
    }

    #[test]
    fn firm_browser_title_keeps_a_product_with_no_brand_prefix_intact() {
        // A product whose mark doesn't lead with the brand (e.g. the
        // litigation practice "1337 Lawyers") slots in whole — no prefix
        // to strip.
        let html = render(
            &fixture("1337 Lawyers", "<p>body</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(
            html.contains(&format!(
                "<title>{} | Services | 1337 Lawyers</title>",
                FIRM_BRAND.site_name
            )),
            "tab title should read Services | 1337 Lawyers, got: {html}"
        );
    }

    #[test]
    fn foundation_product_keeps_the_plain_browser_title() {
        // A Foundation product page (Nimbus) is not under `/services`, so
        // its tab title stays the plain product mark under the Foundation
        // brand — no "Services" segment.
        let mut content = fixture("Neon Law Foundation Nimbus", "<p>body</p>");
        content.brand = *FOUNDATION_BRAND;
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(&format!(
                "<title>{} | Neon Law Foundation Nimbus</title>",
                FOUNDATION_BRAND.site_name
            )),
            "Foundation product keeps the plain title, got: {html}"
        );
        assert!(
            !html.contains("| Services |"),
            "Foundation product is not a /services page, got: {html}"
        );
    }

    #[test]
    fn render_embeds_body_html_verbatim() {
        let html = render(
            &fixture("X", "<h2>Drafted</h2><p>Trusts.</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(html.contains("<h2>Drafted</h2>"));
        assert!(html.contains("<p>Trusts.</p>"));
    }

    #[test]
    fn service_page_prose_carries_the_responsive_measure_class() {
        // The prose column wears `.service-prose`; the reading measure
        // now lives in brand.css (65ch on a phone, 78ch on desktop)
        // rather than an inline cap, so the desktop page can run wider.
        let html = render(&fixture("X", "<p>x</p>"), crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("class=\"service-page service-prose\""),
            "service body should carry the responsive-measure class, got: {html}"
        );
        // The old hard-coded inline cap is gone — the class owns it now.
        assert!(
            !html.contains("max-width: 65ch"),
            "measure should be class-driven, not an inline cap, got: {html}"
        );
    }

    #[test]
    fn hero_image_renders_a_banner_above_the_brand_title() {
        // A page with a hero image leads with a wide banner cover, then
        // the "Neon Law …" brand title as the page <h1> beneath it
        // (Notion-style stacking).
        let mut content = fixture(
            "Neon Law Nautilus",
            "<h1>Put a lawyer between you</h1><p>body</p>",
        );
        content.hero_image = Some("migrating-birds");
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        // The banner is a full-width figure carrying the curated photo.
        assert!(
            html.contains("class=\"service-banner mb-4 mb-lg-5\"")
                && html.contains("migrating-birds")
                && html.contains("<picture>"),
            "page should open with a banner figure of the curated photo, got: {html}"
        );
        // Brand title is the page h1, and it sits *below* the banner.
        assert!(
            html.contains("<h1 class=\"display-4 fw-bold mb-3\">Neon Law Nautilus</h1>"),
            "title should headline the page, got: {html}"
        );
        let banner_at = html.find("service-banner").unwrap();
        let title_at = html.find("Neon Law Nautilus</h1>").unwrap();
        assert!(
            banner_at < title_at,
            "banner must lead the title, got: {html}"
        );
        // The markdown headline is lifted into the centered lead, once.
        assert_eq!(
            html.matches("Put a lawyer between you").count(),
            1,
            "headline must move into the lead, not be duplicated, got: {html}"
        );
        assert!(
            html.contains(
                "<p class=\"lead mb-0 mx-auto service-tagline\">Put a lawyer between you</p>"
            ),
            "headline should become the centered tagline lead, got: {html}"
        );
        // And it leads the LCP via a hero preload.
        assert!(
            html.contains("rel=\"preload\" as=\"image\""),
            "banner photo should be preloaded, got: {html}"
        );
    }

    #[test]
    fn hero_title_carries_the_product_icon_when_set() {
        // The product's Bootstrap Icon (the mark that used to sit in the
        // Services dropdown) renders before the hero brand title.
        let mut content = fixture("Neon Law Nexus", "<h1>Headline</h1><p>body</p>");
        content.hero_image = Some("bengaluru-skyline");
        content.icon = Some("diagram-3-fill");
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("<i class=\"bi bi-diagram-3-fill me-3\" aria-hidden=\"true\"></i>"),
            "hero title should carry the product icon, got: {html}"
        );
        // The icon sits inside the hero h1, immediately before the title.
        assert!(
            html.contains(
                "<h1 class=\"display-4 fw-bold mb-3\">\
                 <i class=\"bi bi-diagram-3-fill me-3\" aria-hidden=\"true\"></i>Neon Law Nexus</h1>"
            ),
            "icon should lead the hero title, got: {html}"
        );
    }

    #[test]
    fn hero_title_has_no_icon_when_unset() {
        // A page with `icon: None` (the Foundation product pages) renders
        // the title with no leading glyph.
        let mut content = fixture("Neon Law Foundation Nimbus", "<h1>Headline</h1><p>body</p>");
        content.hero_image = Some("bengaluru-skyline");
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains("<h1 class=\"display-4 fw-bold mb-3\">Neon Law Foundation Nimbus</h1>"),
            "no-icon page should render a bare hero title, got: {html}"
        );
    }

    #[test]
    fn the_product_card_sits_above_the_prose_and_the_footer_books_a_consultation() {
        // In the stacked layout the product (pricing) card renders in its
        // own section above the prose outline, and the closing footer CTA
        // is the firm's booking calendar — a large primary button.
        let mut content = fixture("Neon Law Nautilus", "<h1>Headline</h1><p>the outline</p>");
        content.hero_image = Some("migrating-birds");
        content.pricing = vec![PricingCard {
            cta_label: "Start Nautilus",
            cta_href: "mailto:support@neonlaw.com",
            ..one_card()
        }];
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        // The card (its own CTA label) sits above the prose outline.
        let card_at = html.find("Start Nautilus").unwrap();
        let outline_at = html.find("the outline").unwrap();
        assert!(
            card_at < outline_at,
            "card must sit above the outline, got: {html}"
        );
        // The closing CTA is the firm booking calendar as a large button.
        assert!(
            html.contains("btn btn-primary btn-lg")
                && html.contains(crate::brand::consultation_url()),
            "footer CTA should book the consultation calendar, got: {html}"
        );
        let footer_cta_at = html.rfind("btn btn-primary btn-lg").unwrap();
        assert!(
            footer_cta_at > outline_at,
            "booking CTA must close the page, got: {html}"
        );
    }

    #[test]
    fn without_a_hero_image_the_title_still_leads_and_no_banner_renders() {
        // A page with no hero_image (a fallback page) renders no banner,
        // but the brand title is still the page <h1> and the body's
        // leading <h1> is lifted into the lead (never left as a second h1).
        let html = render(
            &fixture("Services", "<h1>Everything we do</h1><p>menu</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(
            !html.contains("service-banner"),
            "no banner without a hero image, got: {html}"
        );
        assert!(
            html.contains("<h1 class=\"display-4 fw-bold mb-3\">Services</h1>"),
            "brand title still leads as the page h1, got: {html}"
        );
        assert!(
            !html.contains("<h1>Everything we do</h1>"),
            "the body's leading h1 is lifted into the lead, got: {html}"
        );
    }

    #[test]
    fn firm_page_cta_books_a_consultation() {
        // A firm service page with no pricing falls back to the booking
        // CTA: an external-safe link to the firm consultation calendar,
        // not a mailto.
        let html = render(&fixture("X", "<p>x</p>"), crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(&format!("href=\"{}\"", crate::brand::consultation_url())),
            "firm CTA should link the consultation calendar: {html}"
        );
        assert!(html.contains("Book a Consultation"), "got: {html}");
        assert!(
            html.contains("rel=\"noopener noreferrer\""),
            "booking link must be external-safe: {html}"
        );
        assert!(
            !html.contains("mailto:support@neonlaw.com"),
            "firm CTA no longer emails the inbox: {html}"
        );
    }

    #[test]
    fn foundation_brand_product_renders_foundation_chrome_and_inbox() {
        // The same view backs a Foundation product page (Nimbus). When
        // handed the Foundation brand + inbox, it renders the Foundation
        // title, writes its CTA to the Foundation inbox, and drops the
        // firm's Services dropdown.
        let mut content = fixture("Neon Law Foundation Nimbus", "<p>x</p>");
        content.brand = *FOUNDATION_BRAND;
        content.cta_email = crate::brand::foundation_email();
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        assert!(
            html.contains(&format!(
                "<title>{} | Neon Law Foundation Nimbus</title>",
                FOUNDATION_BRAND.site_name
            )),
            "got: {html}"
        );
        assert!(
            html.contains(&format!("mailto:{}", crate::brand::foundation_email())),
            "CTA should write to the Foundation inbox, got: {html}"
        );
        assert!(
            !html.contains(">Services</summary>"),
            "Foundation page must not carry the firm Services dropdown, got: {html}"
        );
    }

    fn one_card<'a>() -> crate::components::PricingCard<'a> {
        crate::components::PricingCard {
            title: "Growth",
            price: "$7,500",
            cadence: Some("/mo"),
            blurb: "blurb",
            features: vec!["20 reviews"],
            cta_label: "Get your tier recommendation",
            cta_href: "mailto:support@neonlaw.com",
            featured: true,
            featured_label: Some("Recommended"),
        }
    }

    #[test]
    fn pricing_cards_render_above_the_prose_and_the_marker_is_dropped() {
        let mut content = fixture("X", "<p>lead</p><p>[[pricing]]</p><h2>Details</h2>");
        content.pricing = vec![one_card()];
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        // The marker is consumed wherever the author put it, and the card
        // now sits in its own section above the whole prose outline.
        assert!(!html.contains("[[pricing]]"));
        assert!(html.contains("Growth"));
        let card = html.find("Growth").unwrap();
        let lead = html.find("lead").unwrap();
        let details = html.find("Details").unwrap();
        assert!(
            card < lead && card < details,
            "card must sit above the prose, not at the old marker position"
        );
    }

    #[test]
    fn marker_without_cards_is_dropped() {
        let html = render(
            &fixture("X", "<p>lead</p><p>[[pricing]]</p><p>tail</p>"),
            crate::AuthState::Anonymous,
        )
        .into_string();
        assert!(!html.contains("[[pricing]]"));
        assert!(html.contains("lead") && html.contains("tail"));
    }

    #[test]
    fn cards_without_marker_render_above_the_body() {
        let mut content = fixture("X", "<p>only body</p>");
        content.pricing = vec![one_card()];
        let html = render(&content, crate::AuthState::Anonymous).into_string();
        let body = html.find("only body").unwrap();
        let card = html.find("Growth").unwrap();
        assert!(card < body, "the card section now leads the prose body");
    }
}
