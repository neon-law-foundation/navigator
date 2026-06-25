//! Workshop materials, loaded once at boot from a content directory.
//!
//! Each workshop is a folder under the content root; each material is
//! a `.md` file inside. We bake the manifest into the binary so the
//! ordering and titles are stable even if the on-disk files get
//! reorganized.

pub mod loader;

use std::sync::Arc;

/// One slide in a workshop — the content under a single `##` heading,
/// rendered for the Keynote-style classroom flow. The reader walks
/// these one URL at a time (`/…/:slug/step/:n`) or scans them all in
/// the light-table grid (`/…/:slug/slides`).
///
/// Each slide is authored as a `##` section whose body may carry a
/// thematic-break divider (`---`): everything above is the **slide
/// face** ([`Self::body_html`]); everything below is the **presenter
/// notes** ([`Self::notes_html`]). The workshop-format invariant test
/// (`every_workshop_section_has_presenter_notes`) requires every slide
/// to carry notes, so the divider is mandatory in shipped content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopSection {
    /// The heading text, used for the table of contents and the
    /// progress label.
    pub title: String,
    /// Pre-rendered HTML for the slide face (includes its own `<h2>`) —
    /// the content above the `---` divider.
    pub body_html: String,
    /// Pre-rendered HTML for the presenter notes — the content below the
    /// `---` divider. Empty only for legacy/unsplit sections; shipped
    /// workshops always populate it (enforced by the format test).
    pub notes_html: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopMaterial {
    /// Public Nebula category path segment, e.g. `workshops` or
    /// `presentations`.
    pub category: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Who this material is for, shown as the audience tag on the
    /// top-level `/foundation/nebula` overview, e.g. "For lawyers".
    /// The reader self-selects in two seconds (Client Council, Pisces).
    pub audience: String,
    /// The you-voiced takeaway — what the reader walks out with —
    /// rendered as the card body on the overview. Describes what the
    /// reader *does*, never a guaranteed outcome (Legal Council,
    /// Scorpio: this is public attorney advertising across CA/NV/WA).
    pub benefit: String,
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

    #[must_use]
    pub fn find_in_category(&self, category: &str, slug: &str) -> Option<&WorkshopMaterial> {
        self.materials
            .iter()
            .find(|m| m.category == category && m.slug == slug)
    }
}
