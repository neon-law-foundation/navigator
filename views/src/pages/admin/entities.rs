//! Admin /entities pages: list, create form, edit form.
//!
//! The list view renders a JSON:API 1.1 sortable table — column
//! headers are plain `<a href>` links targeting the same path with a
//! toggled `?sort=` parameter. Row identity is exposed via the per-row
//! Edit link href (`/portal/admin/entities/:id/edit`) so the ID column was
//! removed; resources are addressed by their links, not their primary
//! keys.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::form::{Choice, Field, FormCard};
use crate::components::row_actions::RowActions;
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct EntityRow<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub entity_type: &'a str,
    pub jurisdiction: &'a str,
}

pub struct TypeChoice<'a> {
    pub id: Uuid,
    pub name: &'a str,
}

pub struct JurisdictionChoice<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub code: &'a str,
}

#[derive(Default)]
pub struct EntityForm<'a> {
    pub name: &'a str,
    pub entity_type_id: Option<Uuid>,
    pub jurisdiction_id: Option<Uuid>,
    pub error: Option<&'a str>,
}

/// Render the entities list view.
///
/// `sort` is the parsed JSON:API `?sort=` value. `csrf_token` threads
/// through every per-row delete form so the CSRF middleware accepts
/// the POST. Pass an empty string only in tests that bypass the
/// middleware.
#[must_use]
pub fn list(rows: &[EntityRow<'_>], csrf_token: &str, sort: &SortSpec) -> Markup {
    let columns = [
        Column::sortable("name", "Name"),
        Column::sortable("entity_type", "Type"),
        Column::sortable("jurisdiction", "Jurisdiction"),
        Column::fixed("actions", ""),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.name) },
                html! { (r.entity_type) },
                html! { (r.jurisdiction) },
                action_cell(r, csrf_token),
            ]
        })
        .collect();
    let body = html! {
        section.admin {
            div.container {
                header.page-header {
                    h1 { "Entities" }
                    p { a class="btn btn-primary" href="/portal/admin/entities/new" { "Add entity" } }
                }
                @if rows.is_empty() {
                    p.empty { "No entities yet. " a href="/portal/admin/entities/new" { "Add the first." } }
                } @else {
                    (data_table(
                        &columns,
                        &table_rows,
                        sort,
                        "/portal/admin/entities",
                        "No entities yet.",
                        &[],
                    ))
                }
            }
        }
    };
    PageLayout::new("Entities — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

fn action_cell(r: &EntityRow<'_>, csrf_token: &str) -> Markup {
    let edit_href = format!("/portal/admin/entities/{}/edit", r.id);
    let delete_action = format!("/portal/admin/entities/{}/delete", r.id);
    let delete_confirm = format!("Delete entity {}?", r.name);
    RowActions::new(csrf_token)
        .edit(&edit_href)
        .delete(&delete_action)
        .with_delete_confirm(&delete_confirm)
        .with_row_label(r.name)
        .render()
}

#[must_use]
pub fn new_form(
    form: &EntityForm<'_>,
    types: &[TypeChoice<'_>],
    jurisdictions: &[JurisdictionChoice<'_>],
) -> Markup {
    form_page(
        "Add entity",
        "/portal/admin/entities",
        "Create",
        form,
        types,
        jurisdictions,
    )
}

#[must_use]
pub fn edit_form(
    id: Uuid,
    form: &EntityForm<'_>,
    types: &[TypeChoice<'_>],
    jurisdictions: &[JurisdictionChoice<'_>],
) -> Markup {
    form_page(
        "Edit entity",
        &format!("/portal/admin/entities/{id}"),
        "Save",
        form,
        types,
        jurisdictions,
    )
}

fn form_page(
    title: &str,
    action: &str,
    submit_label: &str,
    f: &EntityForm<'_>,
    types: &[TypeChoice<'_>],
    jurisdictions: &[JurisdictionChoice<'_>],
) -> Markup {
    // SeaORM keys are UUIDs; the select chrome takes string values,
    // so stage the option values/labels in locals the `Choice`s
    // borrow from for the duration of the render.
    let type_ids: Vec<String> = types.iter().map(|t| t.id.to_string()).collect();
    let jur_ids: Vec<String> = jurisdictions.iter().map(|j| j.id.to_string()).collect();
    let jur_labels: Vec<String> = jurisdictions
        .iter()
        .map(|j| format!("{} ({})", j.name, j.code))
        .collect();
    let selected_type = f.entity_type_id.map(|id| id.to_string());
    let selected_jur = f.jurisdiction_id.map(|id| id.to_string());

    let mut type_opts = vec![Choice::new("", "Choose…")];
    type_opts.extend(
        type_ids
            .iter()
            .zip(types)
            .map(|(id, t)| Choice::new(id, t.name)),
    );
    let mut jur_opts = vec![Choice::new("", "Choose…")];
    jur_opts.extend(
        jur_ids
            .iter()
            .zip(&jur_labels)
            .map(|(id, label)| Choice::new(id, label)),
    );

    let fields = vec![
        Field::text("Name", "name", f.name).required(),
        Field::select(
            "Entity type",
            "entity_type_id",
            type_opts,
            selected_type.as_deref(),
        )
        .required(),
        Field::select(
            "Jurisdiction",
            "jurisdiction_id",
            jur_opts,
            selected_jur.as_deref(),
        )
        .required(),
    ];

    let body = html! {
        section.admin {
            div.container {
                (FormCard::new(title, action, submit_label)
                    .fields(fields)
                    .error(f.error)
                    .cancel("/portal/admin/entities")
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
    use super::{edit_form, list, new_form, EntityForm, EntityRow, JurisdictionChoice, TypeChoice};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID2: Uuid = Uuid::from_u128(2);
    const ID5: Uuid = Uuid::from_u128(5);

    fn types() -> [TypeChoice<'static>; 2] {
        [
            TypeChoice {
                id: ID1,
                name: "LLC",
            },
            TypeChoice {
                id: ID2,
                name: "Trust",
            },
        ]
    }
    fn jurs() -> [JurisdictionChoice<'static>; 2] {
        [
            JurisdictionChoice {
                id: ID1,
                name: "Nevada",
                code: "NV",
            },
            JurisdictionChoice {
                id: ID2,
                name: "California",
                code: "CA",
            },
        ]
    }

    fn acme() -> EntityRow<'static> {
        EntityRow {
            id: ID1,
            name: "Acme",
            entity_type: "LLC",
            jurisdiction: "Nevada",
        }
    }

    #[test]
    fn list_empty_shows_add_link() {
        let html = list(&[], "", &SortSpec::default()).into_string();
        assert!(html.contains("No entities yet."));
    }

    #[test]
    fn list_renders_row_columns() {
        let rows = [acme()];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(html.contains("Acme"));
        assert!(html.contains("LLC"));
        assert!(html.contains("Nevada"));
        assert!(html.contains(&format!("href=\"/portal/admin/entities/{ID1}/edit\"")));
    }

    #[test]
    fn list_does_not_render_id_column() {
        // Row identity is exposed via the Edit-link href, not a
        // standalone ID cell, matching the people list convention.
        let rows = [acme()];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(
            !html.contains("<th>ID</th>"),
            "ID column header should be gone, got: {html}",
        );
        assert!(
            !html.contains(&format!("<td>{ID1}</td>")),
            "ID cell should be gone, got: {html}",
        );
    }

    #[test]
    fn list_renders_sortable_headers_as_href_links() {
        let rows = [acme()];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(
            html.contains("href=\"/portal/admin/entities?sort=name\""),
            "Name header should link to ?sort=name, got: {html}",
        );
        assert!(html.contains("href=\"/portal/admin/entities?sort=entity_type\""));
        assert!(html.contains("href=\"/portal/admin/entities?sort=jurisdiction\""));
    }

    #[test]
    fn list_active_sort_descending_arrow_renders() {
        let rows = [acme()];
        let html = list(
            &rows,
            "TOK",
            &SortSpec::single("name", SortDirection::Descending),
        )
        .into_string();
        assert!(html.contains("↓"), "expected descending arrow: {html}");
    }

    #[test]
    fn list_renders_row_actions_icons() {
        let rows = [acme()];
        let html = list(&rows, "TOK", &SortSpec::default()).into_string();
        assert!(
            html.contains("class=\"bi bi-pencil-square\""),
            "expected pencil glyph: {html}",
        );
        assert!(
            html.contains("class=\"bi bi-trash3-fill\""),
            "expected filled trash glyph: {html}",
        );
    }

    #[test]
    fn list_delete_form_carries_csrf_and_confirm() {
        let rows = [acme()];
        let html = list(&rows, "SESSION_TOKEN", &SortSpec::default()).into_string();
        assert!(
            html.contains(&format!("action=\"/portal/admin/entities/{ID1}/delete\"")),
            "delete action route missing: {html}",
        );
        // CSRF threaded.
        assert!(html.contains("name=\"_csrf\""));
        assert!(html.contains("value=\"SESSION_TOKEN\""));
        // Confirm echoes the row identity.
        assert!(
            html.contains("Delete entity Acme?"),
            "confirm prompt should name the entity: {html}",
        );
    }

    #[test]
    fn new_form_renders_dropdowns() {
        let html = new_form(&EntityForm::default(), &types(), &jurs()).into_string();
        assert!(html.contains("action=\"/portal/admin/entities\""));
        assert!(html.contains(&format!("<option value=\"{ID1}\">LLC</option>")));
        assert!(html.contains(&format!("<option value=\"{ID2}\">Trust</option>")));
        assert!(html.contains("Nevada (NV)"));
        assert!(html.contains("California (CA)"));
    }

    #[test]
    fn edit_form_preselects_options() {
        let html = edit_form(
            ID5,
            &EntityForm {
                name: "Acme",
                entity_type_id: Some(ID1),
                jurisdiction_id: Some(ID2),
                error: None,
            },
            &types(),
            &jurs(),
        )
        .into_string();
        assert!(html.contains(&format!("action=\"/portal/admin/entities/{ID5}\"")));
        assert!(html.contains(&format!("<option value=\"{ID1}\" selected>LLC</option>")));
        assert!(html.contains(&format!(
            "<option value=\"{ID2}\" selected>California (CA)</option>"
        )));
        assert!(html.contains("value=\"Acme\""));
    }
}
