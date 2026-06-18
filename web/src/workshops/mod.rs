//! Workshop materials, loaded once at boot from a content directory.
//!
//! Each workshop is a folder under the content root; each material is
//! a `.md` file inside. We bake the manifest into the binary so the
//! ordering and titles are stable even if the on-disk files get
//! reorganized.

pub mod loader;

use std::sync::Arc;

/// One step in a workshop — the content under a single `##` heading,
/// rendered for the "one thing at a time" classroom flow. The reader
/// walks these one URL at a time (`/…/:slug/step/:n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopSection {
    /// The heading text, used for the table of contents and the
    /// progress label.
    pub title: String,
    /// Pre-rendered HTML for this section (includes its own `<h2>`).
    pub body_html: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopMaterial {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub raw_markdown: String,
    /// Full rendered body with the leading `#` title stripped — the
    /// page chrome supplies the sole `<h1>`, so the markdown must not
    /// repeat it.
    pub body_html: String,
    /// Rendered HTML for everything before the first `##` heading —
    /// the workshop's orientation lede, shown on the overview page.
    pub intro_html: String,
    /// Ordered steps, one per `##` heading. Empty for materials with
    /// no second-level headings (they render as a single page).
    pub sections: Vec<WorkshopSection>,
}

#[derive(Debug, Clone)]
pub struct WorkshopIndex {
    materials: Arc<Vec<WorkshopMaterial>>,
}

impl WorkshopIndex {
    #[must_use]
    pub fn new(materials: Vec<WorkshopMaterial>) -> Self {
        Self {
            materials: Arc::new(materials),
        }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    #[must_use]
    pub fn materials(&self) -> &[WorkshopMaterial] {
        &self.materials
    }

    #[must_use]
    pub fn find(&self, slug: &str) -> Option<&WorkshopMaterial> {
        self.materials.iter().find(|m| m.slug == slug)
    }
}
