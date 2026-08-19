//! The line marks that open a CLI download box — one per operating system.
//!
//! Drawn to the same specification as [`crate::components::practice_card`]'s
//! marks (a 24-unit square, stroked in `currentColor` at 1.5, round joins), so
//! a download box and a practice box read as the same object wearing a
//! different glyph. `home.css`'s `.home-practice__mark` sizes and centres both.
//!
//! **None of the three is a vendor logo, and that is deliberate.** The Apple
//! wordmark, the Apple silhouette, the Microsoft four-square, and Tux are each
//! someone else's trademark; a law firm's own site is the last place to draw one
//! for decoration. Each mark is the platform's *idiom* instead — a terminal for
//! Linux, a laptop for macOS, a four-pane window for Windows — which names the
//! platform to a reader without borrowing anyone's identity.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// Which platform mark opens a download box.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlatformMark {
    /// A terminal window with a prompt, for Linux.
    #[default]
    Terminal,
    /// A laptop, for macOS.
    Laptop,
    /// A four-pane window, for Windows.
    Window,
}

impl PlatformMark {
    /// Stable identifier for the mark, emitted as `data-platform-mark` so a
    /// test can assert which glyph a box opens on without matching path data.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Laptop => "laptop",
            Self::Window => "window",
        }
    }
}

/// Draw one platform mark.
///
/// Hidden from assistive technology: the heading beside it already names the
/// platform, so announcing the glyph would say "Linux" twice.
#[component]
pub(crate) fn PlatformMarkGlyph(mark: PlatformMark, #[props(default)] class: String) -> Element {
    rsx! {
        svg {
            class: "{class}",
            "data-platform-mark": mark.name(),
            "viewBox": "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            "stroke-width": "1.5",
            "stroke-linecap": "round",
            "stroke-linejoin": "round",
            "aria-hidden": "true",
            "focusable": "false",
            match mark {
                PlatformMark::Terminal => rsx! {
                    path { d: "M4 4.5h16a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18V6A1.5 1.5 0 0 1 4 4.5Z" }
                    path { d: "M2.5 8.5h19" }
                    path { d: "m6.5 12 2.25 2.25L6.5 16.5" }
                    path { d: "M11.75 16.5h5.75" }
                },
                PlatformMark::Laptop => rsx! {
                    path { d: "M4.5 5h15v10.5h-15z" }
                    path { d: "M2.5 18.5h19" }
                    path { d: "M6 15.5 4.5 18.5" }
                    path { d: "m18 15.5 1.5 3" }
                },
                PlatformMark::Window => rsx! {
                    path { d: "M4 4.5h16a1.5 1.5 0 0 1 1.5 1.5v12a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18V6A1.5 1.5 0 0 1 4 4.5Z" }
                    path { d: "M12 4.5v15" }
                    path { d: "M2.5 12h19" }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(mark: PlatformMark) -> String {
        let mut dom = VirtualDom::new_with_props(
            PlatformMarkGlyph,
            PlatformMarkGlyphProps {
                mark,
                class: "home-practice__mark".to_string(),
            },
        );
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    /// Each mark names itself in the markup and inherits the box's colour, so
    /// one glyph serves the light and dark themes the way the practice marks do.
    #[test]
    fn every_mark_is_named_and_stroked_in_the_boxs_own_colour() {
        for (mark, name) in [
            (PlatformMark::Terminal, "terminal"),
            (PlatformMark::Laptop, "laptop"),
            (PlatformMark::Window, "window"),
        ] {
            let out = html(mark);
            assert!(
                out.contains(&format!(r#"data-platform-mark="{name}""#)),
                "{name} names itself: {out}"
            );
            assert!(
                out.contains(r#"stroke="currentColor""#),
                "{name} inherits the box colour: {out}"
            );
            assert!(
                out.contains(r#"aria-hidden="true"#),
                "{name} is decorative: {out}"
            );
            assert!(
                out.contains("home-practice__mark"),
                "{name} takes the caller's class: {out}"
            );
        }
    }

    /// The window's four panes are its mullion and transom crossing the frame.
    /// Pinned because a mark that loses a stroke degrades into the terminal
    /// frame beside it, and nothing else would notice.
    #[test]
    fn the_windows_mark_is_divided_into_four_panes() {
        let out = html(PlatformMark::Window);
        assert!(out.contains(r#"d="M12 4.5v15""#), "the mullion: {out}");
        assert!(out.contains(r#"d="M2.5 12h19""#), "the transom: {out}");
    }
}
