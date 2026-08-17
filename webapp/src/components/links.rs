//! Off-site link, as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::ExternalLink`. An anchor that
//! leaves our domains opens in a new tab and carries the OWASP `rel` pair
//! (`noopener noreferrer`), with a decorative upper-right arrow (the
//! [`IconName::BoxArrowUpRight`] inline SVG) so the reader knows it goes
//! off-site. The anchor text is the accessible label, so the glyph is
//! `aria-hidden`.

use dioxus::prelude::*;

use crate::components::{Icon, IconName};

/// An off-site anchor around `children`. `class` sets the `<a>` class (e.g.
/// `link-secondary` for muted footer links); `title` sets a hover tooltip.
#[component]
pub fn ExternalLink(
    href: String,
    #[props(default)] class: Option<String>,
    #[props(default)] title: Option<String>,
    children: Element,
) -> Element {
    rsx! {
        a {
            href: "{href}",
            class: class,
            title: title,
            target: "_blank",
            rel: "noopener noreferrer",
            {children}
            " "
            Icon { name: IconName::BoxArrowUpRight }
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
    fn opens_new_tab_with_owasp_rel_and_offsite_glyph() {
        fn app() -> Element {
            rsx! {
                ExternalLink { href: "https://example.com".to_string(), "Example" }
            }
        }
        let html = ssr(app);
        assert!(html.contains(r#"href="https://example.com""#), "{html}");
        assert!(html.contains(r#"target="_blank""#), "{html}");
        assert!(html.contains(r#"rel="noopener noreferrer""#), "{html}");
        assert!(html.contains("Example"), "{html}");
        // The off-site arrow is an inline SVG, decorative.
        assert!(html.contains("nav-icon"), "{html}");
    }

    #[test]
    fn carries_an_optional_class_and_title() {
        fn app() -> Element {
            rsx! {
                ExternalLink {
                    href: "https://example.com".to_string(),
                    class: "link-secondary".to_string(),
                    title: "Opens example.com".to_string(),
                    "Docs"
                }
            }
        }
        let html = ssr(app);
        assert!(html.contains(r#"class="link-secondary""#), "{html}");
        assert!(html.contains(r#"title="Opens example.com""#), "{html}");
    }
}
