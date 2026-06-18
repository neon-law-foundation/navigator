//! Admin `/portal/admin/playbooks` pages: list, create form, edit-positions
//! form.
//!
//! A **playbook** is the set of negotiating positions a client Entity has
//! decided it wants — the yardstick the inbound-contract review measures a
//! third-party contract against. Each position is a row of
//! `topic | preferred | fallback | walk-away | severity`. The create/edit
//! forms carry the whole position set in one textarea (one position per
//! line, pipe-delimited) so an attorney edits the playbook as a block.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::form::{Choice, Field, FormCard};
use crate::components::row_actions::RowActions;
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

/// The pipe-delimited textarea contract, shown as form help so an attorney
/// knows the line shape.
pub const POSITIONS_HELP: &str =
    "One position per line: Topic | Preferred | Fallback | Walk-away | severity \
     (severity is low, medium, or high).";

pub struct PlaybookRow<'a> {
    pub id: Uuid,
    pub entity_name: &'a str,
    pub name: &'a str,
    pub position_count: usize,
    pub active: bool,
}

pub struct EntityChoice<'a> {
    pub id: Uuid,
    pub name: &'a str,
}

#[derive(Default)]
pub struct PlaybookForm<'a> {
    pub name: &'a str,
    pub entity_id: Option<Uuid>,
    /// The whole position set, one `topic | … | severity` line per row.
    pub positions_text: &'a str,
    pub error: Option<&'a str>,
}

/// The playbooks list view.
#[must_use]
pub fn list(rows: &[PlaybookRow<'_>], csrf_token: &str, sort: &SortSpec) -> Markup {
    let columns = [
        Column::sortable("entity", "Company"),
        Column::sortable("name", "Playbook"),
        Column::fixed("positions", "Positions"),
        Column::fixed("active", "Active"),
        Column::fixed("actions", ""),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.entity_name) },
                html! { (r.name) },
                html! { (r.position_count) },
                html! { @if r.active { "Yes" } @else { "No" } },
                action_cell(r, csrf_token),
            ]
        })
        .collect();
    let body = html! {
        section.admin {
            div.container {
                header.page-header {
                    h1 { "Playbooks" }
                    p.lead {
                        "The negotiating positions a Company's inbound contracts are measured against."
                    }
                    p { a class="btn btn-primary" href="/portal/admin/playbooks/new" { "Add playbook" } }
                }
                @if rows.is_empty() {
                    p.empty {
                        "No playbooks yet. "
                        a href="/portal/admin/playbooks/new" { "Add the first." }
                    }
                } @else {
                    (data_table(
                        &columns,
                        &table_rows,
                        sort,
                        "/portal/admin/playbooks",
                        "No playbooks yet.",
                        &[],
                    ))
                }
            }
        }
    };
    PageLayout::new("Playbooks — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn action_cell(r: &PlaybookRow<'_>, csrf_token: &str) -> Markup {
    let edit_href = format!("/portal/admin/playbooks/{}/edit", r.id);
    RowActions::new(csrf_token).edit(&edit_href).render()
}

/// The create form — picks the Company, names the playbook, and enters the
/// positions.
#[must_use]
pub fn new_form(
    form: &PlaybookForm<'_>,
    entities: &[EntityChoice<'_>],
    csrf_token: &str,
) -> Markup {
    let entity_ids: Vec<String> = entities.iter().map(|e| e.id.to_string()).collect();
    let selected = form.entity_id.map(|id| id.to_string());
    let mut entity_opts = vec![Choice::new("", "Choose…")];
    entity_opts.extend(
        entity_ids
            .iter()
            .zip(entities)
            .map(|(id, e)| Choice::new(id, e.name)),
    );

    let fields = vec![
        Field::select("Company", "entity_id", entity_opts, selected.as_deref()).required(),
        Field::text("Playbook name", "name", form.name).required(),
        Field::textarea("Positions", "positions", form.positions_text, 10)
            .help(POSITIONS_HELP)
            .required(),
    ];
    form_page(
        "Add playbook",
        "/portal/admin/playbooks",
        "Create",
        fields,
        form.error,
        csrf_token,
    )
}

/// The edit-positions form — the Company + name are fixed context; the
/// attorney edits the position set.
#[must_use]
pub fn edit_form(id: Uuid, form: &PlaybookForm<'_>, entity_name: &str, csrf_token: &str) -> Markup {
    let context = format!("{entity_name} — {}", form.name);
    let fields = vec![
        Field::textarea("Positions", "positions", form.positions_text, 14)
            .help(POSITIONS_HELP)
            .required(),
    ];
    let action = format!("/portal/admin/playbooks/{id}");
    let body = html! {
        section.admin {
            div.container {
                header.page-header { h1 { "Edit playbook" } p.lead { (context) } }
                (FormCard::new("Positions", &action, "Save")
                    .csrf(csrf_token)
                    .fields(fields)
                    .error(form.error)
                    .cancel("/portal/admin/playbooks")
                    .render())
            }
        }
    };
    PageLayout::new("Edit playbook — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn form_page(
    title: &str,
    action: &str,
    submit_label: &str,
    fields: Vec<Field<'_>>,
    error: Option<&str>,
    csrf_token: &str,
) -> Markup {
    let body = html! {
        section.admin {
            div.container {
                (FormCard::new(title, action, submit_label)
                    .csrf(csrf_token)
                    .fields(fields)
                    .error(error)
                    .cancel("/portal/admin/playbooks")
                    .render())
            }
        }
    };
    PageLayout::new(title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{edit_form, list, new_form, EntityChoice, PlaybookForm, PlaybookRow};
    use crate::components::sort_spec::SortSpec;
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ENT: Uuid = Uuid::from_u128(9);

    #[test]
    fn list_empty_shows_add_link() {
        let html = list(&[], "", &SortSpec::default()).into_string();
        assert!(html.contains("No playbooks yet."));
    }

    #[test]
    fn list_renders_company_name_and_count_and_edit_link() {
        let rows = [PlaybookRow {
            id: ID1,
            entity_name: "Acme Inc",
            name: "Vendor MSA",
            position_count: 4,
            active: true,
        }];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(html.contains("Acme Inc"));
        assert!(html.contains("Vendor MSA"));
        assert!(html.contains(&format!("href=\"/portal/admin/playbooks/{ID1}/edit\"")));
    }

    #[test]
    fn new_form_renders_company_dropdown_and_positions_help() {
        let entities = [EntityChoice {
            id: ENT,
            name: "Acme Inc",
        }];
        let html = new_form(&PlaybookForm::default(), &entities, "TOK").into_string();
        assert!(html.contains("action=\"/portal/admin/playbooks\""));
        assert!(html.contains(&format!("<option value=\"{ENT}\">Acme Inc</option>")));
        assert!(html.contains("One position per line"));
        // CSRF threaded so the create POST passes require_csrf.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"TOK\""));
    }

    #[test]
    fn edit_form_prefills_positions_and_fixes_context() {
        let form = PlaybookForm {
            name: "Vendor MSA",
            entity_id: Some(ENT),
            positions_text: "Liability | mutual cap | 2x | uncapped | high",
            error: None,
        };
        let html = edit_form(ID1, &form, "Acme Inc", "TOK").into_string();
        assert!(html.contains(&format!("action=\"/portal/admin/playbooks/{ID1}\"")));
        assert!(html.contains("Acme Inc — Vendor MSA"));
        assert!(html.contains("Liability | mutual cap | 2x | uncapped | high"));
        assert!(html.contains("name=\"_csrf\""));
    }
}
