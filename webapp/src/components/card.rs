//! The shared card, as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::Card` builder. Cards are the
//! firm's most-repeated container — dashboard counts, the sign-in form, the
//! design gallery — so they share one component here instead of each site
//! hand-rolling the markup. Styling comes from the Dioxus Components theme
//! (`theme.css`, the `.nav-card*` rules), not Bootstrap: `highlighted` paints
//! the border and header band in the brand cyan, the same anchor treatment the
//! pricing card uses, so a "this one" card reads consistently everywhere.

use dioxus::prelude::*;

/// A card composed from a body (`children`) plus optional header / footer.
///
/// `highlighted` is the cyan anchor treatment (brand border, and a cyan header
/// band when a header is set). `center_body` centers the body content (the
/// dashboard-count look). The chrome is theme CSS; call sites stay declarative.
#[component]
pub fn Card(
    children: Element,
    #[props(default)] header: Option<Element>,
    #[props(default)] footer: Option<Element>,
    #[props(default)] highlighted: bool,
    #[props(default)] center_body: bool,
) -> Element {
    let card_class = if highlighted {
        "nav-card nav-card--highlighted"
    } else {
        "nav-card"
    };
    let body_class = if center_body {
        "nav-card__body nav-card__body--center"
    } else {
        "nav-card__body"
    };
    rsx! {
        div { class: "{card_class}",
            if let Some(header) = header {
                div { class: "nav-card__header", {header} }
            }
            div { class: "{body_class}", {children} }
            if let Some(footer) = footer {
                div { class: "nav-card__footer", {footer} }
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

    #[test]
    fn plain_card_wraps_body_in_theme_classes() {
        fn app() -> Element {
            rsx! { Card { p { "Hello" } } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"class="nav-card""#), "{html}");
        assert!(html.contains(r#"class="nav-card__body""#), "{html}");
        assert!(html.contains("Hello"), "{html}");
    }

    #[test]
    fn highlighted_card_paints_border_and_header_band() {
        fn app() -> Element {
            rsx! {
                Card {
                    highlighted: true,
                    header: rsx! { "Recommended" },
                    "body"
                }
            }
        }
        let html = ssr(app);
        assert!(html.contains("nav-card nav-card--highlighted"), "{html}");
        assert!(html.contains("nav-card__header"), "{html}");
        assert!(html.contains("Recommended"), "{html}");
    }

    #[test]
    fn footer_and_centered_body_render() {
        fn app() -> Element {
            rsx! {
                Card {
                    center_body: true,
                    footer: rsx! { a { href: "/x", "More" } },
                    "body"
                }
            }
        }
        let html = ssr(app);
        assert!(
            html.contains("nav-card__body nav-card__body--center"),
            "{html}"
        );
        assert!(html.contains(r#"class="nav-card__footer""#), "{html}");
        assert!(html.contains("More"), "{html}");
    }
}
