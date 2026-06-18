//! A reusable Bootstrap toast.
//!
//! The first toast in the app was the red "you need to sign in" banner on
//! the login page; this lifts that one-off into a shared builder so any
//! surface can raise a toast in a consistent shape. Rendered with the
//! static `.show` class so a server-rendered toast is visible on load
//! without a JS init call; the close button is wired to the vendored
//! `bootstrap.bundle.min.js` via `data-bs-dismiss="toast"`.
//!
//! Toned with Bootstrap's `text-bg-*` helpers — `Primary` is the brand
//! cyan, so a neutral notice picks up the firm color for free.

use maud::{html, Markup};

/// The color of a toast. Maps to a Bootstrap `text-bg-*` helper.
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
    fn bg_class(self) -> &'static str {
        match self {
            ToastTone::Danger => "text-bg-danger",
            ToastTone::Success => "text-bg-success",
            ToastTone::Primary => "text-bg-primary",
            ToastTone::Warning => "text-bg-warning",
        }
    }

    /// The dark-background tones need the white close glyph; amber is light
    /// enough to keep the default dark glyph.
    fn close_is_white(self) -> bool {
        !matches!(self, ToastTone::Warning)
    }
}

/// A dismissible Bootstrap toast carrying a single message.
pub struct Toast {
    message: String,
    tone: ToastTone,
}

impl Toast {
    /// A toast with an explicit [`ToastTone`].
    #[must_use]
    pub fn new(message: impl Into<String>, tone: ToastTone) -> Self {
        Self {
            message: message.into(),
            tone,
        }
    }

    /// A red error toast.
    #[must_use]
    pub fn danger(message: impl Into<String>) -> Self {
        Self::new(message, ToastTone::Danger)
    }

    /// A green confirmation toast.
    #[must_use]
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(message, ToastTone::Success)
    }

    /// A neutral cyan toast.
    #[must_use]
    pub fn primary(message: impl Into<String>) -> Self {
        Self::new(message, ToastTone::Primary)
    }

    /// An amber warning toast.
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, ToastTone::Warning)
    }

    #[must_use]
    pub fn render(&self) -> Markup {
        let toast_class = format!(
            "toast show align-items-center {} border-0",
            self.tone.bg_class()
        );
        let close_class = if self.tone.close_is_white() {
            "btn-close btn-close-white me-2 m-auto"
        } else {
            "btn-close me-2 m-auto"
        };
        html! {
            div class=(toast_class) role="alert" aria-live="assertive" aria-atomic="true" {
                div."d-flex" {
                    div."toast-body" { (self.message) }
                    button class=(close_class) type="button"
                        data-bs-dismiss="toast" aria-label="Close" {}
                }
            }
        }
    }
}

/// Wrap one or more rendered toasts in the fixed top-right overlay
/// container — the placement the login gate uses so the notice floats over
/// the page on arrival.
#[must_use]
pub fn toast_overlay(toasts: &Markup) -> Markup {
    html! {
        div."toast-container"."position-fixed"."top-0"."end-0"."p-3" { (toasts) }
    }
}

#[cfg(test)]
mod tests {
    use super::{toast_overlay, Toast, ToastTone};

    #[test]
    fn danger_toast_is_red_and_dismissible() {
        let out = Toast::danger("Sign in to continue").render().into_string();
        assert!(out.contains("class=\"toast show align-items-center text-bg-danger border-0\""));
        assert!(out.contains("toast-body"));
        assert!(out.contains("Sign in to continue"));
        assert!(out.contains("data-bs-dismiss=\"toast\""));
        assert!(out.contains("btn-close-white"));
    }

    #[test]
    fn primary_toast_uses_the_brand_cyan_helper() {
        let out = Toast::primary("Saved").render().into_string();
        assert!(out.contains("text-bg-primary"));
    }

    #[test]
    fn warning_toast_keeps_the_default_dark_close_glyph() {
        let out = Toast::new("Heads up", ToastTone::Warning)
            .render()
            .into_string();
        assert!(out.contains("text-bg-warning"));
        assert!(!out.contains("btn-close-white"));
    }

    #[test]
    fn overlay_pins_toasts_to_the_top_right() {
        let out = toast_overlay(&Toast::success("Done").render()).into_string();
        assert!(out.contains("toast-container position-fixed top-0 end-0 p-3"));
        assert!(out.contains("text-bg-success"));
    }
}
