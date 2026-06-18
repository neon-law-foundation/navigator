//! Admin /questions page: read-only list of questionnaire questions.
//!
//! Questions are seeded from template frontmatter via `cli import`.
//! The web admin view is a transparency surface only — no Add /
//! Edit / Delete affordances. Sortable headers let staff flip the
//! list alphabetically by code or by answer type.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct Row<'a> {
    pub id: Uuid,
    pub code: &'a str,
    pub prompt: &'a str,
    pub answer_type: &'a str,
}

#[must_use]
pub fn list(rows: &[Row<'_>], sort: &SortSpec) -> Markup {
    let columns = [
        Column::sortable("code", "Code"),
        Column::fixed("prompt", "Prompt"),
        Column::sortable("answer_type", "Answer type"),
    ];
    let table_rows: Vec<Vec<Markup>> = rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.code) },
                html! { (r.prompt) },
                html! { (r.answer_type) },
            ]
        })
        .collect();
    let body = html! {
        section.admin { div.container {
            h1 { "Questions" }
            (data_table(
                &columns,
                &table_rows,
                sort,
                "/portal/admin/questions",
                "No questions yet.",
                &[],
            ))
        } }
    };
    PageLayout::new("Questions — Admin")
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
                code: "name",
                prompt: "What is your name?",
                answer_type: "string",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(html.contains("What is your name?"));
        assert!(html.contains("name"));
        assert!(html.contains("string"));
    }

    #[test]
    fn list_has_no_crud_affordances() {
        let html = list(
            &[Row {
                id: ID1,
                code: "name",
                prompt: "What is your name?",
                answer_type: "string",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(!html.contains("/portal/admin/questions/new"));
        assert!(!html.contains("/edit"));
        assert!(!html.contains("/delete"));
        assert!(!html.contains("<form"));
    }

    #[test]
    fn list_renders_empty_state_when_no_rows() {
        let html = list(&[], &SortSpec::default()).into_string();
        assert!(html.contains("No questions yet."));
    }

    #[test]
    fn list_renders_sortable_headers_for_code_and_answer_type() {
        let html = list(
            &[Row {
                id: ID1,
                code: "x",
                prompt: "x",
                answer_type: "string",
            }],
            &SortSpec::default(),
        )
        .into_string();
        assert!(html.contains("href=\"/portal/admin/questions?sort=code\""));
        assert!(html.contains("href=\"/portal/admin/questions?sort=answer_type\""));
        // Prompt column is intentionally not sortable — sorting by a
        // free-form prompt would be confusing, and a stable sort would
        // anchor on code anyway.
    }

    #[test]
    fn list_active_sort_descending_arrow_renders() {
        let html = list(
            &[Row {
                id: ID1,
                code: "x",
                prompt: "x",
                answer_type: "string",
            }],
            &SortSpec::single("code", SortDirection::Descending),
        )
        .into_string();
        assert!(html.contains("↓"));
    }
}
