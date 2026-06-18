//! Compact "actions" cell for an admin list-row: an Edit link beside
//! an inline Delete form, both rendered as icon glyphs from the
//! vendored Bootstrap Icons font.
//!
//! Replaces the per-page hand-rolled `<a>Edit</a> + <form>Delete</form>`
//! pairs so every admin table gets the same pencil + trash chrome, the
//! same `confirm()` prompt wiring, the same CSRF threading, and the
//! same screen-reader labels. Callers compose one `RowActions` per row
//! and drop the rendered `Markup` into the table cell.
//!
//! Why a builder: an admin row eventually grows other affordances
//! (resend, archive). The builder keeps positional parameters stable
//! and leaves room for more verbs without re-flowing every call site.

use maud::{html, Markup};

/// Builder for the icon-row actions cell. Always-on edit + delete;
/// the trash form gets a `confirm()` prompt by default so misclicks
/// cost a keystroke.
#[derive(Debug)]
pub struct RowActions<'a> {
    csrf_token: &'a str,
    edit_href: Option<&'a str>,
    delete_action: Option<&'a str>,
    delete_confirm: &'a str,
    /// Human-readable identifier for the row (email / name / code) —
    /// stitched into the ARIA labels so a screen reader hears
    /// "Edit libra@example.com" rather than just "Edit." Empty means
    /// fall back to the bare verb.
    row_label: &'a str,
}

impl<'a> RowActions<'a> {
    /// New cell with no actions wired yet. `csrf_token` is required
    /// by the CSRF middleware on every `/portal/admin/*` POST — pass an
    /// empty string only in tests that bypass middleware.
    #[must_use]
    pub const fn new(csrf_token: &'a str) -> Self {
        Self {
            csrf_token,
            edit_href: None,
            delete_action: None,
            delete_confirm: "Are you sure? This cannot be undone.",
            row_label: "",
        }
    }

    /// Wire the pencil-icon link to `href`.
    #[must_use]
    pub const fn edit(mut self, href: &'a str) -> Self {
        self.edit_href = Some(href);
        self
    }

    /// Wire the trash-icon form to `action` with the default prompt.
    /// Combine with [`Self::with_delete_confirm`] to echo the row's
    /// identity in the dialog ("Delete person libra@example.com?").
    #[must_use]
    pub const fn delete(mut self, action: &'a str) -> Self {
        self.delete_action = Some(action);
        self
    }

    /// Override the `confirm()` text shown before the delete POST
    /// fires. Single quotes in `message` are entity-escaped because
    /// the prompt is rendered inside the `onsubmit` attribute.
    #[must_use]
    pub const fn with_delete_confirm(mut self, message: &'a str) -> Self {
        self.delete_confirm = message;
        self
    }

    /// Identity string echoed into ARIA labels so the verbs are
    /// disambiguated row-to-row.
    #[must_use]
    pub const fn with_row_label(mut self, label: &'a str) -> Self {
        self.row_label = label;
        self
    }

    #[must_use]
    pub fn render(&self) -> Markup {
        let edit_aria = if self.row_label.is_empty() {
            "Edit".to_string()
        } else {
            format!("Edit {}", self.row_label)
        };
        let delete_aria = if self.row_label.is_empty() {
            "Delete".to_string()
        } else {
            format!("Delete {}", self.row_label)
        };
        let escaped_prompt = escape_for_attribute(self.delete_confirm);
        let onsubmit = if self.delete_confirm.is_empty() {
            None
        } else {
            Some(format!("return confirm('{escaped_prompt}')"))
        };
        // `btn-group` keeps the two icons visually paired with no
        // gap between them; `btn-sm` shrinks the chrome to row scale.
        // `btn-outline-secondary` (edit) reads neutral; `btn-outline-danger`
        // (delete) inherits Bootstrap's danger token so the trash glyph
        // is red on hover/focus — the council's "destructive must be
        // unmistakable" finding lands automatically here.
        html! {
            div."btn-group"."btn-group-sm".row-actions role="group" aria-label="Row actions" {
                @if let Some(href) = self.edit_href {
                    a href=(href)
                      class="btn btn-outline-secondary row-action row-action-edit"
                      data-action="edit"
                      aria-label=(edit_aria)
                      title=(edit_aria) {
                        i.bi."bi-pencil-square" aria-hidden="true" {}
                    }
                }
                @if let Some(action) = self.delete_action {
                    // Progressive enhancement: with HTMX loaded the
                    // POST flies AJAX, swaps the parent <tr> with the
                    // (empty) response body, and the row vanishes
                    // without a page reload. Without HTMX the
                    // standard form submit + handler redirect still
                    // works. `hx-confirm` replaces the JS `onsubmit
                    // confirm()` so HTMX users get one prompt, not
                    // two; we keep `onsubmit` for the non-HTMX path.
                    form method="post"
                         action=(action)
                         class="row-action-form d-inline"
                         hx-post=(action)
                         hx-target="closest tr"
                         hx-swap="outerHTML swap:200ms"
                         hx-confirm=(self.delete_confirm)
                         onsubmit=[onsubmit.as_deref()] {
                        @if !self.csrf_token.is_empty() {
                            input type="hidden" name="_csrf" value=(self.csrf_token);
                        }
                        button type="submit"
                               class="btn btn-outline-danger row-action row-action-delete"
                               data-action="delete"
                               aria-label=(delete_aria)
                               title=(delete_aria) {
                            i.bi."bi-trash3-fill" aria-hidden="true" {}
                        }
                    }
                }
            }
        }
    }
}

/// Escape a string for safe embedding inside the `onsubmit="..."`
/// attribute. Single quotes flip to `&apos;` so an apostrophe in copy
/// ("can't") doesn't close the attribute early.
fn escape_for_attribute(message: &str) -> String {
    message.replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::RowActions;

    #[test]
    fn renders_pencil_edit_link_pointing_at_href() {
        let html = RowActions::new("TOK")
            .edit("/portal/admin/people/42/edit")
            .delete("/portal/admin/people/42/delete")
            .render()
            .into_string();
        assert!(
            html.contains("href=\"/portal/admin/people/42/edit\""),
            "edit link missing: {html}",
        );
        assert!(
            html.contains("class=\"bi bi-pencil-square\""),
            "pencil glyph missing: {html}",
        );
        assert!(
            html.contains("data-action=\"edit\""),
            "data-action edit hook missing: {html}",
        );
    }

    #[test]
    fn renders_filled_trash_glyph_for_delete() {
        let html = RowActions::new("TOK")
            .edit("/portal/admin/x/1/edit")
            .delete("/portal/admin/x/1/delete")
            .render()
            .into_string();
        assert!(
            html.contains("class=\"bi bi-trash3-fill\""),
            "filled trash glyph missing — the council picked the filled \
             variant so destructive is unmistakable; got: {html}",
        );
        assert!(
            html.contains("data-action=\"delete\""),
            "data-action delete hook missing: {html}",
        );
    }

    #[test]
    fn wraps_actions_in_a_bootstrap_btn_group() {
        // The two glyphs read as a paired control group via Bootstrap's
        // `.btn-group .btn-group-sm`. `role="group"` + `aria-label`
        // tell assistive tech this is one logical control set.
        let html = RowActions::new("TOK")
            .edit("/e")
            .delete("/d")
            .render()
            .into_string();
        assert!(
            html.contains("class=\"btn-group btn-group-sm row-actions\""),
            "wrapper missing btn-group classes, got: {html}",
        );
        assert!(html.contains("role=\"group\""));
        assert!(html.contains("aria-label=\"Row actions\""));
    }

    #[test]
    fn edit_button_uses_outline_secondary_and_delete_uses_outline_danger() {
        let html = RowActions::new("TOK")
            .edit("/e")
            .delete("/d")
            .render()
            .into_string();
        assert!(
            html.contains("btn btn-outline-secondary"),
            "edit anchor missing outline-secondary, got: {html}",
        );
        assert!(
            html.contains("btn btn-outline-danger"),
            "delete button missing outline-danger — destructive token \
             must be visible; got: {html}",
        );
    }

    #[test]
    fn delete_form_posts_to_action_with_confirm() {
        let html = RowActions::new("TOK")
            .delete("/portal/admin/people/42/delete")
            .render()
            .into_string();
        assert!(html.contains("method=\"post\""));
        assert!(html.contains("action=\"/portal/admin/people/42/delete\""));
        assert!(
            html.contains("onsubmit=\"return confirm("),
            "missing confirm wiring: {html}",
        );
        assert!(
            html.contains("Are you sure?"),
            "default prompt missing: {html}",
        );
    }

    #[test]
    fn delete_form_emits_htmx_attributes_for_partial_swap() {
        // With HTMX loaded, the POST should fly AJAX, swap the
        // parent <tr>, and the row vanishes without a page reload.
        // Without HTMX, the same form submits normally and the
        // handler redirect kicks in (progressive enhancement).
        let html = RowActions::new("TOK")
            .delete("/portal/admin/people/42/delete")
            .with_delete_confirm("Delete person libra@example.com?")
            .render()
            .into_string();
        assert!(
            html.contains("hx-post=\"/portal/admin/people/42/delete\""),
            "expected hx-post mirroring the action URL, got: {html}",
        );
        assert!(
            html.contains("hx-target=\"closest tr\""),
            "expected hx-target=closest tr so the row vanishes, got: {html}",
        );
        assert!(
            html.contains("hx-swap=\"outerHTML swap:200ms\""),
            "expected hx-swap with a 200ms transition, got: {html}",
        );
        assert!(
            html.contains("hx-confirm=\"Delete person libra@example.com?\""),
            "expected hx-confirm to mirror the prompt, got: {html}",
        );
    }

    #[test]
    fn csrf_token_is_threaded_when_present() {
        let html = RowActions::new("SESSION_TOKEN")
            .delete("/portal/admin/x/1/delete")
            .render()
            .into_string();
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"SESSION_TOKEN\""));
    }

    #[test]
    fn csrf_token_input_omitted_when_token_empty() {
        let html = RowActions::new("")
            .delete("/portal/admin/x/1/delete")
            .render()
            .into_string();
        assert!(!html.contains("name=\"_csrf\""));
    }

    #[test]
    fn row_label_disambiguates_aria_labels_per_row() {
        let html = RowActions::new("TOK")
            .edit("/portal/admin/people/42/edit")
            .delete("/portal/admin/people/42/delete")
            .with_row_label("libra@example.com")
            .render()
            .into_string();
        assert!(html.contains("aria-label=\"Edit libra@example.com\""));
        assert!(html.contains("aria-label=\"Delete libra@example.com\""));
        // Tooltip mirrors the ARIA label so sighted staff also see
        // the disambiguated verb on hover/focus.
        assert!(html.contains("title=\"Edit libra@example.com\""));
        assert!(html.contains("title=\"Delete libra@example.com\""));
    }

    #[test]
    fn empty_row_label_falls_back_to_bare_verb() {
        let html = RowActions::new("TOK")
            .edit("/x")
            .delete("/y")
            .render()
            .into_string();
        assert!(html.contains("aria-label=\"Edit\""));
        assert!(html.contains("aria-label=\"Delete\""));
    }

    #[test]
    fn delete_confirm_can_be_overridden_per_row() {
        let html = RowActions::new("TOK")
            .delete("/portal/admin/people/42/delete")
            .with_delete_confirm("Delete person libra@example.com?")
            .render()
            .into_string();
        assert!(html.contains("Delete person libra@example.com?"));
        // Default prompt must NOT leak through when an override is set.
        assert!(!html.contains("Are you sure?"));
    }

    #[test]
    fn apostrophe_in_confirm_prompt_is_escaped() {
        let html = RowActions::new("TOK")
            .delete("/x")
            .with_delete_confirm("can't undo")
            .render()
            .into_string();
        assert!(
            html.contains("can&apos;t undo") || html.contains("can&amp;apos;t undo"),
            "apostrophe must be entity-escaped, got: {html}",
        );
    }

    #[test]
    fn empty_confirm_message_omits_onsubmit() {
        let html = RowActions::new("TOK")
            .delete("/x")
            .with_delete_confirm("")
            .render()
            .into_string();
        assert!(
            !html.contains("onsubmit"),
            "empty confirm should opt out of the prompt entirely: {html}",
        );
    }

    #[test]
    fn icon_glyphs_are_aria_hidden_so_screen_readers_use_button_labels() {
        // The <i> glyph is decorative; the surrounding <a>/<button>
        // carries the real ARIA label. Hiding the icon prevents a
        // screen reader from announcing it twice.
        let html = RowActions::new("TOK")
            .edit("/x")
            .delete("/y")
            .render()
            .into_string();
        // Two glyphs, both aria-hidden.
        let hidden_count = html.matches("aria-hidden=\"true\"").count();
        assert!(
            hidden_count >= 2,
            "expected both glyphs aria-hidden, got: {html}",
        );
    }

    #[test]
    fn edit_only_row_renders_no_delete_form() {
        let html = RowActions::new("TOK").edit("/x").render().into_string();
        assert!(html.contains("href=\"/x\""));
        assert!(!html.contains("<form"));
        assert!(!html.contains("bi-trash3-fill"));
    }

    #[test]
    fn delete_only_row_renders_no_edit_link() {
        let html = RowActions::new("TOK").delete("/x").render().into_string();
        assert!(!html.contains("bi-pencil-square"));
        assert!(html.contains("<form"));
        assert!(html.contains("action=\"/x\""));
    }

    #[test]
    fn both_glyphs_live_in_the_same_wrapper_for_one_cell_packing() {
        // Both verbs land inside one .btn-group wrapper so the table
        // cell stays a single column — the original ask was to pack
        // Edit and Delete into the same row-action cell.
        let html = RowActions::new("TOK")
            .edit("/e")
            .delete("/d")
            .render()
            .into_string();
        let start = html
            .find("<div class=\"btn-group btn-group-sm row-actions\"")
            .expect("wrapper div present");
        let end = html[start..].find("</div>").expect("wrapper closes");
        let inner = &html[start..start + end];
        assert!(
            inner.contains("bi-pencil-square") && inner.contains("bi-trash3-fill"),
            "both glyphs must live in the same wrapper div, got inner: {inner}",
        );
    }
}
