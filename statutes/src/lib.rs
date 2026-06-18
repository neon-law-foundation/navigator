//! Weekly Nevada Revised Statutes scraper + public-reference data layer.
//!
//! The library fetches the practice-relevant NRS chapters, parses each into
//! sections, and reconciles them into Postgres via the insert-only
//! `store::statutes` helpers. Scraping is idempotent: a re-run is a no-op
//! for unchanged sections and a mid-run crash loses nothing.
//!
//! The weekly run is the two-step [`Statutes`](workflow::Statutes) Restate
//! workflow — `scrape` (this library's [`run_sync`]) then `email` (a
//! Foundation-branded summary) — hosted by the `workflows-service` worker
//! and started by the thin `trigger` `CronJob`, the same shape as
//! `archives`. The `statutes-sync` bin remains as a manual/dev entrypoint
//! that runs the scrape directly, without the broker.
//!
//! Module map:
//! - [`parse`] — pure HTML → sections (tested against a saved fixture).
//! - [`fetch`] — polite, rate-limited HTTP with windows-1252 decoding.
//! - [`sync`] — per-chapter orchestration with failure isolation.
//! - [`workflow`] — the durable `Statutes` workflow (scrape → email).
//! - [`email`] — the Foundation-branded weekly summary email.

pub mod email;
pub mod fetch;
pub mod parse;
pub mod sync;
pub mod workflow;

pub use parse::{parse_chapter, ParseError, ParsedChapter, ParsedSection};
pub use sync::{run_sync, scrape_one_chapter, ChapterResult, SyncSummary};
pub use workflow::{ScrapeReport, Statutes, StatutesService};

/// Jurisdiction every scraped section belongs to.
pub const JURISDICTION: &str = "NV";
/// Code abbreviation for the Nevada Revised Statutes.
pub const CODE: &str = "NRS";

/// Descriptive bot identity sent on every request, per the
/// good-citizen rule — names the Foundation and links its site so the
/// legislature's operators can identify and contact us.
pub const USER_AGENT: &str = "NeonLawFoundationBot/1.0 (+https://www.neonlaw.org)";

/// One chapter to scrape and the product it supports (drives the
/// `/statutes` index grouping). Expanding coverage is editing this list.
#[derive(Debug, Clone, Copy)]
pub struct ChapterSpec {
    /// Chapter number as the source prints it (`86`, `86A`, `118A`).
    pub chapter: &'static str,
    /// The Neon Law product this chapter supports.
    pub product: &'static str,
}

/// The practice-relevant chapters (v1, full breadth). Single source of
/// truth for both the scraper and the render-surface grouping.
///
/// - **Nest** (Nevada entities): NRS 78, 82, 86, 86A.
/// - **Northstar** (wills / trusts / probate): NRS 132–156, 163.
/// - **Tenant defense**: NRS 118A.
/// - **Nautilus** (debt collection): NRS 649.
pub const CHAPTERS: &[ChapterSpec] = &[
    ChapterSpec {
        chapter: "78",
        product: "Nest",
    },
    ChapterSpec {
        chapter: "82",
        product: "Nest",
    },
    ChapterSpec {
        chapter: "86",
        product: "Nest",
    },
    ChapterSpec {
        chapter: "86A",
        product: "Nest",
    },
    ChapterSpec {
        chapter: "118A",
        product: "Tenant defense",
    },
    ChapterSpec {
        chapter: "132",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "133",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "134",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "135",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "136",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "137",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "138",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "139",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "140",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "141",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "142",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "143",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "144",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "145",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "146",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "147",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "148",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "149",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "150",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "151",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "152",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "153",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "154",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "155",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "156",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "163",
        product: "Northstar",
    },
    ChapterSpec {
        chapter: "649",
        product: "Nautilus",
    },
];

/// The product a chapter supports, or `None` if the chapter isn't in
/// [`CHAPTERS`]. Lets the `/statutes` render surface group chapters by
/// product without duplicating the mapping.
#[must_use]
pub fn product_for(chapter: &str) -> Option<&'static str> {
    CHAPTERS
        .iter()
        .find(|c| c.chapter == chapter)
        .map(|c| c.product)
}

/// Default base URL for the NRS chapter pages. Overridable with
/// `STATUTES_NRS_BASE_URL` so an OSS fork (or a test) can point
/// elsewhere; nothing about the host is hard-coded into logic.
pub const DEFAULT_NRS_BASE_URL: &str = "https://www.leg.state.nv.us/NRS/";

/// Build the chapter page URL: `NRS-0NN.html`, the numeric part
/// zero-padded to three digits with any alpha suffix preserved
/// (`86` → `NRS-086.html`, `118A` → `NRS-118A.html`).
#[must_use]
pub fn chapter_url(base: &str, chapter: &str) -> String {
    let digits: String = chapter.chars().take_while(char::is_ascii_digit).collect();
    let suffix: String = chapter.chars().skip_while(char::is_ascii_digit).collect();
    let base = base.strip_suffix('/').unwrap_or(base);
    format!("{base}/NRS-{digits:0>3}{suffix}.html")
}

#[cfg(test)]
mod tests {
    use super::{chapter_url, CHAPTERS};

    #[test]
    fn url_zero_pads_numeric_part_and_preserves_suffix() {
        let base = "https://www.leg.state.nv.us/NRS/";
        assert_eq!(
            chapter_url(base, "86"),
            "https://www.leg.state.nv.us/NRS/NRS-086.html"
        );
        assert_eq!(
            chapter_url(base, "86A"),
            "https://www.leg.state.nv.us/NRS/NRS-086A.html"
        );
        assert_eq!(
            chapter_url(base, "118A"),
            "https://www.leg.state.nv.us/NRS/NRS-118A.html"
        );
        assert_eq!(
            chapter_url(base, "649"),
            "https://www.leg.state.nv.us/NRS/NRS-649.html"
        );
    }

    #[test]
    fn url_builder_tolerates_trailing_slash_either_way() {
        assert_eq!(
            chapter_url("http://x/NRS", "78"),
            "http://x/NRS/NRS-078.html"
        );
        assert_eq!(
            chapter_url("http://x/NRS/", "78"),
            "http://x/NRS/NRS-078.html"
        );
    }

    #[test]
    fn chapter_list_covers_every_product_family() {
        let products: std::collections::BTreeSet<_> = CHAPTERS.iter().map(|c| c.product).collect();
        assert!(products.contains("Nest"));
        assert!(products.contains("Northstar"));
        assert!(products.contains("Tenant defense"));
        assert!(products.contains("Nautilus"));
        // sanity: the full Northstar probate range is present
        assert!(CHAPTERS.iter().any(|c| c.chapter == "132"));
        assert!(CHAPTERS.iter().any(|c| c.chapter == "156"));
        assert!(CHAPTERS.iter().any(|c| c.chapter == "163"));
    }
}
