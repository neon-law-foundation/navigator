//! Conference talks served under `/foundation/presentations/:slug`,
//! reusing the workshop stepped-content flow (overview + one URL per
//! `##` section) so a slide deck behaves like the workshop runbook:
//! progress rail, jump-to-step dropdown, copy-as-markdown.
//!
//! Unlike workshops — which load a manifest of files from a content
//! directory at boot — a talk is a single canonical document baked
//! into the binary with `include_str!`. That keeps the talk out of `AppState`
//! entirely (the engineer council's call on 2026-05-29: no new state
//! field, no filesystem dependency in tests) while still rendering
//! through the shared [`crate::workshops::WorkshopMaterial`] model.

use std::sync::{Arc, LazyLock};

use crate::workshops::{loader, WorkshopMaterial};

/// The "Rust in Peace" talk, baked at compile time. Path is resolved
/// from the crate manifest dir so it is robust to where the binary
/// runs from.
const RUST_IN_PEACE_MD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/content/presentations/RUST_IN_PEACE.md"
));

const RUST_IN_PEACE_DESCRIPTION: &str =
    "A Neon Law Foundation talk for Rust NYC on how we use Rust to improve access to justice: \
     deterministic workflows from law — statute to Cucumber feature to template to notation — \
     dissected one modular, attorney-gated step at a time, with every code slide an exact copy \
     of the shipped repository kept honest by a grounding test.";

/// In-memory set of baked talks. Mirrors `WorkshopIndex` but is its
/// own type so it can be a distinct router-state / lookup surface.
#[derive(Debug, Clone)]
pub struct PresentationIndex {
    talks: Arc<Vec<WorkshopMaterial>>,
}

impl PresentationIndex {
    #[must_use]
    pub fn new(talks: Vec<WorkshopMaterial>) -> Self {
        Self {
            talks: Arc::new(talks),
        }
    }

    #[must_use]
    pub fn find(&self, slug: &str) -> Option<&WorkshopMaterial> {
        self.talks.iter().find(|m| m.slug == slug)
    }

    /// Every baked talk, for the `/foundation/presentations` hub.
    #[must_use]
    pub fn talks(&self) -> &[WorkshopMaterial] {
        &self.talks
    }
}

/// The baked talks, parsed once on first access.
pub static PRESENTATIONS: LazyLock<PresentationIndex> = LazyLock::new(|| {
    PresentationIndex::new(vec![loader::material_from_markdown(
        "rust-in-peace",
        "Rust in Peace",
        RUST_IN_PEACE_DESCRIPTION,
        RUST_IN_PEACE_MD,
    )])
});

#[cfg(test)]
mod tests {
    use super::PRESENTATIONS;

    #[test]
    fn rust_in_peace_is_registered_with_steps() {
        let talk = PRESENTATIONS
            .find("rust-in-peace")
            .expect("rust-in-peace talk must be baked in");
        assert_eq!(talk.title, "Rust in Peace");
        // The agenda + every beat of the talk becomes its own step.
        assert!(
            talk.sections.len() >= 6,
            "expected the talk to split into its agenda + beats, got {}",
            talk.sections.len()
        );
        assert_eq!(talk.sections[0].title, "Agenda");
        // The chrome owns the sole <h1>; the body must not repeat it.
        assert!(!talk.body_html.contains("<h1>"));
    }

    #[test]
    fn unknown_slug_is_none() {
        assert!(PRESENTATIONS.find("no-such-talk").is_none());
    }

    /// Every code slide in the talk is an **exact copy** of the workspace
    /// file it cites. The convention is the visible attribution line —
    /// ``From `path/to/file`:`` — followed by a fenced block; this test
    /// walks the raw markdown, reads each cited file from the workspace
    /// (not a second baked copy, which would always pass), and fails the
    /// build when a snippet drifts from its source. The floor assertion
    /// keeps the convention itself from silently vanishing.
    #[test]
    fn talk_snippets_are_exact_copies_of_cited_sources() {
        let workspace_root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        let lines: Vec<&str> = super::RUST_IN_PEACE_MD.lines().collect();
        let mut grounded = 0;
        let mut i = 0;
        while i < lines.len() {
            if let Some(path) = lines[i]
                .strip_prefix("From `")
                .and_then(|rest| rest.strip_suffix("`:"))
            {
                let mut open = i + 1;
                while open < lines.len() && !lines[open].starts_with("```") {
                    open += 1;
                }
                assert!(
                    open < lines.len(),
                    "attribution for {path} has no code fence after it"
                );
                let mut close = open + 1;
                while close < lines.len() && lines[close] != "```" {
                    close += 1;
                }
                assert!(close < lines.len(), "code fence for {path} is never closed");
                let snippet = lines[open + 1..close].join("\n");
                let source = std::fs::read_to_string(format!("{workspace_root}/{path}"))
                    .unwrap_or_else(|e| panic!("cited source {path} is unreadable: {e}"));
                assert!(
                    source.contains(&snippet),
                    "slide snippet drifted from {path} — update the talk to match the source"
                );
                grounded += 1;
                i = close;
            }
            i += 1;
        }
        assert!(
            grounded >= 6,
            "expected at least 6 grounded snippets in the talk, found {grounded}"
        );
    }
}
