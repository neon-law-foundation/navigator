//! Content-freshness footer, as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::freshness`. Renders the
//! git-derived edit date as `Last edited in main MMM D, YYYY`, or nothing when
//! the date is absent (the distroless prod image has no git history, so the
//! line is silently dropped there). The date is formatted server-side and
//! passed in as a string, so this component stays free of a date library in the
//! wasm client — the same render-side-subset discipline the sort state uses.

use dioxus::prelude::*;

/// The last-edited footer. `last_edited` is the pre-formatted date (e.g.
/// `"May 22, 2026"`); `None` renders nothing so callers can splice it
/// unconditionally.
#[component]
pub fn Freshness(last_edited: Option<String>) -> Element {
    let Some(date) = last_edited else {
        return rsx! {};
    };
    rsx! {
        p { class: "nav-freshness",
            small { "Last edited in main {date}" }
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

    #[test]
    fn empty_when_absent() {
        fn app() -> Element {
            rsx! { Freshness { last_edited: None } }
        }
        assert!(!ssr(app).contains("nav-freshness"));
    }

    #[test]
    fn renders_the_last_edited_line() {
        fn app() -> Element {
            rsx! { Freshness { last_edited: Some("May 22, 2026".to_string()) } }
        }
        let html = ssr(app);
        assert!(html.contains("nav-freshness"), "{html}");
        assert!(html.contains("Last edited in main May 22, 2026"), "{html}");
    }
}
