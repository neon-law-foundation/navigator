//! Product marks for the catalog card and the service page.
//!
//! Each product is denoted by a small icon. Most are Bootstrap Icons
//! font glyphs (`<i class="bi bi-…">`), but litigation wears the **scales
//! of justice** — and Bootstrap Icons ships no balance-scale glyph. So
//! [`product_icon`] resolves a single sentinel, `"libra-scales"`, to an
//! inline SVG and passes every other name straight through to the font.
//!
//! Both render sites — [`crate::pages::service`] and
//! [`crate::pages::products`] — call this one helper, so the catalog card
//! and the detail page can never disagree about a product's mark.

use maud::{html, Markup};

/// The inline-SVG sentinel: a product whose `icon` is this string renders
/// the scales-of-justice drawing instead of a Bootstrap glyph.
pub const LIBRA_SCALES: &str = "libra-scales";

/// Render a product's mark. `icon` is the glyph name without the `bi-`
/// prefix (e.g. `"star-fill"`), or the [`LIBRA_SCALES`] sentinel for the
/// scales of justice, or `None` for no mark. `margin_class` is the spacing
/// utility that separates the mark from the text that follows it (`"me-3"`
/// on the page hero, `"me-2"` on a catalog card).
#[must_use]
pub fn product_icon(icon: Option<&str>, margin_class: &str) -> Markup {
    match icon {
        None => html! {},
        Some(LIBRA_SCALES) => libra_scales(margin_class),
        Some(glyph) => {
            let class = format!("bi bi-{glyph} {margin_class}");
            html! { i class=(class) aria-hidden="true" {} }
        }
    }
}

/// The scales of justice (Libra), drawn at a 16×16 viewBox to sit beside
/// Bootstrap Icons glyphs: a central post on a stepped base, a balanced
/// beam, and two pans on chains. Sized at `1em` and filled with
/// `currentColor`, so it inherits the surrounding text's size and color
/// exactly like a font glyph. The `.libra-scales` rule in `brand.css`
/// nudges the baseline to match the font icons.
fn libra_scales(margin_class: &str) -> Markup {
    let class = format!("libra-scales {margin_class}");
    html! {
        svg
            class=(class)
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 16 16"
            width="1em"
            height="1em"
            fill="currentColor"
            role="img"
            aria-hidden="true"
        {
            // Top knob, central post, and the stepped base.
            circle cx="8" cy="1.9" r="0.95" {}
            rect x="7.55" y="2.4" width="0.9" height="10.4" {}
            rect x="6.7" y="12.5" width="2.6" height="0.9" {}
            rect x="5.3" y="13.4" width="5.4" height="1" rx="0.5" {}
            // The balanced beam.
            rect x="2" y="3.45" width="12" height="0.95" rx="0.47" {}
            // Chains from each beam end down to the two pans.
            path
                d="M2.5 4.2 0.9 7.4M2.5 4.2 4.1 7.4M13.5 4.2 11.9 7.4M13.5 4.2 15.1 7.4"
                stroke="currentColor"
                stroke-width="0.5"
                fill="none"
                stroke-linecap="round" {}
            // The two pans, hanging level.
            path d="M0.6 7.3a1.9 1.9 0 0 0 3.8 0z" {}
            path d="M11.6 7.3a1.9 1.9 0 0 0 3.8 0z" {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{product_icon, LIBRA_SCALES};

    #[test]
    fn a_glyph_name_renders_a_bootstrap_icon() {
        let html = product_icon(Some("star-fill"), "me-3").into_string();
        assert_eq!(
            html,
            "<i class=\"bi bi-star-fill me-3\" aria-hidden=\"true\"></i>"
        );
    }

    #[test]
    fn the_margin_class_is_passed_through() {
        let html = product_icon(Some("shield-fill-check"), "me-2").into_string();
        assert_eq!(
            html,
            "<i class=\"bi bi-shield-fill-check me-2\" aria-hidden=\"true\"></i>"
        );
    }

    #[test]
    fn the_libra_scales_sentinel_renders_an_inline_svg_not_a_glyph() {
        let html = product_icon(Some(LIBRA_SCALES), "me-3").into_string();
        // An inline SVG, scoped by the sizing/baseline class, never a
        // `bi-libra-scales` font glyph (no such glyph exists).
        assert!(html.contains("<svg"), "should be inline SVG, got: {html}");
        assert!(
            html.contains("class=\"libra-scales me-3\""),
            "SVG should carry the scoping + margin class, got: {html}"
        );
        assert!(
            !html.contains("bi-libra-scales"),
            "must not emit a nonexistent font glyph, got: {html}"
        );
        assert!(
            html.contains("aria-hidden=\"true\""),
            "decorative mark should be hidden from a11y tree, got: {html}"
        );
    }

    #[test]
    fn none_renders_nothing() {
        assert_eq!(product_icon(None, "me-3").into_string(), "");
    }
}
