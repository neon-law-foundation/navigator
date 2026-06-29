//! Admin /people pages: list, create form, edit form.
//!
//! The list view renders a JSON:API 1.1 sortable table — column
//! headers are plain `<a href>` links targeting the same path with
//! a toggled `?sort=` parameter. Filters arrive via `?filter[name]=`
//! / `?filter[email]=`; both survive through sort clicks via
//! [`data_table`]'s `extra_query` stitching.
//!
//! The `id` column is intentionally absent — row identity is
//! preserved through the per-row Edit link href (`/portal/admin/people/:id`).

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::form::{Choice, Field, FormCard};
use crate::components::row_actions::RowActions;
use crate::components::sort_spec::SortSpec;
use crate::components::toast::{toast_overlay, Toast};
use crate::PageLayout;

pub struct PersonRow<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub email: &'a str,
}

#[derive(Default)]
pub struct PersonForm<'a> {
    pub name: &'a str,
    pub email: &'a str,
    /// System-wide tier as the string token (`client`, `staff`,
    /// `admin`). Empty string is fine — the form posts it as
    /// `client` (the safe default) if the field is missing.
    pub role: &'a str,
    /// When true, the form blocks edits to the role field (renders
    /// it disabled). Used to lock the bootstrap admin row so an operator
    /// can't accidentally demote themselves from the UI.
    pub role_locked: bool,
    /// Per-session CSRF token, rendered as a hidden form field.
    /// Empty string suppresses the field — used by dev/test paths
    /// without a session.
    pub csrf_token: &'a str,
    pub error: Option<&'a str>,
    /// Xero `ContactID` cached on the person, when synced. Drives the
    /// "View in Xero" deep-link on the show view; `None` renders a muted
    /// "not synced yet" note instead. Unused by `new_form`.
    pub xero_contact_id: Option<&'a str>,
}

const ROLE_CHOICES: &[(&str, &str)] =
    &[("client", "Client"), ("staff", "Staff"), ("admin", "Admin")];

/// Render the people list view.
///
/// `sort` is the parsed JSON:API `?sort=` value; `extra_query`
/// carries any active `filter[*]` pairs so sort-link hrefs preserve
/// them. The handler is responsible for validating `sort` against
/// the allowed-key set before calling.
#[must_use]
pub fn list(
    rows: &[PersonRow<'_>],
    csrf_token: &str,
    sort: &SortSpec,
    extra_query: &[(&str, &str)],
) -> Markup {
    let columns = [
        Column::sortable("name", "Name"),
        Column::sortable("email", "Email"),
        Column::fixed("actions", ""),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.name) },
                html! { (r.email) },
                action_cell(r, csrf_token),
            ]
        })
        .collect();
    let body = html! {
        section.admin {
            div.container {
                header.page-header {
                    h1 { "People" }
                    p { a class="btn btn-primary" href="/portal/admin/people/new" { "Add person" } }
                }
                @if rows.is_empty() {
                    p.empty { "No people yet. " a href="/portal/admin/people/new" { "Add the first." } }
                } @else {
                    (data_table(
                        &columns,
                        &table_rows,
                        sort,
                        "/portal/admin/people",
                        "No people yet.",
                        extra_query,
                    ))
                }
            }
        }
    };
    PageLayout::new("People — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Per-row actions on the list: Edit + Delete only. "Send welcome email"
/// moved to the person show view (the detail/edit page) so the list stays
/// a scannable directory and the email send sits next to the person's
/// other per-record actions (Xero link, etc.).
fn action_cell(r: &PersonRow<'_>, csrf_token: &str) -> Markup {
    let delete_action = format!("/portal/admin/people/{}/delete", r.id);
    let edit_href = format!("/portal/admin/people/{}/edit", r.id);
    let delete_confirm = format!("Delete person {}?", r.email);
    let row_actions = RowActions::new(csrf_token)
        .edit(&edit_href)
        .delete(&delete_action)
        .with_delete_confirm(&delete_confirm)
        .with_row_label(r.email)
        .render();
    html! {
        span.row-actions-cell {
            (row_actions)
        }
    }
}

/// The person show view: the editable detail form plus a per-record
/// **Actions** panel — "Send welcome email" (moved here off the list) and
/// a "View in Xero" deep-link. The welcome POST and the Xero link live
/// outside the edit `<form>` (no nested forms) so they act on the record
/// independently of a field edit.
///
/// `notice` is an optional flash toast floated at the top-right on arrival
/// — the green confirmation after a welcome-email send (or a red one if the
/// dispatch failed). It mirrors the sign-in page's `LoginNotice` pattern:
/// the handler maps a `?notice=` query flag to a toned [`Toast`] and the
/// view renders it through the shared overlay.
#[must_use]
pub fn edit_form(id: Uuid, form: &PersonForm<'_>, notice: Option<&Toast>) -> Markup {
    let action = format!("/portal/admin/people/{id}");
    let body = html! {
        section.admin {
            div.container {
                @if let Some(toast) = notice {
                    (toast_overlay(&toast.render()))
                }
                (person_form_card("Edit person", &action, "Save", form))
                (person_actions(id, form))
            }
        }
    };
    PageLayout::new("Edit person")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// The per-record actions panel on the show view.
fn person_actions(id: Uuid, f: &PersonForm<'_>) -> Markup {
    let welcome_action = format!("/portal/admin/people/{id}/welcome");
    let welcome_confirm = format!("return confirm('Send welcome email to {}?')", f.email);
    html! {
        section.person-actions.card {
            h2 { "Actions" }
            div.action-row {
                form method="post" action=(welcome_action) onsubmit=(welcome_confirm)
                    aria-label="Send welcome email" {
                    @if !f.csrf_token.is_empty() {
                        input type="hidden" name="_csrf" value=(f.csrf_token);
                    }
                    button type="submit" class="btn btn-secondary" data-action="welcome" {
                        i.bi."bi-envelope-paper" aria-hidden="true" {}
                        " Send welcome email"
                    }
                }
                @match f.xero_contact_id {
                    Some(contact_id) => {
                        a class="btn btn-outline-secondary"
                          href=(format!("https://go.xero.com/Contacts/View/{contact_id}"))
                          target="_blank" rel="noopener noreferrer" {
                            i.bi."bi-box-arrow-up-right" aria-hidden="true" {}
                            " View in Xero"
                        }
                    }
                    None => {
                        span.muted.xero-unsynced {
                            "Not synced to Xero yet — a contact is created when the "
                            "matter-close fee is raised."
                        }
                    }
                }
            }
        }
    }
}

#[must_use]
pub fn new_form(form: &PersonForm<'_>) -> Markup {
    let body = html! {
        section.admin {
            div.container {
                (person_form_card("Add person", "/portal/admin/people", "Create", form))
            }
        }
    };
    PageLayout::new("Add person")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// The shared person form card (fields + role select + CSRF + error),
/// used by both the create and show views.
fn person_form_card(title: &str, action: &str, submit_label: &str, f: &PersonForm<'_>) -> Markup {
    let current = if f.role.is_empty() { "client" } else { f.role };
    let role_opts: Vec<Choice<'_>> = ROLE_CHOICES
        .iter()
        .map(|&(value, display)| Choice::new(value, display))
        .collect();
    let mut role = Field::select("Role", "role", role_opts, Some(current));
    if f.role_locked {
        // Lock the bootstrap admin row so an operator can't demote
        // themselves from the UI; the hint says where to change it.
        role = role.disabled().help(
            "The bootstrap admin role on this account cannot be changed from the UI. \
             Edit via NAVIGATOR_BOOTSTRAP_ADMIN_EMAIL or a direct DB write.",
        );
    }
    let fields = vec![
        Field::text("Name", "name", f.name).required(),
        Field::email("Email", "email", f.email).required(),
        role,
    ];

    FormCard::new(title, action, submit_label)
        .fields(fields)
        .csrf(f.csrf_token)
        .error(f.error)
        .cancel("/portal/admin/people")
        .render()
}

#[cfg(test)]
mod tests {
    use super::{edit_form, list, new_form, PersonForm, PersonRow, Toast};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID2: Uuid = Uuid::from_u128(2);
    const ID7: Uuid = Uuid::from_u128(7);

    fn libra() -> PersonRow<'static> {
        PersonRow {
            id: ID1,
            name: "Libra",
            email: "libra@example.com",
        }
    }

    #[test]
    fn list_empty_shows_add_first_link() {
        let html = list(&[], "", &SortSpec::default(), &[]).into_string();
        assert!(html.contains(&format!(
            "<title>{} | People — Admin</title>",
            crate::brand::FIRM_BRAND.site_name
        )));
        assert!(html.contains("No people yet."));
        assert!(html.contains("href=\"/portal/admin/people/new\""));
    }

    #[test]
    fn list_renders_each_row_with_edit_and_delete() {
        let rows = [
            libra(),
            PersonRow {
                id: ID2,
                name: "Taurus",
                email: "taurus@example.com",
            },
        ];
        let html = list(&rows, "TOK", &SortSpec::default(), &[]).into_string();
        assert!(html.contains(&format!("href=\"/portal/admin/people/{ID1}/edit\"")));
        assert!(html.contains(&format!("action=\"/portal/admin/people/{ID1}/delete\"")));
        assert!(html.contains(&format!("href=\"/portal/admin/people/{ID2}/edit\"")));
        assert!(html.contains("Libra"));
        assert!(html.contains("taurus@example.com"));
    }

    #[test]
    fn list_action_cell_renders_pencil_and_trash_glyphs() {
        // The unified RowActions cell packs Edit + Delete into a
        // single column with Bootstrap-Icons glyphs instead of plain
        // "Edit" / "Delete" text links.
        let rows = [libra()];
        let html = list(&rows, "TOK", &SortSpec::default(), &[]).into_string();
        assert!(
            html.contains("class=\"bi bi-pencil-square\""),
            "expected edit pencil glyph: {html}",
        );
        assert!(
            html.contains("class=\"bi bi-trash3-fill\""),
            "expected delete trash glyph: {html}",
        );
    }

    #[test]
    fn list_delete_confirm_echoes_row_identity() {
        // Council ask (Leo): the confirm dialog must echo who is
        // about to be deleted so a misclick is obvious to staff.
        let rows = [libra()];
        let html = list(&rows, "TOK", &SortSpec::default(), &[]).into_string();
        assert!(
            html.contains("Delete person libra@example.com?"),
            "delete confirm must name the row, got: {html}",
        );
        // Generic prompt must not leak through when an override is
        // set on the RowActions builder.
        assert!(!html.contains("Are you sure?"));
    }

    #[test]
    fn list_delete_forms_carry_csrf_token() {
        let rows = [libra()];
        let html = list(&rows, "SESSION_TOKEN", &SortSpec::default(), &[]).into_string();
        // Every per-row delete form must include the session's CSRF
        // hidden input or the POST will 403 at the CSRF middleware.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"SESSION_TOKEN\""));
    }

    #[test]
    fn list_does_not_render_id_column() {
        // Row identity is exposed via the Edit-link href, not a
        // standalone ID cell. The JSON:API contract is that resources
        // are addressed by their links, not their primary keys.
        let rows = [libra()];
        let html = list(&rows, "TOK", &SortSpec::default(), &[]).into_string();
        assert!(
            !html.contains("<th>ID</th>"),
            "ID column header should be gone, got: {html}",
        );
        // The id still appears in the Edit href — that's intentional.
        // But it must not appear as a standalone column cell.
        assert!(
            !html.contains(&format!("<td>{ID1}</td>")),
            "ID cell should be gone, got: {html}",
        );
    }

    #[test]
    fn list_renders_jsonapi_sortable_headers_as_href_links() {
        let rows = [libra()];
        let html = list(&rows, "TOK", &SortSpec::default(), &[]).into_string();
        // Plain anchor tags — no JavaScript, no form submission.
        assert!(
            html.contains("href=\"/portal/admin/people?sort=name\""),
            "sortable Name header should link to ?sort=name, got: {html}",
        );
        assert!(
            html.contains("href=\"/portal/admin/people?sort=email\""),
            "sortable Email header should link to ?sort=email, got: {html}",
        );
    }

    #[test]
    fn list_active_sort_ascending_flips_link_to_descending() {
        let rows = [libra()];
        let html = list(
            &rows,
            "TOK",
            &SortSpec::single("name", SortDirection::Ascending),
            &[],
        )
        .into_string();
        assert!(
            html.contains("href=\"/portal/admin/people?sort=-name\""),
            "active ascending sort should flip to descending, got: {html}",
        );
        // Up arrow rendered to indicate active direction.
        assert!(html.contains("↑"), "expected ascending arrow: {html}");
    }

    #[test]
    fn list_filter_extra_query_survives_sort_link() {
        // JSON:API filter[name]=ada stays stitched onto sort links so
        // toggling a sort doesn't drop the active filter.
        let rows = [libra()];
        let html = list(
            &rows,
            "TOK",
            &SortSpec::default(),
            &[("filter[name]", "ada")],
        )
        .into_string();
        // JSON:API allows unencoded brackets in filter keys; browsers
        // auto-encode on send, axum decodes back to the same string.
        // maud entity-escapes the `&` separator inside the attribute.
        assert!(
            html.contains("href=\"/portal/admin/people?filter[name]=ada&amp;sort=name\""),
            "filter must survive sort link, got: {html}",
        );
    }

    #[test]
    fn new_form_renders_fields_and_submit() {
        let html = new_form(&PersonForm::default()).into_string();
        assert!(html.contains("action=\"/portal/admin/people\""));
        assert!(html.contains("name=\"name\""));
        assert!(html.contains("name=\"email\""));
        assert!(html.contains(">Create</button>"));
    }

    #[test]
    fn edit_form_pre_fills_values_and_targets_id() {
        let html = edit_form(
            ID7,
            &PersonForm {
                name: "Staff",
                email: "staff@neonlaw.com",
                role: "staff",
                ..Default::default()
            },
            None,
        )
        .into_string();
        assert!(html.contains(&format!("action=\"/portal/admin/people/{ID7}\"")));
        assert!(html.contains("value=\"Staff\""));
        assert!(html.contains("value=\"staff@neonlaw.com\""));
        assert!(html.contains(">Save</button>"));
    }

    #[test]
    fn show_view_carries_welcome_button_with_recipient_confirm() {
        // "Send welcome email" lives on the show view now, not the list.
        let html = edit_form(
            ID7,
            &PersonForm {
                name: "Libra",
                email: "libra@example.com",
                csrf_token: "TOK",
                ..Default::default()
            },
            None,
        )
        .into_string();
        assert!(html.contains(&format!("action=\"/portal/admin/people/{ID7}/welcome\"")));
        assert!(html.contains("Send welcome email to libra@example.com?"));
        // The welcome POST carries the CSRF token or it 403s.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOK\""));
    }

    #[test]
    fn show_view_links_to_xero_when_contact_synced() {
        let html = edit_form(
            ID7,
            &PersonForm {
                name: "Libra",
                email: "libra@example.com",
                xero_contact_id: Some("xero-abc-123"),
                ..Default::default()
            },
            None,
        )
        .into_string();
        assert!(
            html.contains("href=\"https://go.xero.com/Contacts/View/xero-abc-123\""),
            "expected Xero deep-link, got: {html}",
        );
        assert!(html.contains("View in Xero"));
    }

    #[test]
    fn show_view_notes_unsynced_when_no_xero_contact() {
        let html = edit_form(ID7, &PersonForm::default(), None).into_string();
        assert!(
            !html.contains("go.xero.com"),
            "no link when unsynced: {html}"
        );
        assert!(html.contains("Not synced to Xero yet"));
    }

    #[test]
    fn show_view_floats_success_toast_when_welcome_sent() {
        // After a welcome-email send the handler redirects back here with
        // `?notice=welcome_sent`, mapped to a green confirmation toast.
        let html = edit_form(
            ID7,
            &PersonForm {
                name: "Libra",
                email: "libra@example.com",
                ..Default::default()
            },
            Some(&Toast::success("Welcome email sent to libra@example.com.")),
        )
        .into_string();
        // Green tone (Bootstrap success helper) + the overlay container.
        assert!(
            html.contains("text-bg-success"),
            "welcome toast must use the success color, got: {html}",
        );
        assert!(
            html.contains("toast-container"),
            "toast must be pinned in the overlay container, got: {html}",
        );
        assert!(html.contains("Welcome email sent to libra@example.com."));
    }

    #[test]
    fn show_view_renders_no_toast_without_a_notice() {
        let html = edit_form(ID7, &PersonForm::default(), None).into_string();
        assert!(
            !html.contains("toast-container"),
            "a plain show view must not float a toast, got: {html}",
        );
    }

    #[test]
    fn form_displays_error_message_when_present() {
        let html = new_form(&PersonForm {
            email: "bad",
            error: Some("Email is invalid"),
            ..Default::default()
        })
        .into_string();
        assert!(html.contains("Email is invalid"));
    }

    #[test]
    fn form_renders_csrf_hidden_input_when_token_present() {
        let html = new_form(&PersonForm {
            csrf_token: "TOKEN_VALUE_42",
            ..Default::default()
        })
        .into_string();
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOKEN_VALUE_42\""));
        assert!(html.contains("type=\"hidden\""));
    }

    #[test]
    fn form_omits_csrf_hidden_input_when_token_empty() {
        let html = new_form(&PersonForm::default()).into_string();
        assert!(!html.contains("name=\"_csrf\""));
    }
}
