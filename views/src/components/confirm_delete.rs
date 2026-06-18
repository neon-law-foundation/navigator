//! Compact `<form>` that POSTs a delete and prompts for confirmation
//! through the browser's built-in `confirm()` dialog. Used inline in
//! admin list rows so the destructive action stays a real HTML form
//! rather than relying on JS to issue an XHR.
//!
//! The admin convention is `POST /…/:id/delete` — no `_method=DELETE`
//! override; the verb stays POST. A CSRF token is required because
//! every `/portal/*` POST is gated on it.

use maud::{html, Markup};

/// Builder for the inline delete form.
#[derive(Debug)]
pub struct ConfirmDeleteForm<'a> {
    action: &'a str,
    csrf_token: &'a str,
    label: &'a str,
    confirm_message: &'a str,
}

impl<'a> ConfirmDeleteForm<'a> {
    /// New form posting to `action` with `csrf_token` threaded as a
    /// hidden `_csrf` input. Pass an empty token only in tests; the
    /// CSRF middleware will reject empty tokens in production.
    #[must_use]
    pub const fn new(action: &'a str, csrf_token: &'a str) -> Self {
        Self {
            action,
            csrf_token,
            label: "Delete",
            confirm_message: "Are you sure? This cannot be undone.",
        }
    }

    #[must_use]
    pub const fn with_label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }

    /// Override the `confirm()` prompt. Pass `""` to opt out of the
    /// prompt entirely (use that on dedicated confirmation pages
    /// where the prompt is already in the page chrome).
    #[must_use]
    pub const fn with_confirm_message(mut self, message: &'a str) -> Self {
        self.confirm_message = message;
        self
    }

    #[must_use]
    pub fn render(&self) -> Markup {
        let escaped_prompt = escape_for_attribute(self.confirm_message);
        let onsubmit = if self.confirm_message.is_empty() {
            None
        } else {
            Some(format!("return confirm('{escaped_prompt}')"))
        };
        // `d-inline` keeps the form participating in the surrounding
        // flex/btn-group flow when it's nested inside one. The button
        // wears `btn btn-danger` so the destructive token is the
        // dominant signal — text-and-color, no glyph.
        html! {
            form method="post"
                 action=(self.action)
                 class="d-inline confirm-delete"
                 onsubmit=[onsubmit.as_deref()]
            {
                @if !self.csrf_token.is_empty() {
                    input type="hidden" name="_csrf" value=(self.csrf_token);
                }
                button type="submit" class="btn btn-danger" { (self.label) }
            }
        }
    }
}

/// Escape the prompt for safe embedding inside the
/// `onsubmit="return confirm('…')"` attribute. Single quotes flip to
/// `&apos;` so an apostrophe in copy ("can't") doesn't close the
/// attribute early; ampersands and angle brackets are passed through
/// maud's standard attribute escaping at render time.
fn escape_for_attribute(message: &str) -> String {
    message.replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::ConfirmDeleteForm;

    #[test]
    fn renders_post_form_pointing_at_action() {
        let html = ConfirmDeleteForm::new("/portal/admin/people/1/delete", "TOKEN")
            .render()
            .into_string();
        assert!(html.contains("method=\"post\""));
        assert!(html.contains("action=\"/portal/admin/people/1/delete\""));
    }

    #[test]
    fn includes_csrf_hidden_input_when_token_present() {
        let html = ConfirmDeleteForm::new("/portal/admin/x/1/delete", "abc")
            .render()
            .into_string();
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"abc\""));
    }

    #[test]
    fn omits_csrf_hidden_input_when_token_empty() {
        let html = ConfirmDeleteForm::new("/portal/admin/x/1/delete", "")
            .render()
            .into_string();
        assert!(!html.contains("name=\"_csrf\""));
    }

    #[test]
    fn default_button_label_is_delete() {
        let html = ConfirmDeleteForm::new("/x", "t").render().into_string();
        assert!(html.contains(">Delete</button>"));
    }

    #[test]
    fn custom_label_replaces_button_text() {
        let html = ConfirmDeleteForm::new("/x", "t")
            .with_label("Remove")
            .render()
            .into_string();
        assert!(html.contains(">Remove</button>"));
        assert!(!html.contains(">Delete</button>"));
    }

    #[test]
    fn default_confirm_prompt_is_wired_through_onsubmit() {
        let html = ConfirmDeleteForm::new("/x", "t").render().into_string();
        assert!(
            html.contains("onsubmit=\"return confirm("),
            "expected onsubmit confirm wiring: {html}",
        );
        assert!(html.contains("Are you sure?"));
    }

    #[test]
    fn apostrophe_in_prompt_is_escaped_for_the_attribute() {
        let html = ConfirmDeleteForm::new("/x", "t")
            .with_confirm_message("can't undo")
            .render()
            .into_string();
        assert!(
            html.contains("can&apos;t undo") || html.contains("can&amp;apos;t undo"),
            "apostrophe must be entity-escaped, got: {html}",
        );
    }

    #[test]
    fn empty_confirm_message_omits_onsubmit() {
        let html = ConfirmDeleteForm::new("/x", "t")
            .with_confirm_message("")
            .render()
            .into_string();
        assert!(!html.contains("onsubmit"));
    }

    #[test]
    fn button_wears_bootstrap_btn_danger() {
        // Destructive token = `btn btn-danger`. Replaces Pico's
        // `class="contrast"` so the destructive color reads in the
        // Bootstrap palette without bespoke CSS.
        let html = ConfirmDeleteForm::new("/x", "t").render().into_string();
        assert!(
            html.contains("class=\"btn btn-danger\""),
            "delete button missing btn-danger, got: {html}",
        );
        assert!(
            !html.contains("class=\"contrast\""),
            "Pico contrast class should be gone, got: {html}",
        );
    }

    #[test]
    fn form_wears_d_inline_so_it_flows_inside_btn_group() {
        let html = ConfirmDeleteForm::new("/x", "t").render().into_string();
        assert!(
            html.contains("d-inline confirm-delete"),
            "form should be d-inline to flow next to neighboring buttons, got: {html}",
        );
    }
}
