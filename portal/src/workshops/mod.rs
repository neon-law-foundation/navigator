//! Workshop materials, loaded once at boot from a content directory.
//!
//! Each workshop is a folder under the content root; each material is
//! a `.md` file inside. We bake the manifest into the binary so the
//! ordering and titles are stable even if the on-disk files get
//! reorganized.

pub mod loader;

use std::sync::Arc;

/// One slide in a workshop — the content under a single `###` heading,
/// rendered for the Keynote-style classroom flow. The reader walks
/// these one URL at a time (`/…/:slug/step/:n`) or scans them all in
/// the light-table grid (`/…/:slug/slides`).
///
/// Each slide is authored as a `###` section beneath a `##` chapter. Its body may carry a
/// thematic-break divider (`---`): everything above is the **slide
/// face** ([`Self::body_html`]); everything below is the **presenter
/// notes** ([`Self::notes_html`]). The workshop-format invariant test
/// (`every_material_has_chapters_and_section_notes`) requires every slide
/// to carry notes, so the divider is mandatory in shipped content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopSection {
    /// The heading text, used for the table of contents and the
    /// progress label.
    pub title: String,
    /// Pre-rendered HTML for the slide face (includes its own `<h3>`) —
    /// the content above the `---` divider.
    pub body_html: String,
    /// Pre-rendered HTML for the presenter notes — the content below the
    /// `---` divider. Shipped workshops always populate it (enforced by
    /// the format test).
    pub notes_html: String,
}

/// One authored chapter in a workshop or presentation. Chapters group a
/// contiguous range of sections without changing the flat, stable playback
/// order used by `/step/:n`, display mode, presentation mode, and progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopChapter {
    pub title: String,
    /// Rendered prose authored between this chapter's `##` heading and its
    /// first `###` section. It introduces the chapter on the outline page
    /// without becoming a numbered slide.
    pub preamble_html: String,
    /// Zero-based index of this chapter's first section in
    /// [`WorkshopMaterial::sections`].
    pub section_start: usize,
    pub section_count: usize,
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
    /// Ordered chapter groups. Authored `##` headings become chapters and
    /// their `###` children become the flat sections below.
    pub chapters: Vec<WorkshopChapter>,
    /// Ordered sections, one per `###` heading. Empty for materials with
    /// no authored sections (they render as a single page).
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
    pub fn find_in_category(&self, category: &str, slug: &str) -> Option<&WorkshopMaterial> {
        self.materials
            .iter()
            .find(|m| m.category == category && m.slug == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkshopIndex, WorkshopMaterial};

    fn material(category: &str, slug: &str, title: &str) -> WorkshopMaterial {
        WorkshopMaterial {
            category: category.to_string(),
            slug: slug.to_string(),
            title: title.to_string(),
            description: String::new(),
            audience: String::new(),
            benefit: String::new(),
            raw_markdown: String::new(),
            body_html: String::new(),
            intro_html: String::new(),
            chapters: Vec::new(),
            sections: Vec::new(),
        }
    }

    #[test]
    fn empty_index_has_no_materials() {
        let index = WorkshopIndex::empty();

        assert!(index.materials().is_empty());
        assert!(index.find_in_category("workshops", "deploy").is_none());
    }

    #[test]
    fn find_in_category_matches_category_and_slug_together() {
        let index = WorkshopIndex::new(vec![
            material("workshops", "deploy", "Deploy"),
            material("presentations", "deploy", "Deploy talk"),
        ]);

        assert_eq!(
            index
                .find_in_category("workshops", "deploy")
                .map(|m| m.title.as_str()),
            Some("Deploy"),
        );
        assert_eq!(
            index
                .find_in_category("presentations", "deploy")
                .map(|m| m.title.as_str()),
            Some("Deploy talk"),
        );
        assert!(index.find_in_category("workshops", "missing").is_none());
    }
}
