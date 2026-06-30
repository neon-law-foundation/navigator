//! Marketing copy for the public landing pages — hero text and
//! supporting sections that the team edits more often than they
//! ship code. Loaded once at boot from a directory of `.md` files
//! and looked up by slug at request time.
//!
//! A marketing slug is a stable identifier (`home`, `foundation`,
//! `estate`, `corporate`, `colossus`, `cles`, …) that a view
//! handler asks for. Front-matter declares the page title and
//! short description; the body is rendered to HTML via
//! pulldown-cmark and embedded with `PreEscaped` in the view.

pub mod loader;

use std::collections::HashMap;
use std::sync::Arc;

/// One marketing fragment.
///
/// `metadata` holds frontmatter keys that aren't one of the four
/// well-known fields (`title`, `slug`, `description`, body). Long-lived
/// content uses it for partner-org details on `/help` entries and
/// `bar_admissions` on `/about` bios — fields the page renderer reads
/// by name. Unknown keys round-trip so the loader stays decoupled
/// from the schema of any one content tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketingDoc {
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Rendered HTML body (NOT raw markdown).
    pub body_html: String,
    pub metadata: HashMap<String, String>,
    /// Pricing / offer cards declared in the page's `pricing:`
    /// frontmatter block. Empty for pages that don't advertise a
    /// price. The view maps these onto [`views::components::PricingCard`]
    /// at render time.
    pub pricing: Vec<PricingCard>,
}

/// One pricing / offer card as authored in marketing frontmatter.
///
/// Owned mirror of the borrowed [`views::components::PricingCard`] the
/// renderer consumes — the `web` crate owns the content schema, `views`
/// owns the markup, and `render_service` maps one onto the other per
/// request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PricingCard {
    /// Outcome-led title — what the client gets, not the work we do.
    pub title: String,
    /// Headline number verbatim, including any range marker
    /// (`"from $1,000"`).
    pub price: String,
    /// Billing cadence (`"/mo"`); omit when the fee label already
    /// carries the timing.
    #[serde(default)]
    pub cadence: Option<String>,
    /// One line answering "is this for someone like me?".
    #[serde(default)]
    pub blurb: String,
    /// Inclusion bullets; may be empty for simple flat-fee offers.
    #[serde(default)]
    pub features: Vec<String>,
    pub cta_label: String,
    pub cta_href: String,
    /// The shared renderer gives every pricing card the highlighted
    /// flat-fee treatment regardless of this marker.
    #[serde(default)]
    pub featured: bool,
    /// Label for the cyan band. No "most popular" claims — they trip
    /// attorney-advertising rules.
    #[serde(default)]
    pub featured_label: Option<String>,
}

/// `Arc`-wrapped lookup shared as router state. Cheap to clone.
///
/// Holds the English (source) docs plus one parallel set per non-source
/// locale. A localized lookup falls back to English when the slug has no
/// twin in that locale, so an untranslated page degrades gracefully
/// instead of 404-ing. See [`docs/i18n.md`](../../../docs/i18n.md).
#[derive(Debug, Clone)]
pub struct MarketingIndex {
    docs: Arc<Vec<MarketingDoc>>,
    es: Arc<Vec<MarketingDoc>>,
}

impl MarketingIndex {
    #[must_use]
    pub fn new(docs: Vec<MarketingDoc>) -> Self {
        Self {
            docs: Arc::new(docs),
            es: Arc::new(Vec::new()),
        }
    }

    /// Attach the Spanish (`es`) document set. Builder-style so existing
    /// `MarketingIndex::new(docs)` call sites are unchanged.
    #[must_use]
    pub fn with_es(mut self, es: Vec<MarketingDoc>) -> Self {
        self.es = Arc::new(es);
        self
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    #[must_use]
    pub fn docs(&self) -> &[MarketingDoc] {
        &self.docs
    }

    /// Find a doc by slug in the English (source) set.
    #[must_use]
    pub fn find(&self, slug: &str) -> Option<&MarketingDoc> {
        self.docs.iter().find(|d| d.slug == slug)
    }

    /// Find a doc by slug in `locale`, falling back to the English doc
    /// when the locale has no twin for that slug. `Locale::En` is the
    /// same as [`find`](Self::find).
    #[must_use]
    pub fn find_localized(&self, slug: &str, locale: views::Locale) -> Option<&MarketingDoc> {
        match locale {
            views::Locale::En => self.find(slug),
            views::Locale::Es => self
                .es
                .iter()
                .find(|d| d.slug == slug)
                .or_else(|| self.find(slug)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MarketingDoc, MarketingIndex};

    fn doc(slug: &str) -> MarketingDoc {
        MarketingDoc {
            slug: slug.into(),
            title: format!("Title {slug}"),
            description: "desc".into(),
            body_html: "<p>x</p>".into(),
            metadata: std::collections::HashMap::new(),
            pricing: Vec::new(),
        }
    }

    #[test]
    fn metadata_round_trips_through_the_struct() {
        let mut meta = std::collections::HashMap::new();
        meta.insert("topic".into(), "immigration".into());
        meta.insert("phone".into(), "1-800-555-0199".into());
        let d = MarketingDoc {
            slug: "x".into(),
            title: "t".into(),
            description: "d".into(),
            body_html: String::new(),
            metadata: meta,
            pricing: Vec::new(),
        };
        assert_eq!(
            d.metadata.get("topic").map(String::as_str),
            Some("immigration")
        );
        assert_eq!(
            d.metadata.get("phone").map(String::as_str),
            Some("1-800-555-0199")
        );
    }

    #[test]
    fn empty_index_finds_nothing() {
        let ix = MarketingIndex::empty();
        assert!(ix.docs().is_empty());
        assert!(ix.find("home").is_none());
    }

    #[test]
    fn find_returns_doc_when_slug_matches() {
        let ix = MarketingIndex::new(vec![doc("home"), doc("foundation")]);
        assert_eq!(ix.find("home").map(|d| d.slug.as_str()), Some("home"));
        assert_eq!(
            ix.find("foundation").map(|d| d.slug.as_str()),
            Some("foundation")
        );
        assert!(ix.find("missing").is_none());
    }
}
