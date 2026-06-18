//! Reusable "list rows in a table" admin view for resources that
//! don't need full CRUD yet — read-only listings of audit /
//! provenance / system tables. Each row is a list of (label-less)
//! strings that match the column order.
//!
//! Two flavors:
//!
//! - [`render`] (and [`ListPage`]) renders a plain unsorted table.
//!   Use for tiny listings (jurisdictions, entity-types) where
//!   sorting and pagination are not worth wiring.
//! - [`render_sortable`] (and [`SortableListPage`]) renders the
//!   [`crate::components::data_table::data_table`] component plus
//!   [`crate::components::pagination::pagination`] underneath, so
//!   the page picks up JSON:API 1.1 sortable headers and prev/next
//!   page controls. The handler is responsible for parsing
//!   `?sort=` and `?page=` and paging the underlying query —
//!   the view is purely presentational.

use maud::{html, Markup};

use crate::components::data_table::{data_table, Column};
use crate::components::pagination::pagination;
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct ListPage<'a> {
    pub title: &'a str,
    pub heading: &'a str,
    pub headers: &'a [&'a str],
    pub rows: Vec<Vec<String>>,
}

/// Sortable, paginated variant. `base_path` is the URL the sort
/// headers + page links target (e.g. `/portal/admin/jurisdictions`);
/// `extra_query` survives every sort click so a `?q=...` filter
/// stays stitched onto the rebuilt URLs.
pub struct SortableListPage<'a> {
    pub title: &'a str,
    pub heading: &'a str,
    pub columns: &'a [Column<'a>],
    pub rows: Vec<Vec<Markup>>,
    pub sort: SortSpec,
    pub base_path: &'a str,
    pub current_page: u32,
    pub total_pages: u32,
    pub empty_message: &'a str,
    pub extra_query: &'a [(&'a str, &'a str)],
}

#[must_use]
pub fn render(p: &ListPage<'_>) -> Markup {
    let body = html! {
        section.admin { div.container {
            h1 { (p.heading) }
            @if p.rows.is_empty() {
                p.empty { "No rows yet." }
            } @else {
                table.admin-table {
                    thead {
                        tr {
                            @for h in p.headers {
                                th { (*h) }
                            }
                        }
                    }
                    tbody {
                        @for row in &p.rows {
                            tr {
                                @for cell in row {
                                    td { (cell) }
                                }
                            }
                        }
                    }
                }
            }
        } }
    };
    PageLayout::new(p.title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Render a load-failure panel for a read-only listing. Used when the
/// underlying query errors: the page shows an explicit failure notice
/// instead of an empty table, so a database error never masquerades as
/// "no rows yet." The error detail itself is logged, not surfaced.
#[must_use]
pub fn render_load_error(title: &str, heading: &str) -> Markup {
    let body = html! {
        section.admin { div.container {
            h1 { (heading) }
            p.error { "Could not load rows. Please retry." }
        } }
    };
    PageLayout::new(title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

/// Render the sortable + paginated variant. Composes
/// [`data_table`] for the table body and [`pagination`] for the
/// prev/next strip. The handler is responsible for honoring the
/// JSON:API `sort` / `page` query parameters before calling.
#[must_use]
pub fn render_sortable(p: &SortableListPage<'_>) -> Markup {
    let body = html! {
        section.admin { div.container {
            h1 { (p.heading) }
            (data_table(
                p.columns,
                &p.rows,
                &p.sort,
                p.base_path,
                p.empty_message,
                p.extra_query,
            ))
            (pagination(p.current_page, p.total_pages, p.base_path))
        } }
    };
    PageLayout::new(p.title)
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, render_load_error, render_sortable, ListPage, SortableListPage};
    use crate::brand::FIRM_BRAND;
    use crate::components::data_table::{text_cell, Column};
    use crate::components::sort_spec::{SortDirection, SortSpec};

    #[test]
    fn empty_rows_shows_empty_state() {
        let html = render(&ListPage {
            title: "X — Admin",
            heading: "X",
            headers: &["A", "B"],
            rows: vec![],
        })
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | X — Admin</title>",
            FIRM_BRAND.site_name
        )));
        assert!(html.contains("No rows yet."));
    }

    #[test]
    fn renders_headers_and_each_row() {
        let html = render(&ListPage {
            title: "Logs",
            heading: "Logs",
            headers: &["ID", "Action"],
            rows: vec![
                vec!["1".into(), "create".into()],
                vec!["2".into(), "update".into()],
            ],
        })
        .into_string();
        assert!(html.contains("<th>ID</th>"));
        assert!(html.contains("<th>Action</th>"));
        assert!(html.contains("<td>1</td>"));
        assert!(html.contains("<td>create</td>"));
        assert!(html.contains("<td>update</td>"));
    }

    #[test]
    fn load_error_shows_failure_notice_not_empty_state() {
        let html = render_load_error("X — Admin", "X").into_string();
        assert!(html.contains(&format!(
            "<title>{} | X — Admin</title>",
            FIRM_BRAND.site_name
        )));
        assert!(html.contains("Could not load rows."));
        // A failed query must never read as a successful empty listing.
        assert!(!html.contains("No rows yet."));
    }

    #[test]
    fn sortable_list_renders_data_table_with_sortable_headers() {
        let columns = [
            Column::sortable("id", "ID"),
            Column::sortable("code", "Code"),
        ];
        let html = render_sortable(&SortableListPage {
            title: "Jurisdictions — Admin",
            heading: "Jurisdictions",
            columns: &columns,
            rows: vec![
                vec![text_cell("1"), text_cell("US-CA")],
                vec![text_cell("2"), text_cell("US-NV")],
            ],
            sort: SortSpec::default(),
            base_path: "/portal/admin/jurisdictions",
            current_page: 1,
            total_pages: 1,
            empty_message: "No rows yet.",
            extra_query: &[],
        })
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | Jurisdictions — Admin</title>",
            FIRM_BRAND.site_name
        )));
        // The sortable header anchor mounts on /portal/admin/jurisdictions
        // with the column key as the active sort target.
        assert!(html.contains("href=\"/portal/admin/jurisdictions?sort=id\""));
        assert!(html.contains("href=\"/portal/admin/jurisdictions?sort=code\""));
        assert!(html.contains("US-CA"));
        assert!(html.contains("US-NV"));
        // Single-page total → no pagination strip rendered.
        assert!(!html.contains("aria-label=\"Pagination\""));
    }

    #[test]
    fn sortable_list_renders_pagination_when_multipage() {
        let columns = [Column::sortable("id", "ID")];
        let html = render_sortable(&SortableListPage {
            title: "Notations — Admin",
            heading: "Notations",
            columns: &columns,
            rows: vec![vec![text_cell("1")]],
            sort: SortSpec::default(),
            base_path: "/portal/admin/notations",
            current_page: 2,
            total_pages: 5,
            empty_message: "No notations.",
            extra_query: &[],
        })
        .into_string();
        assert!(html.contains("aria-label=\"Pagination\""));
        assert!(html.contains("href=\"/portal/admin/notations?page=1\""));
        assert!(html.contains("href=\"/portal/admin/notations?page=3\""));
        assert!(html.contains("Page 2 of 5"));
    }

    #[test]
    fn sortable_list_renders_active_sort_arrow_for_descending() {
        let columns = [Column::sortable("code", "Code")];
        let html = render_sortable(&SortableListPage {
            title: "X",
            heading: "X",
            columns: &columns,
            rows: vec![vec![text_cell("US-CA")]],
            sort: SortSpec::single("code", SortDirection::Descending),
            base_path: "/portal/admin/x",
            current_page: 1,
            total_pages: 1,
            empty_message: "—",
            extra_query: &[],
        })
        .into_string();
        // Active descending → toggling the same header flips to ascending.
        assert!(html.contains("href=\"/portal/admin/x?sort=code\""));
        assert!(html.contains("↓"));
    }

    #[test]
    fn sortable_list_with_empty_rows_shows_empty_message_not_table() {
        let columns = [Column::sortable("id", "ID")];
        let html = render_sortable(&SortableListPage {
            title: "Empty — Admin",
            heading: "Empty",
            columns: &columns,
            rows: vec![],
            sort: SortSpec::default(),
            base_path: "/portal/admin/empty",
            current_page: 1,
            total_pages: 1,
            empty_message: "Nothing here.",
            extra_query: &[],
        })
        .into_string();
        assert!(html.contains("Nothing here."));
        assert!(!html.contains("<table>"));
    }
}
