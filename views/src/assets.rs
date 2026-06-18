//! Responsive photography: the `asset_url` seam, the curated image
//! manifest, and the [`responsive_picture`] component.
//!
//! Photos are delivered as multi-resolution `<picture>` elements —
//! AVIF → WebP → JPEG, three width variants each — so phones download
//! the smallest file that fits their viewport. The browser picks the
//! first `<source>` whose `type` it supports, so the formats are
//! emitted smallest-first. The byte-generating
//! half (transcoding the `/tmp` sources into those variants and
//! uploading them to the `<project>-assets` bucket) lives in the
//! `cli assets build` subcommand; this module only emits the markup
//! that points at them.
//!
//! ## The `asset_url` seam
//!
//! Every photo path is resolved against [`asset_url`], which prefixes
//! `NAVIGATOR_ASSET_BASE_URL`. It defaults to `/public` so the KIND
//! dev loop, `cargo test`, and OSS forks serve the crate-bundled
//! assets with zero configuration; production points it at the Cloud
//! CDN host (e.g. `https://cdn.your-domain.example`). Nothing here is
//! hard-coded to one deployment.

use std::sync::LazyLock;

use maud::{html, Markup};

/// Width variants emitted for every photo, in ascending order. The
/// `<img>` fallback `src` uses [`FALLBACK_WIDTH`]. 1200 is the cap:
/// the source photos are ~2048px on the long edge, a full-width hero
/// reads crisp at 1200, and a 400px tile at 3× retina is exactly
/// 1200 — anything larger is bytes phones download but never show.
pub const WIDTHS: [u32; 3] = [400, 800, 1200];

/// Width of the plain `<img>` `src` fallback (browsers without
/// `srcset` support, and the resource the preload scanner fetches).
pub const FALLBACK_WIDTH: u32 = 1200;

/// Base URL every photo path resolves against. Read once: production
/// sets it via env, dev/test/OSS fall back to the crate-bundled
/// `/public` mount. Only responsive photos route through this seam;
/// vendored JS/CSS (Bootstrap, htmx, Alpine, highlight.js) is linked
/// from the literal same-origin `/public` mount in [`crate::layout`]
/// and [`crate::components::code`], so it never follows the photo CDN
/// cross-origin.
static ASSET_BASE_URL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("NAVIGATOR_ASSET_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/public".to_string())
});

/// Resolve a repo-relative asset path (e.g. `img/lake-tahoe/...`)
/// against the configured base URL.
#[must_use]
pub fn asset_url(rel: &str) -> String {
    join_base(&ASSET_BASE_URL, rel)
}

/// Pure join used by [`asset_url`]; split out so tests can exercise
/// every base form without stomping the process-wide env var (which
/// would race the parallel test runner).
fn join_base(base: &str, rel: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        rel.trim_start_matches('/')
    )
}

/// Canonical public origin of the running site (scheme + host, no
/// trailing slash), read once from `NAV_BASE_URL`. Distinct from
/// [`ASSET_BASE_URL`]: that points at the photo CDN, whereas this is
/// the app's own origin where `/public/...` is served.
///
/// Open Graph / Twitter Card scrapers (Facebook, X, Slack, iMessage,
/// `LinkedIn`, Discord) require **absolute** URLs for `og:image`;
/// relative paths are silently dropped. Empty when unset (KIND,
/// tests, an OSS fork that hasn't configured a hostname) — callers
/// then fall back to the relative path, which is fine in dev where
/// links aren't scraped by external clients.
static SITE_BASE_URL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("NAV_BASE_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
});

/// Resolve a root-relative path (`/public/logo-firm.png`) to an
/// absolute URL against [`SITE_BASE_URL`] for use in social-share
/// meta tags. Returns `rel` unchanged when it is already absolute or
/// when no base URL is configured.
#[must_use]
pub fn absolute_url(rel: &str) -> String {
    join_site(&SITE_BASE_URL, rel)
}

/// Pure join used by [`absolute_url`]; split out so tests can cover
/// every base/`rel` shape without touching the process-wide env var.
fn join_site(base: &str, rel: &str) -> String {
    if base.is_empty() || rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        rel.trim_start_matches('/')
    )
}

/// Which of the three brand stories a photo tells. Drives nothing in
/// the markup — it is the editorial axis the page authors curate by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Nevada-first: Tahoe, the Mojave, the Las Vegas Strip.
    Nevada,
    /// The globally distributed team: India, Japan.
    Global,
    /// The beautiful things in life: blossoms, birds, gardens.
    Beauty,
}

/// Aspect-ratio box a photo is presented in. Maps to Bootstrap's
/// `ratio` helpers (5.3) so the slot reserves space before the image
/// loads — this is what keeps Cumulative Layout Shift at zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    /// 21:9 cinematic letterbox — the home hero.
    Hero,
    /// 16:9 — gallery tiles, section banners.
    Wide,
    /// 4:3 — classic landscape.
    Landscape,
    /// 1:1 — square accents.
    Square,
    /// 3:4 — portrait (custom ratio; Bootstrap ships no `ratio-3x4`).
    Portrait,
}

impl Aspect {
    /// The Bootstrap `ratio-*` modifier class, or `""` when the ratio
    /// is custom (see [`Self::ratio_style`]).
    #[must_use]
    pub fn ratio_class(self) -> &'static str {
        match self {
            Aspect::Hero => "ratio-21x9",
            Aspect::Wide => "ratio-16x9",
            Aspect::Landscape => "ratio-4x3",
            Aspect::Square => "ratio-1x1",
            Aspect::Portrait => "",
        }
    }

    /// Inline `--bs-aspect-ratio` override for ratios Bootstrap does
    /// not ship as a utility class (`Portrait`), else `None`.
    #[must_use]
    pub fn ratio_style(self) -> Option<&'static str> {
        match self {
            Aspect::Portrait => Some("--bs-aspect-ratio: 133.33%"),
            _ => None,
        }
    }

    /// Nominal intrinsic `(width, height)` for the `<img>` element.
    /// The `ratio` box already reserves space; these attributes are a
    /// belt-and-suspenders CLS guard for when CSS is slow or absent.
    #[must_use]
    pub fn intrinsic(self) -> (u32, u32) {
        match self {
            Aspect::Hero => (2100, 900),
            Aspect::Wide => (1600, 900),
            Aspect::Landscape => (1600, 1200),
            Aspect::Square => (1200, 1200),
            Aspect::Portrait => (1200, 1600),
        }
    }
}

/// Loading urgency. The single above-the-fold hero is [`Eager`]; every
/// other photo is [`Lazy`] so it never competes with the Largest
/// Contentful Paint.
///
/// [`Eager`]: Priority::Eager
/// [`Lazy`]: Priority::Lazy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// `fetchpriority="high"`, eager — pair with a `<link rel=preload>`
    /// in the page `<head>` for the hero only.
    Eager,
    /// `loading="lazy" decoding="async"` — the default for everything
    /// below the fold.
    Lazy,
}

/// One curated photo. `slug` is the URL + directory stem; `source` is
/// the original filename the `cli assets build` step transcodes from.
#[derive(Debug, Clone, Copy)]
pub struct GalleryImage {
    pub slug: &'static str,
    pub theme: Theme,
    pub aspect: Aspect,
    pub alt: &'static str,
    /// Source filename under the build's input directory (`/tmp/...`).
    pub source: &'static str,
}

/// The curated set. Held back by editorial/brand rules and therefore
/// absent: the Raiders mural and Golden Knights billboard (third-party
/// trademarks in commercial advertising) and the Hiroshima A-Bomb Dome
/// (too somber for the firm's bold brand voice).
pub static GALLERY: &[GalleryImage] = &[
    // ── Nevada-first ───────────────────────────────────────────────
    GalleryImage {
        slug: "lake-tahoe",
        theme: Theme::Nevada,
        aspect: Aspect::Hero,
        alt: "Lake Tahoe ringed by snow-capped peaks and pine forest under a clear blue sky",
        source: "photo_00.jpg",
    },
    GalleryImage {
        slug: "mojave-yucca",
        theme: Theme::Nevada,
        aspect: Aspect::Portrait,
        alt: "A yucca and creosote bush on open Mojave Desert gravel under deep blue sky",
        source: "photo_16.jpg",
    },
    GalleryImage {
        slug: "desert-lizard",
        theme: Theme::Nevada,
        aspect: Aspect::Square,
        alt: "A desert lizard sunning on a pale rock with a mountain ridge behind",
        source: "photo_17.jpg",
    },
    GalleryImage {
        slug: "bellagio-horses",
        theme: Theme::Nevada,
        aspect: Aspect::Landscape,
        alt: "Glass mosaic horses leaping above red flowers in the Bellagio Conservatory",
        source: "photo_05.jpg",
    },
    GalleryImage {
        slug: "bellagio-atrium",
        theme: Theme::Nevada,
        aspect: Aspect::Portrait,
        alt: "Butterfly sculptures over a lush flower garden in a glass-domed conservatory",
        source: "photo_15.jpg",
    },
    // ── Globally distributed (India, Japan) ───────────────────────
    GalleryImage {
        slug: "bengaluru-skyline",
        theme: Theme::Global,
        aspect: Aspect::Wide,
        alt: "The green tree canopy and towers of Bengaluru under a wide cloudy sky",
        source: "photo_13.jpg",
    },
    GalleryImage {
        slug: "falaknuma-palace",
        theme: Theme::Global,
        aspect: Aspect::Wide,
        alt: "Manicured palace gardens overlooking the Hyderabad cityscape from Falaknuma Palace",
        source: "photo_10.jpg",
    },
    GalleryImage {
        slug: "india-tricolor-rangoli",
        theme: Theme::Global,
        aspect: Aspect::Portrait,
        alt: "A flower rangoli in the saffron, white and green of the Indian flag with brass bowls",
        source: "photo_12.jpg",
    },
    GalleryImage {
        slug: "kyoto-blossoms",
        theme: Theme::Global,
        aspect: Aspect::Portrait,
        alt: "White cherry blossoms in front of a Kyoto temple gate under blue sky",
        source: "photo_01.jpg",
    },
    // ── Beautiful things in life ──────────────────────────────────
    GalleryImage {
        slug: "migrating-birds",
        theme: Theme::Beauty,
        aspect: Aspect::Wide,
        alt: "A loose V of migrating birds crossing a deep blue sky",
        source: "photo_06.jpg",
    },
    GalleryImage {
        slug: "yellow-rose",
        theme: Theme::Beauty,
        aspect: Aspect::Portrait,
        alt: "A single full-bloom yellow rose against dark green leaves",
        source: "photo_09.jpg",
    },
    GalleryImage {
        slug: "lantana",
        theme: Theme::Beauty,
        aspect: Aspect::Wide,
        alt: "Clusters of pink, orange and yellow lantana flowers among green leaves",
        source: "photo_11.jpg",
    },
    GalleryImage {
        slug: "wa-capitol-blossoms",
        theme: Theme::Beauty,
        aspect: Aspect::Portrait,
        alt: "The Washington State Capitol dome framed by pink cherry blossoms at golden hour",
        source: "photo_14.jpg",
    },
];

/// Look up a curated photo by slug.
#[must_use]
pub fn find(slug: &str) -> Option<&'static GalleryImage> {
    GALLERY.iter().find(|img| img.slug == slug)
}

/// Build the `srcset` string for one format: every [`WIDTHS`] variant
/// paired with its width descriptor.
fn srcset(slug: &str, ext: &str) -> String {
    WIDTHS
        .iter()
        .map(|w| format!("{} {w}w", variant_url(slug, *w, ext)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// URL for a single variant. The path is stable across builds, so the
/// `/public` mount (and, in production, the CDN) caches it under a
/// bounded TTL and re-fetches when it expires — no cache-bust token.
fn variant_url(slug: &str, width: u32, ext: &str) -> String {
    asset_url(&format!("img/{slug}/{slug}-{width}w.{ext}"))
}

/// Render a responsive `<picture>` for a curated photo.
///
/// - `sizes` is the standard `sizes` attribute — how wide the image
///   renders at each breakpoint (e.g. `"100vw"` for a hero, or
///   `"(min-width: 768px) 33vw, 100vw"` for a three-up gallery row).
/// - `priority` controls eager-vs-lazy loading; use [`Priority::Eager`]
///   for the one hero and a matching `<link rel=preload>` in `<head>`.
#[must_use]
pub fn responsive_picture(img: &GalleryImage, sizes: &str, priority: Priority) -> Markup {
    let (w, h) = img.aspect.intrinsic();
    let ratio_class = format!("ratio {}", img.aspect.ratio_class());
    let avif = srcset(img.slug, "avif");
    let webp = srcset(img.slug, "webp");
    let jpeg = srcset(img.slug, "jpg");
    let fallback = variant_url(img.slug, FALLBACK_WIDTH, "jpg");
    html! {
        div class=(ratio_class.trim()) style=[img.aspect.ratio_style()] {
            picture {
                source type="image/avif" srcset=(avif) sizes=(sizes);
                source type="image/webp" srcset=(webp) sizes=(sizes);
                img
                    src=(fallback)
                    srcset=(jpeg)
                    sizes=(sizes)
                    alt=(img.alt)
                    width=(w)
                    height=(h)
                    class="img-fluid object-fit-cover w-100 h-100"
                    loading=(if priority == Priority::Eager { "eager" } else { "lazy" })
                    decoding=(if priority == Priority::Eager { "auto" } else { "async" })
                    fetchpriority=(if priority == Priority::Eager { "high" } else { "auto" });
            }
        }
    }
}

/// Render the curated photo with `slug`, or nothing if the slug is
/// unknown. The ergonomic entry point for pages — they never `unwrap`
/// the manifest.
#[must_use]
pub fn picture(slug: &str, sizes: &str, priority: Priority) -> Markup {
    find(slug).map_or_else(|| html! {}, |img| responsive_picture(img, sizes, priority))
}

/// Render a curated photo as a full-width **banner** — the wide image
/// that leads a Notion-style page, edge to edge across the content
/// column. Unlike [`picture`], there is no aspect-ratio box: the `<img>`
/// fills the column width and is cropped to a short banner height by the
/// `.service-banner-img` rule in `brand.css`, so any source aspect
/// (portrait or wide) reads as a clean horizontal band. Renders nothing
/// for an unknown slug.
#[must_use]
pub fn banner(slug: &str, priority: Priority) -> Markup {
    find(slug).map_or_else(
        || html! {},
        |img| {
            let avif = srcset(img.slug, "avif");
            let webp = srcset(img.slug, "webp");
            let jpeg = srcset(img.slug, "jpg");
            let fallback = variant_url(img.slug, FALLBACK_WIDTH, "jpg");
            html! {
                picture {
                    source type="image/avif" srcset=(avif) sizes="100vw";
                    source type="image/webp" srcset=(webp) sizes="100vw";
                    img
                        src=(fallback)
                        srcset=(jpeg)
                        sizes="100vw"
                        alt=(img.alt)
                        class="service-banner-img img-fluid w-100"
                        loading=(if priority == Priority::Eager { "eager" } else { "lazy" })
                        decoding=(if priority == Priority::Eager { "auto" } else { "async" })
                        fetchpriority=(if priority == Priority::Eager { "high" } else { "auto" });
                }
            }
        },
    )
}

/// Preload `<link>` href for a hero photo's `<img>` fallback (the
/// resource the browser's preload scanner fetches). Pages pass this to
/// [`crate::layout::PageLayout::with_preload`] so the Largest
/// Contentful Paint image starts downloading before the body parses.
/// `None` for an unknown slug.
#[must_use]
pub fn preload_href(slug: &str) -> Option<String> {
    find(slug).map(|img| variant_url(img.slug, FALLBACK_WIDTH, "jpg"))
}

#[cfg(test)]
mod tests {
    use super::{
        asset_url, find, join_base, join_site, responsive_picture, Aspect, Priority, Theme,
        GALLERY, WIDTHS,
    };

    #[test]
    fn asset_url_defaults_to_public_mount() {
        // With no env override the seam must resolve to the
        // crate-bundled `/public` mount so dev/KIND/tests work.
        assert_eq!(
            asset_url("img/lake-tahoe/lake-tahoe-800w.avif"),
            "/public/img/lake-tahoe/lake-tahoe-800w.avif"
        );
    }

    #[test]
    fn join_base_normalizes_slashes_for_any_base() {
        // Trailing slash on base, leading slash on rel — exactly one
        // separator either way.
        assert_eq!(join_base("/public", "img/a.avif"), "/public/img/a.avif");
        assert_eq!(join_base("/public/", "/img/a.avif"), "/public/img/a.avif");
        assert_eq!(
            join_base("https://cdn.example.com", "img/a.avif"),
            "https://cdn.example.com/img/a.avif"
        );
    }

    #[test]
    fn join_site_makes_relative_paths_absolute_against_the_origin() {
        // With a configured origin, a `/public/...` logo path becomes
        // the absolute URL a social scraper can fetch.
        assert_eq!(
            join_site("https://www.neonlaw.com", "/public/logo-firm.png"),
            "https://www.neonlaw.com/public/logo-firm.png"
        );
        // Trailing slash on base, no leading slash on rel — still one
        // separator.
        assert_eq!(
            join_site("https://www.neonlaw.com/", "public/logo-firm.png"),
            "https://www.neonlaw.com/public/logo-firm.png"
        );
    }

    #[test]
    fn join_site_passes_through_when_unconfigured_or_already_absolute() {
        // No NAV_BASE_URL (dev/KIND/tests): keep the relative path.
        assert_eq!(
            join_site("", "/public/logo-firm.png"),
            "/public/logo-firm.png"
        );
        // Already-absolute image (e.g. a CDN URL): leave it untouched
        // rather than double-prefixing.
        assert_eq!(
            join_site(
                "https://www.neonlaw.com",
                "https://cdn.example.com/logo.png"
            ),
            "https://cdn.example.com/logo.png"
        );
    }

    #[test]
    fn gallery_excludes_trademarked_and_somber_images() {
        // Editorial/brand guardrail: the held-back set must never be
        // wired into a marketing surface via the manifest.
        for banned in ["raider", "golden-knight", "cosmopolitan", "hiroshima"] {
            assert!(
                GALLERY.iter().all(|i| !i.slug.contains(banned)),
                "manifest must not carry held-back image `{banned}`"
            );
        }
    }

    #[test]
    fn gallery_covers_all_three_themes() {
        for theme in [Theme::Nevada, Theme::Global, Theme::Beauty] {
            assert!(
                GALLERY.iter().any(|i| i.theme == theme),
                "every brand theme needs at least one photo: {theme:?}"
            );
        }
    }

    #[test]
    fn gallery_slugs_are_unique() {
        let mut slugs: Vec<_> = GALLERY.iter().map(|i| i.slug).collect();
        slugs.sort_unstable();
        let before = slugs.len();
        slugs.dedup();
        assert_eq!(before, slugs.len(), "gallery slugs must be unique");
    }

    #[test]
    fn the_hero_is_lake_tahoe_in_nevada() {
        let hero = find("lake-tahoe").expect("lake-tahoe in manifest");
        assert_eq!(hero.theme, Theme::Nevada);
        assert_eq!(hero.aspect, Aspect::Hero);
    }

    #[test]
    fn picture_helper_renders_known_slug_and_swallows_unknown() {
        use super::picture;
        let known = picture("lake-tahoe", "100vw", Priority::Eager).into_string();
        assert!(known.contains("<picture>") && known.contains("lake-tahoe"));
        // Unknown slug must not panic — it renders empty.
        assert_eq!(
            picture("no-such-photo", "100vw", Priority::Lazy).into_string(),
            ""
        );
    }

    #[test]
    fn preload_href_points_at_the_jpeg_fallback() {
        use super::{preload_href, FALLBACK_WIDTH};
        let href = preload_href("lake-tahoe").expect("hero has a preload href");
        assert!(href.contains(&format!("lake-tahoe-{FALLBACK_WIDTH}w.jpg")));
        assert!(preload_href("no-such-photo").is_none());
    }

    #[test]
    fn picture_offers_avif_then_webp_then_the_jpeg_fallback() {
        let img = find("lake-tahoe").unwrap();
        let out = responsive_picture(img, "100vw", Priority::Eager).into_string();
        // The browser picks the first <source> whose type it supports,
        // so formats are emitted smallest-first: AVIF, then WebP, then
        // the universal JPEG <img> fallback. A browser that supports
        // AVIF must never download the larger WebP or JPEG.
        let avif = out.find("image/avif").expect("avif source");
        let webp = out.find("image/webp").expect("webp source");
        let img_tag = out.find("<img").expect("img fallback");
        assert!(
            avif < webp && webp < img_tag,
            "order must be AVIF <source> → WebP <source> → <img> fallback"
        );
        assert!(out.contains(".avif") && out.contains(".webp") && out.contains(".jpg"));
    }

    #[test]
    fn picture_emits_every_width_descriptor() {
        let img = find("migrating-birds").unwrap();
        let out = responsive_picture(img, "100vw", Priority::Lazy).into_string();
        for w in WIDTHS {
            assert!(
                out.contains(&format!("{w}w")),
                "missing {w}w descriptor: {out}"
            );
        }
    }

    #[test]
    fn eager_hero_sets_high_fetchpriority_and_not_lazy() {
        let img = find("lake-tahoe").unwrap();
        let out = responsive_picture(img, "100vw", Priority::Eager).into_string();
        assert!(out.contains("fetchpriority=\"high\""));
        assert!(out.contains("loading=\"eager\""));
        assert!(!out.contains("loading=\"lazy\""));
    }

    #[test]
    fn lazy_image_defers_loading_and_decoding() {
        let img = find("yellow-rose").unwrap();
        let out =
            responsive_picture(img, "(min-width: 768px) 33vw, 100vw", Priority::Lazy).into_string();
        assert!(out.contains("loading=\"lazy\""));
        assert!(out.contains("decoding=\"async\""));
        assert!(out.contains("fetchpriority=\"auto\""));
    }

    #[test]
    fn picture_reserves_space_with_ratio_box_and_intrinsic_dims() {
        // Ratio wrapper + width/height kill Cumulative Layout Shift.
        let img = find("lake-tahoe").unwrap();
        let out = responsive_picture(img, "100vw", Priority::Eager).into_string();
        assert!(
            out.contains("ratio ratio-21x9"),
            "hero needs a 21:9 ratio box: {out}"
        );
        assert!(out.contains("width=\"2100\"") && out.contains("height=\"900\""));
        assert!(out.contains("object-fit-cover"));
    }

    #[test]
    fn portrait_uses_custom_ratio_style_not_a_missing_class() {
        // Bootstrap ships no `ratio-3x4`; portrait must fall back to
        // the inline `--bs-aspect-ratio` custom property.
        let img = find("yellow-rose").unwrap();
        assert_eq!(img.aspect, Aspect::Portrait);
        let out = responsive_picture(img, "100vw", Priority::Lazy).into_string();
        assert!(
            out.contains("--bs-aspect-ratio: 133.33%"),
            "portrait needs custom ratio: {out}"
        );
    }
}
