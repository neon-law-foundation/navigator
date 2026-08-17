//! The shared toast, as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::Toast`. A server-rendered
//! toast is visible on load (`role="alert"`, `aria-live="assertive"`) with no JS
//! init call — the Bootstrap `data-bs-dismiss` wiring is gone with Bootstrap;
//! interactive dismissal returns as a Dioxus signal when a page needs it. Toned
//! by the theme: `Primary` is the brand cyan, so a neutral notice picks up the
//! firm color for free.

use dioxus::prelude::*;

/// The color of a toast. Selects the theme's `.nav-toast--*` accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastTone {
    /// Red — errors and "you must sign in" gates.
    Danger,
    /// Green — confirmations.
    Success,
    /// Brand cyan — neutral notices.
    Primary,
    /// Amber — non-blocking warnings.
    Warning,
}

impl ToastTone {
    /// The theme modifier class for this tone.
    fn modifier(self) -> &'static str {
        match self {
            ToastTone::Danger => "nav-toast--danger",
            ToastTone::Success => "nav-toast--success",
            ToastTone::Primary => "nav-toast--primary",
            ToastTone::Warning => "nav-toast--warning",
        }
    }
}

/// A toast carrying a single message, toned by [`ToastTone`].
#[component]
pub fn Toast(message: String, tone: ToastTone) -> Element {
    let class = format!("nav-toast {}", tone.modifier());
    rsx! {
        div {
            class: "{class}",
            role: "alert",
            "aria-live": "assertive",
            "aria-atomic": "true",
            div { class: "nav-toast__body", "{message}" }
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
    fn primary_toast_uses_the_brand_cyan_modifier() {
        fn app() -> Element {
            rsx! { Toast { message: "Saved".to_string(), tone: ToastTone::Primary } }
        }
        let html = ssr(app);
        assert!(html.contains("nav-toast nav-toast--primary"), "{html}");
        assert!(html.contains("Saved"), "{html}");
    }

    #[test]
    fn toast_is_an_assertive_live_region() {
        fn app() -> Element {
            rsx! { Toast { message: "Sign in to continue".to_string(), tone: ToastTone::Danger } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"role="alert""#), "{html}");
        assert!(html.contains(r#"aria-live="assertive""#), "{html}");
        assert!(html.contains("nav-toast--danger"), "{html}");
        assert!(html.contains("Sign in to continue"), "{html}");
    }
}
