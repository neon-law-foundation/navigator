//! The shared "blueprints, not legal advice" disclaimer, as a Dioxus component
//! (issue #641, Phase 2).
//!
//! The successor to the `views::components::disclaimer`. A single UPL
//! guardrail partial rendered on every public template gallery page and the LSP
//! showcase. The copy is legal-council-reviewed and must not drift: it never
//! claims coverage of any particular state (the per-template badge carries the
//! jurisdiction), says a template is a starting point, and says downloading one
//! forms no attorney–client relationship. Ported verbatim from the source.

use dioxus::prelude::*;

/// The reusable disclaimer block. Plain language, not a fine-print wall.
#[component]
pub fn LegalBlueprintDisclaimer() -> Element {
    rsx! {
        aside { class: "nav-alert nav-alert--warning template-disclaimer", role: "note",
            h2 { class: "nav-alert__title", "These are blueprints, not legal advice" }
            p { class: "nav-alert__body",
                "Every document here is a plain-markdown "
                em { "template" }
                " — a starting point, not legal advice. Downloading one \
                 does not create an attorney–client relationship, and no \
                 lawyer has reviewed your situation. Each template is \
                 written for a specific jurisdiction — check the \
                 jurisdiction label before you rely on it. To have a \
                 licensed attorney prepare and stand behind a document, \
                 start a matter with the firm."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    fn html() -> String {
        fn app() -> Element {
            rsx! { LegalBlueprintDisclaimer {} }
        }
        ssr(app)
    }

    #[test]
    fn names_the_three_load_bearing_points() {
        let out = html();
        assert!(out.contains("not legal advice"));
        assert!(out.contains("does not create an attorney"));
        assert!(out.contains("specific jurisdiction"));
    }

    #[test]
    fn claims_no_state_coverage() {
        // UPL-safe: never assert coverage of a particular state.
        let out = html();
        for state in ["Oregon", "Nevada", "California", "Washington"] {
            assert!(!out.contains(state), "disclaimer must not name {state}");
        }
    }
}
