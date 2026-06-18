//! Admin /entity-types page: read-only list of entity types.
//!
//! Entity types are seeded by the workspace and aren't authored from
//! the web UI; the page is a transparency surface only — no Add /
//! Edit / Delete affordances. The column header is a sortable link
//! so staff can flip the view alphabetically when the seed list grows.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct Row<'a> {
    pub id: Uuid,
    pub name: &'a str,
}

#[must_use]
pub fn list(rows: &[Row<'_>], sort: &SortSpec) -> Markup {
    let columns = [Column::sortable("name", "Name")];
    let table_rows: Vec<Vec<Markup>> = rows.iter().map(|r| vec![html! { (r.name) }]).collect();
    let body = html! {
        section.admin { div.container {
            h1 { "Entity types" }
            (data_table(
                &columns,
                &table_rows,
                sort,
                "/portal/admin/entity-types",
                "No entity types yet.",
                &[],
            ))
        } }
    };
    PageLayout::new("Entity types — Admin")
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{list, Row};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);

    #[test]
    fn list_renders_rows() {
        let html = list(
            &[Row {
                id: ID1,
                name: "LLC",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(html.contains("LLC"));
    }

    #[test]
    fn list_has_no_crud_affordances() {
        let html = list(
            &[Row {
                id: ID1,
                name: "LLC",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(!html.contains("/portal/admin/entity-types/new"));
        assert!(!html.contains("/edit"));
        assert!(!html.contains("/delete"));
        assert!(!html.contains("<form"));
    }

    #[test]
    fn list_renders_empty_state_when_no_rows() {
        let html = list(&[], &SortSpec::default()).into_string();
        assert!(html.contains("No entity types yet."));
    }

    #[test]
    fn list_renders_sortable_name_header() {
        let html = list(
            &[Row {
                id: ID1,
                name: "LLC",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(html.contains("href=\"/portal/admin/entity-types?sort=name\""));
    }

    #[test]
    fn list_active_sort_descending_arrow_renders() {
        let html = list(
            &[Row {
                id: ID1,
                name: "LLC",
            }],
            &SortSpec::single("name", SortDirection::Descending),
        )
        .into_string();
        assert!(html.contains("↓"));
    }
}
