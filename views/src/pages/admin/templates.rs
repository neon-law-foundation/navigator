//! Admin /templates page: read-only list of notation templates.
//!
//! Templates are authored as markdown files in `templates/` and
//! loaded into the DB by the `cli import` command. The web admin
//! view is a transparency surface only — no Add / Edit / Delete.
//! Column headers are sortable so staff can flip alphabetically by
//! code, title, or respondent type without grepping the catalog.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct Row<'a> {
    pub id: Uuid,
    pub code: &'a str,
    pub title: &'a str,
    pub respondent_type: &'a str,
}

#[must_use]
pub fn list(rows: &[Row<'_>], sort: &SortSpec) -> Markup {
    let columns = [
        Column::sortable("code", "Code"),
        Column::sortable("title", "Title"),
        Column::sortable("respondent_type", "Respondent"),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.code) },
                html! { (r.title) },
                html! { (r.respondent_type) },
            ]
        })
        .collect();
    let body = html! {
        section.admin { div.container {
            h1 { "Templates" }
            (data_table(
                &columns,
                &table_rows,
                sort,
                "/portal/admin/templates",
                "No templates yet.",
                &[],
            ))
        } }
    };
    PageLayout::new("Templates — Admin")
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
        let rows = [Row {
            id: ID1,
            code: "trusts__nevada",
            title: "Nevada Trust",
            respondent_type: "entity",
        }];
        let html = list(&rows, &SortSpec::default()).into_string();
        assert!(html.contains("Nevada Trust"));
        assert!(html.contains("trusts__nevada"));
        assert!(html.contains("entity"));
    }

    #[test]
    fn list_has_no_crud_affordances() {
        let html = list(
            &[Row {
                id: ID1,
                code: "x",
                title: "X",
                respondent_type: "person",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(!html.contains("/portal/admin/templates/new"));
        assert!(!html.contains("/edit"));
        assert!(!html.contains("/delete"));
        assert!(!html.contains("<form"));
    }

    #[test]
    fn list_renders_empty_state_when_no_rows() {
        let html = list(&[], &SortSpec::default()).into_string();
        assert!(html.contains("No templates yet."));
    }

    #[test]
    fn list_renders_sortable_headers() {
        let html = list(
            &[Row {
                id: ID1,
                code: "x",
                title: "X",
                respondent_type: "person",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(html.contains("href=\"/portal/admin/templates?sort=code\""));
        assert!(html.contains("href=\"/portal/admin/templates?sort=title\""));
        assert!(html.contains("href=\"/portal/admin/templates?sort=respondent_type\""));
    }

    #[test]
    fn list_active_sort_descending_arrow_renders() {
        let html = list(
            &[Row {
                id: ID1,
                code: "x",
                title: "X",
                respondent_type: "person",
            }],
            &SortSpec::single("title", SortDirection::Descending),
        )
        .into_string();
        assert!(html.contains("↓"));
    }
}
