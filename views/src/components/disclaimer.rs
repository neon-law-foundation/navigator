#![allow(clippy::doc_markdown)]
//! Shared "blueprints, not legal advice" disclaimer.
//!
//! A single UPL guardrail partial rendered on every public template
//! gallery page and on the LSP showcase. The firm is admitted in a
//! handful of states and these surfaces are reachable from anywhere, so
//! the copy never claims coverage: it says plainly that a template is a
//! starting point, that downloading one forms no attorney–client
//! relationship, and that each template is written for a specific
//! jurisdiction (the per-template badge carries the specifics). Build it
//! once here so the language can't drift between the two surfaces.

use maud::{html, Markup};

/// The reusable disclaimer block. Plain language, not a fine-print wall.
#[must_use]
pub fn legal_blueprint_disclaimer() -> Markup {
    html! {
        aside.alert."alert-warning"."template-disclaimer" role="note" {
            h2.h6 { "These are blueprints, not legal advice" }
            p.mb-0 {
                "Every document here is a plain-markdown "
                em { "template" }
                " — a starting point, not legal advice. Downloading one "
                "does not create an attorney–client relationship, and no "
                "lawyer has reviewed your situation. Each template is "
                "written for a specific jurisdiction — check the "
                "jurisdiction label before you rely on it. To have a "
                "licensed attorney prepare and stand behind a document, "
                "contact the firm."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::legal_blueprint_disclaimer;

    #[test]
    fn names_the_three_load_bearing_points() {
        let html = legal_blueprint_disclaimer().into_string();
        // Not legal advice.
        assert!(html.contains("not legal advice"));
        // No attorney-client relationship formed by download.
        assert!(html.contains("does not create an attorney"));
        // Jurisdiction-specific.
        assert!(html.contains("specific jurisdiction"));
    }

    #[test]
    fn claims_no_state_coverage() {
        // Fork-safe + UPL-safe: the partial must not assert the firm
        // covers any particular state (that would imply representation
        // and hard-code a per-deployment fact). The per-template badge
        // carries jurisdiction; the disclaimer stays neutral.
        let html = legal_blueprint_disclaimer().into_string();
        for state in ["Oregon", "Nevada", "California", "Washington"] {
            assert!(
                !html.contains(state),
                "disclaimer must not name {state} — it would imply coverage"
            );
        }
    }
}
