//! Declarative Bootstrap 5 form builder — the reusable form chrome
//! shared by every portal create & edit page.
//!
//! A page describes its form as a [`FormCard`] (title, action, a list
//! of [`Field`]s, a submit label, an optional cancel link) and calls
//! [`FormCard::render`]. The component owns the canonical Bootstrap
//! markup so no page hand-rolls it:
//!
//! - a constrained `card` column (`shadow-sm`) so the form has a
//!   readable line length instead of stretching the full container,
//! - `mb-3` rhythm between fields plus `form-label` / `form-control`
//!   / `form-select` chrome,
//! - a `form-text` hint rendered under any field that carries one,
//!   wired to its control with `aria-describedby` so screen readers
//!   read the hint when the field is focused,
//! - a visible `*` on required fields (the native `required`
//!   attribute carries the state to assistive tech; the mark is
//!   `aria-hidden` so it isn't announced twice),
//! - an `alert-danger`, `role="alert"` banner for the form-level
//!   error that takes focus on load so a keyboard / screen-reader
//!   user hears what went wrong instead of landing silently in the
//!   first field.
//!
//! Focus order is left to the DOM — no positive `tabindex`, which
//! would otherwise hoist the form ahead of the page nav in the tab
//! sequence (an axe / WCAG 2.4.3 failure). A standalone form page
//! autofocuses its first field; an embedded form (`section_heading`)
//! does not, so it never steals focus from the page it sits in.
//!
//! The `<form>` keeps the `admin-form` class the browser e2e suite
//! locates it by.

use maud::{html, Markup};

/// One `<option>` inside a [`FieldKind::Select`]. Callers that need a
/// "Choose…" / "—" placeholder row include it as the first choice
/// (an empty `value` keeps `required` validation honest).
pub struct Choice<'a> {
    pub value: &'a str,
    pub label: &'a str,
}

impl<'a> Choice<'a> {
    #[must_use]
    pub fn new(value: &'a str, label: &'a str) -> Self {
        Self { value, label }
    }
}

/// The control a [`Field`] renders.
pub enum FieldKind<'a> {
    /// `<input type=…>` — text, email, number, file, …
    Input {
        input_type: &'a str,
        value: &'a str,
        placeholder: Option<&'a str>,
        prefix: Option<&'a str>,
        step: Option<&'a str>,
    },
    /// Multi-line `<textarea>`.
    Textarea { value: &'a str, rows: u8 },
    /// `<select>` with options + an optional preselected value.
    Select {
        options: Vec<Choice<'a>>,
        selected: Option<&'a str>,
        disabled: bool,
    },
    /// Single `<input type=checkbox>` rendered as a Bootstrap
    /// `form-check`. `value` is what posts when the box is ticked.
    Checkbox { value: &'a str, checked: bool },
}

/// A labeled form control plus its optional helper text. Build one
/// with a constructor ([`Field::text`], [`Field::select`], …) and
/// chain the optional modifiers ([`Field::required`],
/// [`Field::help`], …).
pub struct Field<'a> {
    label: &'a str,
    name: &'a str,
    required: bool,
    help: Option<&'a str>,
    kind: FieldKind<'a>,
}

impl<'a> Field<'a> {
    fn new(label: &'a str, name: &'a str, kind: FieldKind<'a>) -> Self {
        Self {
            label,
            name,
            required: false,
            help: None,
            kind,
        }
    }

    /// A single-line text input.
    #[must_use]
    pub fn text(label: &'a str, name: &'a str, value: &'a str) -> Self {
        Self::input(label, name, value, "text")
    }

    /// An email input (mobile keyboards + browser validation).
    #[must_use]
    pub fn email(label: &'a str, name: &'a str, value: &'a str) -> Self {
        Self::input(label, name, value, "email")
    }

    /// A numeric input.
    #[must_use]
    pub fn number(label: &'a str, name: &'a str, value: &'a str) -> Self {
        Self::input(label, name, value, "number")
    }

    /// A file picker.
    #[must_use]
    pub fn file(label: &'a str, name: &'a str) -> Self {
        Self::input(label, name, "", "file")
    }

    /// A typed text input — escape hatch for less common `type`s.
    #[must_use]
    pub fn input(label: &'a str, name: &'a str, value: &'a str, input_type: &'a str) -> Self {
        Self::new(
            label,
            name,
            FieldKind::Input {
                input_type,
                value,
                placeholder: None,
                prefix: None,
                step: None,
            },
        )
    }

    /// A multi-line textarea with a fixed visible row count.
    #[must_use]
    pub fn textarea(label: &'a str, name: &'a str, value: &'a str, rows: u8) -> Self {
        Self::new(label, name, FieldKind::Textarea { value, rows })
    }

    /// A dropdown. Include a placeholder choice as `options[0]` when
    /// the field is optional or should start unselected.
    #[must_use]
    pub fn select(
        label: &'a str,
        name: &'a str,
        options: Vec<Choice<'a>>,
        selected: Option<&'a str>,
    ) -> Self {
        Self::new(
            label,
            name,
            FieldKind::Select {
                options,
                selected,
                disabled: false,
            },
        )
    }

    /// A single checkbox; `value` is posted when ticked.
    #[must_use]
    pub fn checkbox(label: &'a str, name: &'a str, value: &'a str, checked: bool) -> Self {
        Self::new(label, name, FieldKind::Checkbox { value, checked })
    }

    /// Mark the field `required` (HTML5 + visual).
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Attach muted helper text rendered as `form-text` below the
    /// control.
    #[must_use]
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }

    /// Set the placeholder (no-op on non-`Input` kinds).
    #[must_use]
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        if let FieldKind::Input { placeholder: p, .. } = &mut self.kind {
            *p = Some(placeholder);
        }
        self
    }

    /// Render a short text prefix inside a Bootstrap input group.
    #[must_use]
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        if let FieldKind::Input { prefix: p, .. } = &mut self.kind {
            *p = Some(prefix);
        }
        self
    }

    /// Set the numeric `step` increment (no-op on non-`Input` kinds).
    #[must_use]
    pub fn step(mut self, step: &'a str) -> Self {
        if let FieldKind::Input { step: s, .. } = &mut self.kind {
            *s = Some(step);
        }
        self
    }

    /// Render the `<select>` disabled (no-op on non-`Select` kinds).
    #[must_use]
    pub fn disabled(mut self) -> Self {
        if let FieldKind::Select { disabled, .. } = &mut self.kind {
            *disabled = true;
        }
        self
    }

    /// Render this field, autofocusing it when `autofocus` is set
    /// (only the first field of a standalone form page is). The hint,
    /// when present, is tied to the control via `aria-describedby`;
    /// required fields gain a visible `aria-hidden` `*`.
    fn render(&self, autofocus: bool) -> Markup {
        let help_id = format!("{}-help", self.name);
        // Point the control at its hint so a screen reader reads the
        // hint on focus; `None` when there is no hint.
        let described_by = self.help.map(|_| help_id.as_str());
        let help = html! {
            @if let Some(h) = self.help {
                div."form-text" id=(help_id) { (h) }
            }
        };
        // The native `required` attribute conveys the state to AT;
        // this mark is the *visible* cue, hidden from the AT so it is
        // not announced as a stray "star".
        let required_mark = html! {
            @if self.required {
                span."text-danger"."ms-1" aria-hidden="true" { "*" }
            }
        };
        match &self.kind {
            FieldKind::Checkbox { value, checked } => html! {
                div."mb-3"."form-check" {
                    input."form-check-input" type="checkbox" id=(self.name) name=(self.name)
                        value=(value) checked[*checked]
                        aria-describedby=[described_by] autofocus[autofocus];
                    label."form-check-label" for=(self.name) { (self.label) }
                    (help)
                }
            },
            FieldKind::Input {
                input_type,
                value,
                placeholder,
                prefix,
                step,
            } => html! {
                div."mb-3" {
                    label."form-label" for=(self.name) { (self.label) (required_mark) }
                    @if let Some(prefix) = prefix {
                        div."input-group" {
                            span."input-group-text" { (prefix) }
                            input."form-control" type=(input_type) id=(self.name) name=(self.name)
                                value=(value) placeholder=[*placeholder] step=[*step] required[self.required]
                                aria-describedby=[described_by] autofocus[autofocus];
                        }
                    } @else {
                        input."form-control" type=(input_type) id=(self.name) name=(self.name)
                            value=(value) placeholder=[*placeholder] step=[*step] required[self.required]
                            aria-describedby=[described_by] autofocus[autofocus];
                    }
                    (help)
                }
            },
            FieldKind::Textarea { value, rows } => html! {
                div."mb-3" {
                    label."form-label" for=(self.name) { (self.label) (required_mark) }
                    textarea."form-control" id=(self.name) name=(self.name) rows=(rows)
                        required[self.required] aria-describedby=[described_by]
                        autofocus[autofocus] { (value) }
                    (help)
                }
            },
            FieldKind::Select {
                options,
                selected,
                disabled,
            } => html! {
                div."mb-3" {
                    label."form-label" for=(self.name) { (self.label) (required_mark) }
                    select."form-select" id=(self.name) name=(self.name)
                        required[self.required] disabled[*disabled]
                        aria-describedby=[described_by] autofocus[autofocus] {
                        @for o in options {
                            @if Some(o.value) == *selected {
                                option value=(o.value) selected { (o.label) }
                            } @else {
                                option value=(o.value) { (o.label) }
                            }
                        }
                    }
                    (help)
                }
            },
        }
    }
}

/// The heading rendered at the top of the card. A standalone form
/// page owns the page's `h1`; a form embedded under an existing `h1`
/// (e.g. the project detail page) uses `h2`.
#[derive(Clone, Copy)]
pub enum Heading {
    H1,
    H2,
}

/// A complete create/edit form rendered as a constrained Bootstrap
/// card. See the module docs for the chrome it owns.
pub struct FormCard<'a> {
    title: &'a str,
    action: &'a str,
    submit_label: &'a str,
    method: &'a str,
    fields: Vec<Field<'a>>,
    csrf_token: Option<&'a str>,
    hidden: Vec<(&'a str, &'a str)>,
    cancel_href: Option<&'a str>,
    cancel_label: &'a str,
    error: Option<&'a str>,
    intro: Option<Markup>,
    extra_fields: Option<Markup>,
    footer: Option<Markup>,
    multipart: bool,
    heading: Heading,
    autofocus: bool,
    centered: bool,
}

impl<'a> FormCard<'a> {
    /// A POST form titled `title`, submitting to `action`, with the
    /// primary button labeled `submit_label`.
    #[must_use]
    pub fn new(title: &'a str, action: &'a str, submit_label: &'a str) -> Self {
        Self {
            title,
            action,
            submit_label,
            method: "post",
            fields: Vec::new(),
            csrf_token: None,
            hidden: Vec::new(),
            cancel_href: None,
            cancel_label: "Cancel",
            error: None,
            intro: None,
            extra_fields: None,
            footer: None,
            multipart: false,
            heading: Heading::H1,
            autofocus: true,
            centered: false,
        }
    }

    #[must_use]
    pub fn fields(mut self, fields: Vec<Field<'a>>) -> Self {
        self.fields = fields;
        self
    }

    /// Thread the per-session CSRF token. Empty string renders no
    /// hidden input (dev/test paths without a session).
    #[must_use]
    pub fn csrf(mut self, token: &'a str) -> Self {
        self.csrf_token = Some(token);
        self
    }

    /// Append a hidden `<input>` carried through the POST — passthrough
    /// state the handler needs but the user never edits (e.g. a
    /// `return_to` URL, or a non-session CSRF token whose field name
    /// isn't the admin `_csrf`). Renders inside the form before the
    /// first visible field. Chain it once per pair.
    #[must_use]
    pub fn hidden(mut self, name: &'a str, value: &'a str) -> Self {
        self.hidden.push((name, value));
        self
    }

    /// Supplemental content rendered inside the card, below the form's
    /// submit row — for a secondary action that is a sibling of the
    /// form rather than part of it (e.g. a federated "Sign in with
    /// Google" button beside an email/password form).
    #[must_use]
    pub fn footer(mut self, footer: Markup) -> Self {
        self.footer = Some(footer);
        self
    }

    /// Center the card in its row and narrow the column — for a
    /// standalone auth/landing card rather than a form embedded in a
    /// left-aligned page body.
    #[must_use]
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    /// Add a "Cancel" link back to `href` beside the submit button.
    #[must_use]
    pub fn cancel(mut self, href: &'a str) -> Self {
        self.cancel_href = Some(href);
        self
    }

    /// Add a secondary link back to `href`, labeled `label`, beside
    /// the submit button — for when "Cancel" isn't the right verb
    /// (e.g. "Save and exit").
    #[must_use]
    pub fn cancel_labeled(mut self, href: &'a str, label: &'a str) -> Self {
        self.cancel_href = Some(href);
        self.cancel_label = label;
        self
    }

    /// Show a form-level error banner when `error` is `Some`.
    #[must_use]
    pub fn error(mut self, error: Option<&'a str>) -> Self {
        self.error = error;
        self
    }

    /// Muted introductory prose between the title and the fields.
    #[must_use]
    pub fn intro(mut self, intro: Markup) -> Self {
        self.intro = Some(intro);
        self
    }

    /// Pre-rendered controls appended inside the `<form>` after the
    /// [`Field`]s — for composite widgets a borrowed `Field` can't
    /// express (e.g. the `people_list` row groups). The markup posts
    /// with the rest of the form.
    #[must_use]
    pub fn extra_fields(mut self, extra: Markup) -> Self {
        self.extra_fields = Some(extra);
        self
    }

    /// Mark the form `multipart/form-data` (file uploads).
    #[must_use]
    pub fn multipart(mut self) -> Self {
        self.multipart = true;
        self
    }

    /// Render the title as `h2` and suppress field autofocus — for a
    /// form embedded beneath an existing page `h1`, so it never
    /// steals focus from the page it sits in.
    #[must_use]
    pub fn section_heading(mut self) -> Self {
        self.heading = Heading::H2;
        self.autofocus = false;
        self
    }

    /// Opt this form's first field out of `autofocus` (e.g. several
    /// peer form pages on one route, or a page where landing focus
    /// belongs elsewhere).
    #[must_use]
    pub fn no_autofocus(mut self) -> Self {
        self.autofocus = false;
        self
    }

    #[must_use]
    pub fn render(&self) -> Markup {
        // On error, pull focus to the alert (not the first field) so
        // the failure is announced; otherwise autofocus the first
        // field of a standalone form page.
        let focus_error = self.autofocus && self.error.is_some();
        let focus_field = self.autofocus && self.error.is_none();
        let inner = html! {
            @match self.heading {
                Heading::H1 => h1."h3"."mb-3" { (self.title) },
                Heading::H2 => h2."h4"."mb-3" { (self.title) },
            }
            @if let Some(intro) = &self.intro {
                div."text-body-secondary"."mb-4" { (intro) }
            }
            @if let Some(err) = self.error {
                // `tabindex=-1` makes the banner programmatically
                // focusable; `autofocus` lands the user on it so the
                // error is read before they reach the fields.
                div."alert"."alert-danger" role="alert" tabindex="-1" autofocus[focus_error] {
                    (err)
                }
            }
            form."admin-form" method=(self.method) action=(self.action)
                aria-label=(self.title)
                enctype=[self.multipart.then_some("multipart/form-data")] {
                @if let Some(token) = self.csrf_token {
                    @if !token.is_empty() {
                        input type="hidden" name="_csrf" value=(token);
                    }
                }
                @for (name, value) in &self.hidden {
                    input type="hidden" name=(*name) value=(*value);
                }
                @for (i, f) in self.fields.iter().enumerate() {
                    (f.render(focus_field && i == 0))
                }
                @if let Some(extra) = &self.extra_fields {
                    (extra)
                }
                div."d-flex"."gap-2"."mt-4" {
                    button."btn"."btn-primary" type="submit" { (self.submit_label) }
                    @if let Some(href) = self.cancel_href {
                        a."btn"."btn-outline-secondary" href=(href) { (self.cancel_label) }
                    }
                }
            }
            @if let Some(footer) = &self.footer {
                (footer)
            }
        };
        // A standalone auth card centers in a narrow column; an
        // embedded admin form stays left-aligned at a readable width.
        let (row_class, col_class) = if self.centered {
            (
                "row justify-content-center",
                "col-12 col-sm-10 col-md-7 col-lg-5 col-xl-4",
            )
        } else {
            ("row", "col-12 col-lg-7 col-xl-6")
        };
        html! {
            div class=(row_class) {
                div class=(col_class) {
                    div."card"."shadow-sm" {
                        div."card-body"."p-4" { (inner) }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Choice, Field, FormCard};

    #[test]
    fn input_field_wears_bootstrap_chrome() {
        let html = FormCard::new("Edit", "/x", "Save")
            .fields(vec![Field::email("Email", "email", "a@b").required()])
            .render()
            .into_string();
        // Constrained card column.
        assert!(html.contains("class=\"card shadow-sm\""), "{html}");
        assert!(html.contains("col-lg-7"), "{html}");
        // Field chrome — label tied to input via for/id.
        assert!(html.contains("class=\"mb-3\""));
        assert!(html.contains("<label class=\"form-label\" for=\"email\">Email"));
        assert!(html.contains("class=\"form-control\""));
        assert!(html.contains("id=\"email\""));
        assert!(html.contains("type=\"email\""));
        assert!(html.contains("required"));
    }

    #[test]
    fn keeps_admin_form_class_for_the_e2e_selector() {
        let html = FormCard::new("Edit", "/x", "Save").render().into_string();
        assert!(
            html.contains("class=\"admin-form\""),
            "browser e2e locates the form by .admin-form, got: {html}",
        );
    }

    #[test]
    fn never_emits_a_positive_tabindex() {
        // Positive tabindex hoists the form ahead of the page nav in
        // the tab sequence (axe / WCAG 2.4.3 failure). The only
        // tabindex we permit is -1 on the focusable error banner.
        let html = FormCard::new("Add", "/x", "Create")
            .fields(vec![
                Field::text("Name", "name", ""),
                Field::text("Slug", "slug", ""),
            ])
            .cancel("/back")
            .error(Some("nope"))
            .render()
            .into_string();
        for stop in [
            "tabindex=\"0\"",
            "tabindex=\"1\"",
            "tabindex=\"2\"",
            "tabindex=\"3\"",
        ] {
            assert!(!html.contains(stop), "unexpected {stop} in: {html}");
        }
        assert!(
            html.contains("tabindex=\"-1\""),
            "error banner should be focusable: {html}"
        );
    }

    #[test]
    fn the_form_carries_an_accessible_name() {
        let html = FormCard::new("Edit person", "/x", "Save")
            .render()
            .into_string();
        assert!(html.contains("aria-label=\"Edit person\""), "{html}");
    }

    #[test]
    fn required_field_shows_a_visible_aria_hidden_marker() {
        let html = FormCard::new("Add", "/x", "Create")
            .fields(vec![Field::text("Name", "name", "").required()])
            .render()
            .into_string();
        assert!(
            html.contains("<span class=\"text-danger ms-1\" aria-hidden=\"true\">*</span>"),
            "{html}",
        );
    }

    #[test]
    fn only_the_first_field_is_autofocused() {
        let html = FormCard::new("Add", "/x", "Create")
            .fields(vec![
                Field::text("Name", "name", ""),
                Field::text("Slug", "slug", ""),
            ])
            .render()
            .into_string();
        assert_eq!(html.matches("autofocus").count(), 1, "{html}");
    }

    #[test]
    fn section_form_does_not_autofocus_anything() {
        // An embedded form must not yank focus into a content page.
        let html = FormCard::new("Upload", "/x", "Upload")
            .section_heading()
            .fields(vec![Field::file("File", "file").required()])
            .render()
            .into_string();
        assert!(!html.contains("autofocus"), "{html}");
    }

    #[test]
    fn error_takes_focus_instead_of_the_first_field() {
        // On error the banner is autofocused and the first field is
        // not, so the failure is announced before the user is in a
        // field.
        let html = FormCard::new("Edit", "/x", "Save")
            .fields(vec![Field::email("Email", "email", "bad").required()])
            .error(Some("Email is invalid"))
            .render()
            .into_string();
        assert_eq!(
            html.matches("autofocus").count(),
            1,
            "exactly one autofocus: {html}"
        );
        // The single autofocus sits on the alert, before the input.
        let alert = html.find("alert-danger").expect("alert present");
        let input = html.find("form-control").expect("input present");
        let focus = html.find("autofocus").expect("autofocus present");
        assert!(
            focus < input && focus > alert,
            "autofocus should be on the alert: {html}"
        );
    }

    #[test]
    fn select_preselects_and_can_disable() {
        let html = FormCard::new("Edit", "/x", "Save")
            .fields(vec![Field::select(
                "Role",
                "role",
                vec![
                    Choice::new("client", "Client"),
                    Choice::new("staff", "Staff"),
                ],
                Some("staff"),
            )
            .disabled()])
            .render()
            .into_string();
        assert!(html.contains("class=\"form-select\""));
        assert!(html.contains("<option value=\"staff\" selected>Staff</option>"));
        assert!(html.contains("disabled"));
    }

    #[test]
    fn helper_text_renders_and_is_wired_via_aria_describedby() {
        let html = FormCard::new("Edit", "/x", "Save")
            .fields(vec![Field::text("Kind", "kind", "").help("Optional.")])
            .render()
            .into_string();
        // The hint carries an id and the control points at it, so a
        // screen reader reads the hint when the field is focused.
        assert!(
            html.contains("<div class=\"form-text\" id=\"kind-help\">Optional.</div>"),
            "{html}",
        );
        assert!(html.contains("aria-describedby=\"kind-help\""), "{html}");
    }

    #[test]
    fn fields_without_a_hint_omit_aria_describedby() {
        let html = FormCard::new("Edit", "/x", "Save")
            .fields(vec![Field::text("Name", "name", "")])
            .render()
            .into_string();
        assert!(!html.contains("aria-describedby"), "{html}");
    }

    #[test]
    fn csrf_hidden_input_rendered_only_when_token_present() {
        let with = FormCard::new("X", "/x", "Go")
            .csrf("TOK")
            .render()
            .into_string();
        assert!(with.contains("name=\"_csrf\""));
        assert!(with.contains("value=\"TOK\""));

        let without = FormCard::new("X", "/x", "Go")
            .csrf("")
            .render()
            .into_string();
        assert!(!without.contains("name=\"_csrf\""));
    }

    #[test]
    fn multipart_sets_enctype() {
        let html = FormCard::new("Upload", "/x", "Upload")
            .multipart()
            .fields(vec![Field::file("File", "file").required()])
            .render()
            .into_string();
        assert!(html.contains("enctype=\"multipart/form-data\""));
        assert!(html.contains("type=\"file\""));
    }

    #[test]
    fn error_banner_is_announced_and_associated() {
        let html = FormCard::new("Edit", "/x", "Save")
            .error(Some("Email is invalid"))
            .render()
            .into_string();
        assert!(html.contains("class=\"alert alert-danger\""));
        assert!(html.contains("role=\"alert\""));
        assert!(html.contains("Email is invalid"));
    }

    #[test]
    fn section_heading_renders_h2() {
        let html = FormCard::new("Upload a document", "/x", "Upload")
            .section_heading()
            .render()
            .into_string();
        assert!(
            html.contains("<h2 class=\"h4 mb-3\">Upload a document</h2>"),
            "{html}"
        );
    }
}
