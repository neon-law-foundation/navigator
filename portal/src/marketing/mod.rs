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
}

/// `Arc`-wrapped lookup shared as router state. Cheap to clone.
///
/// Holds every marketing doc, keyed by slug.
#[derive(Debug, Clone)]
pub struct MarketingIndex {
    docs: Arc<Vec<MarketingDoc>>,
}

impl MarketingIndex {
    #[must_use]
    pub fn new(docs: Vec<MarketingDoc>) -> Self {
        Self {
            docs: Arc::new(docs),
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    #[must_use]
    pub fn docs(&self) -> &[MarketingDoc] {
        &self.docs
    }

    /// Find a doc by slug.
    #[must_use]
    pub fn find(&self, slug: &str) -> Option<&MarketingDoc> {
        self.docs.iter().find(|d| d.slug == slug)
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
