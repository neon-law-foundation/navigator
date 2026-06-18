//! Open Graph + Twitter Card meta tags — the social-share "preview
//! card" partial.
//!
//! When a link to the site is pasted into iMessage, Android Messages,
//! Slack, Facebook, X, LinkedIn, or Discord, those clients fetch the
//! page and read these `<meta>` tags to render a rich preview: the
//! brand logo, the page title, and a one-line message. Without them
//! the link shows as bare text. The tags live in `<head>` and are
//! emitted for every page by [`crate::layout::PageLayout`].

use maud::{html, Markup};

use crate::assets::absolute_url;
use crate::brand::SiteBrand;

/// Inputs for [`social_meta`]. `title` and `description` mirror the
/// page's `<title>` and `meta description`; `brand` supplies the
/// logo image and the site name.
pub struct SocialMeta<'a> {
    /// Full document title, e.g. `"Neon Law Foundation | Mission"`.
    pub title: &'a str,
    /// One-line share message — the page description, or the brand
    /// tagline when a page sets none.
    pub description: &'a str,
    pub brand: &'a SiteBrand,
}

/// Render the Open Graph + Twitter Card `<meta>` block for one page.
#[must_use]
pub fn social_meta(meta: &SocialMeta<'_>) -> Markup {
    // Scrapers drop relative `og:image` URLs, so resolve the brand's
    // PNG mark against the site origin (`NAV_BASE_URL`). In dev, where
    // no origin is configured, the path stays relative — fine, since
    // local links aren't scraped by external clients.
    let image = absolute_url(meta.brand.social_image);
    let image_alt = format!("{} logo", meta.brand.site_name);
    html! {
        // Open Graph — Facebook, iMessage, Slack, LinkedIn, Discord.
        meta property="og:type" content="website";
        meta property="og:site_name" content=(meta.brand.site_name);
        meta property="og:title" content=(meta.title);
        meta property="og:description" content=(meta.description);
        meta property="og:image" content=(image);
        meta property="og:image:alt" content=(image_alt);
        // Twitter / X. `summary` renders the square logo as a small
        // thumbnail; the wide `summary_large_image` card expects a
        // 1.91:1 banner, which a square mark would letterbox.
        meta name="twitter:card" content="summary";
        meta name="twitter:title" content=(meta.title);
        meta name="twitter:description" content=(meta.description);
        meta name="twitter:image" content=(image);
    }
}

#[cfg(test)]
mod tests {
    use super::{social_meta, SocialMeta};
    use crate::brand::{FIRM_BRAND, FOUNDATION_BRAND};

    #[test]
    fn emits_open_graph_title_description_and_site_name() {
        let out = social_meta(&SocialMeta {
            title: "Neon Law | Home",
            description: "A small firm built for access to justice.",
            brand: &FIRM_BRAND,
        })
        .into_string();
        assert!(out.contains("property=\"og:title\" content=\"Neon Law | Home\""));
        assert!(out.contains(
            "property=\"og:description\" content=\"A small firm built for access to justice.\""
        ));
        assert!(
            out.contains(&format!(
                "property=\"og:site_name\" content=\"{}\"",
                FIRM_BRAND.site_name
            )),
            "got: {out}"
        );
        assert!(out.contains("property=\"og:type\" content=\"website\""));
    }

    #[test]
    fn emits_twitter_summary_card_mirroring_open_graph() {
        let out = social_meta(&SocialMeta {
            title: "Neon Law | Home",
            description: "msg",
            brand: &FIRM_BRAND,
        })
        .into_string();
        // A square logo belongs in the small `summary` card, not the
        // 1.91:1 `summary_large_image` banner.
        assert!(out.contains("name=\"twitter:card\" content=\"summary\""));
        assert!(!out.contains("summary_large_image"));
        assert!(out.contains("name=\"twitter:title\" content=\"Neon Law | Home\""));
        assert!(out.contains("name=\"twitter:image\""));
    }

    #[test]
    fn image_points_at_the_brand_raster_mark_with_alt_text() {
        let firm = social_meta(&SocialMeta {
            title: "t",
            description: "d",
            brand: &FIRM_BRAND,
        })
        .into_string();
        // The image is the PNG (not the SVG favicon) so scrapers render it.
        assert!(
            firm.contains("logo-firm.png"),
            "firm og:image should be the PNG mark, got: {firm}"
        );
        assert!(firm.contains(&format!(
            "property=\"og:image:alt\" content=\"{} logo\"",
            FIRM_BRAND.site_name
        )));

        let foundation = social_meta(&SocialMeta {
            title: "t",
            description: "d",
            brand: &FOUNDATION_BRAND,
        })
        .into_string();
        assert!(
            foundation.contains("logo-foundation.png"),
            "foundation og:image should be the NLF PNG mark, got: {foundation}"
        );
    }
}
