//! The vendor marks that open a matter's collaboration-resource rows.
//!
//! **These are real third-party logos, which
//! [`crate::components::platform_mark`] deliberately refuses to draw.** The two
//! surfaces are not the same case, and the distinction is the reason this module
//! exists rather than reusing the idiom marks:
//!
//! - `platform_mark` draws the CLI download boxes on the **public marketing
//!   site**. Nothing there links to Apple or Microsoft; a vendor logo would be
//!   decoration, borrowing someone's identity to dress up the firm's own page.
//! - This module marks rows in the **authenticated matter workbench** that link
//!   to the actual Slack channel, Notion page, or Drive folder they name. The
//!   mark is doing identification work — it is how a reader tells six links
//!   apart at a glance — and it points at the genuine service.
//!
//! Referring to a service by its own mark, in order to name that service, is
//! the narrow use trademark law is most permissive about. The rule stands
//! unchanged for marketing: a logo still does not go on a public page to
//! decorate it.
//!
//! Each mark is drawn from its owner's published geometry, in its own brand
//! colors, so it is recognisable rather than approximated. The two multi-colour
//! marks (Slack, Drive) carry explicit fills and read correctly on either
//! theme; Notion's is monochrome and takes `currentColor`, so it inverts with
//! the page instead of vanishing into a dark background.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Which mark opens a resource row.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ResourceMark {
    /// Slack's four-colour pinwheel.
    #[default]
    Slack,
    /// Notion's monochrome page glyph.
    Notion,
    /// Google Drive's tri-colour triangle.
    GoogleDrive,
    /// Navigator's own portal glyph — a browser window. The client portal is a
    /// Navigator route, not a third-party service, so it carries the firm's own
    /// idiom mark in `currentColor` rather than a borrowed logo.
    Portal,
}

impl ResourceMark {
    /// Stable identifier, emitted as `data-resource-mark` so a test can assert
    /// which glyph a row opens on without matching path data.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Notion => "notion",
            Self::GoogleDrive => "google-drive",
            Self::Portal => "portal",
        }
    }
}

/// Draw one resource mark.
///
/// Hidden from assistive technology: the row's own link text already names the
/// service, so announcing the glyph would say "Slack" twice.
#[component]
pub fn ResourceMarkGlyph(mark: ResourceMark, #[props(default)] class: String) -> Element {
    match mark {
        ResourceMark::Slack => rsx! {
            svg {
                class: "{class}",
                "data-resource-mark": mark.name(),
                "viewBox": "0 0 122.8 122.8",
                "aria-hidden": "true",
                "focusable": "false",
                path {
                    fill: "#E01E5A",
                    d: "M25.8 77.6c0 7.1-5.8 12.9-12.9 12.9S0 84.7 0 77.6s5.8-12.9 12.9-12.9h12.9v12.9zm6.5 0c0-7.1 5.8-12.9 12.9-12.9s12.9 5.8 12.9 12.9v32.3c0 7.1-5.8 12.9-12.9 12.9s-12.9-5.8-12.9-12.9V77.6z",
                }
                path {
                    fill: "#36C5F0",
                    d: "M45.2 25.8c-7.1 0-12.9-5.8-12.9-12.9S38.1 0 45.2 0s12.9 5.8 12.9 12.9v12.9H45.2zm0 6.5c7.1 0 12.9 5.8 12.9 12.9s-5.8 12.9-12.9 12.9H12.9C5.8 58.1 0 52.3 0 45.2s5.8-12.9 12.9-12.9h32.3z",
                }
                path {
                    fill: "#2EB67D",
                    d: "M97 45.2c0-7.1 5.8-12.9 12.9-12.9s12.9 5.8 12.9 12.9-5.8 12.9-12.9 12.9H97V45.2zm-6.5 0c0 7.1-5.8 12.9-12.9 12.9s-12.9-5.8-12.9-12.9V12.9C64.7 5.8 70.5 0 77.6 0s12.9 5.8 12.9 12.9v32.3z",
                }
                path {
                    fill: "#ECB22E",
                    d: "M77.6 97c7.1 0 12.9 5.8 12.9 12.9s-5.8 12.9-12.9 12.9-12.9-5.8-12.9-12.9V97h12.9zm0-6.5c-7.1 0-12.9-5.8-12.9-12.9s5.8-12.9 12.9-12.9h32.3c7.1 0 12.9 5.8 12.9 12.9s-5.8 12.9-12.9 12.9H77.6z",
                }
            }
        },
        ResourceMark::Notion => rsx! {
            svg {
                class: "{class}",
                "data-resource-mark": mark.name(),
                "viewBox": "0 0 24 24",
                fill: "currentColor",
                "aria-hidden": "true",
                "focusable": "false",
                path {
                    d: "M4.459 4.208c.746.606 1.026.56 2.428.466l13.215-.793c.28 0 .047-.28-.046-.326L17.86 1.968c-.42-.326-.981-.7-2.055-.607L3.01 2.295c-.466.046-.56.28-.373.466zm.793 3.08v13.904c0 .747.373 1.027 1.214.98l14.523-.84c.841-.046.935-.56.935-1.167V6.354c0-.606-.233-.933-.748-.887l-15.177.887c-.56.047-.747.327-.747.933zm14.337.745c.093.42 0 .84-.42.888l-.7.14v10.264c-.608.327-1.168.514-1.635.514-.748 0-.935-.234-1.495-.933l-4.577-7.186v6.952L12.21 19s0 .84-1.168.84l-3.222.186c-.093-.186 0-.653.327-.746l.84-.233V9.854L7.822 9.76c-.094-.42.14-1.026.793-1.073l3.456-.233 4.764 7.279v-6.44l-1.215-.139c-.093-.514.28-.887.747-.98zM1.936 1.035l13.31-.98c1.634-.14 2.055-.047 3.082.7l4.249 2.986c.7.513.934.653.934 1.213v16.378c0 1.026-.373 1.634-1.68 1.726l-15.458.933c-.98.047-1.448-.093-1.962-.746l-3.129-4.06c-.56-.747-.793-1.306-.793-1.96V2.667c0-.839.374-1.54 1.447-1.632z",
                }
            }
        },
        ResourceMark::GoogleDrive => rsx! {
            svg {
                class: "{class}",
                "data-resource-mark": mark.name(),
                "viewBox": "0 0 87.3 78",
                "aria-hidden": "true",
                "focusable": "false",
                path {
                    fill: "#0066da",
                    d: "m6.6 66.85 3.85 6.65c.8 1.4 1.95 2.5 3.3 3.3l13.75-23.8h-27.5c0 1.55.4 3.1 1.2 4.5z",
                }
                path {
                    fill: "#00ac47",
                    d: "m43.65 25-13.75-23.8c-1.35.8-2.5 1.9-3.3 3.3l-25.4 44a9.06 9.06 0 0 0 -1.2 4.5h27.5z",
                }
                path {
                    fill: "#ea4335",
                    d: "m73.55 76.8c1.35-.8 2.5-1.9 3.3-3.3l1.6-2.75 7.65-13.25c.8-1.4 1.2-2.95 1.2-4.5h-27.502l5.852 11.5z",
                }
                path {
                    fill: "#00832d",
                    d: "m43.65 25 13.75-23.8c-1.35-.8-2.9-1.2-4.5-1.2h-18.5c-1.6 0-3.15.45-4.5 1.2z",
                }
                path {
                    fill: "#2684fc",
                    d: "m59.8 53h-32.3l-13.75 23.8c1.35.8 2.9 1.2 4.5 1.2h50.8c1.6 0 3.15-.45 4.5-1.2z",
                }
                path {
                    fill: "#ffba00",
                    d: "m73.4 26.5-12.7-22c-.8-1.4-1.95-2.5-3.3-3.3l-13.75 23.8 16.15 28h27.45c0-1.55-.4-3.1-1.2-4.5z",
                }
            }
        },
        ResourceMark::Portal => rsx! {
            svg {
                class: "{class}",
                "data-resource-mark": mark.name(),
                "viewBox": "0 0 24 24",
                fill: "none",
                stroke: "currentColor",
                "stroke-width": "1.5",
                "stroke-linecap": "round",
                "stroke-linejoin": "round",
                "aria-hidden": "true",
                "focusable": "false",
                path { d: "M4 4.5h16a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18V6A1.5 1.5 0 0 1 4 4.5Z" }
                path { d: "M2.5 9h19" }
                path { d: "M5.75 6.75h.01" }
                path { d: "M8.25 6.75h.01" }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceMark, ResourceMarkGlyph};
    use dioxus::prelude::*;

    fn render(mark: ResourceMark) -> String {
        dioxus_ssr::render_element(rsx! {
            ResourceMarkGlyph { mark, class: "resource-row__mark".to_string() }
        })
    }

    /// Every mark round-trips its stable name, which is what a test asserts on
    /// rather than path data.
    #[test]
    fn every_mark_names_itself() {
        for (mark, name) in [
            (ResourceMark::Slack, "slack"),
            (ResourceMark::Notion, "notion"),
            (ResourceMark::GoogleDrive, "google-drive"),
            (ResourceMark::Portal, "portal"),
        ] {
            assert_eq!(mark.name(), name);
        }
    }

    /// The two multi-colour vendor marks carry explicit brand fills, because a
    /// `currentColor` pinwheel would render as one flat blob.
    #[test]
    fn the_vendor_marks_carry_their_own_colours() {
        let slack = render(ResourceMark::Slack);
        for brand_colour in ["#E01E5A", "#36C5F0", "#2EB67D", "#ECB22E"] {
            assert!(slack.contains(brand_colour), "Slack missing {brand_colour}");
        }
        let drive = render(ResourceMark::GoogleDrive);
        for brand_colour in ["#0066da", "#00ac47", "#ea4335", "#ffba00"] {
            assert!(drive.contains(brand_colour), "Drive missing {brand_colour}");
        }
    }

    /// Notion's mark is monochrome, so it must follow the text colour or it
    /// disappears against a dark background.
    #[test]
    fn the_monochrome_marks_follow_the_text_colour() {
        for mark in [ResourceMark::Notion, ResourceMark::Portal] {
            let html = render(mark);
            assert!(
                html.contains("currentColor"),
                "{} should inherit the text colour: {html}",
                mark.name()
            );
        }
    }

    /// The glyph is decorative — the row's link text names the service — so it
    /// is hidden from assistive technology rather than announced twice.
    #[test]
    fn a_mark_is_hidden_from_assistive_technology() {
        let html = render(ResourceMark::Slack);
        assert!(html.contains(r#"aria-hidden="true""#), "{html}");
    }
}
