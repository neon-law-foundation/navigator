//! Sortable HTML table shared across admin list pages.
//!
//! Pages build a `&[Column<'_>]` describing the columns, a
//! `&[Vec<Markup>]` of pre-rendered cells, and pass the current
//! [`SortSpec`] parsed from `?sort=`. Sortable headers are real
//! `<a>` links to the same path with a flipped `?sort=` value, so
//! sorting works without JavaScript and is trivially testable
//! through `axum`'s in-process harness. Renders with Pico-classless
//! markup — the rest of the app does not use Tailwind.

use std::fmt::Write as _;

use maud::{html, Markup, PreEscaped};

use super::sort_spec::SortSpec;

/// One column in a [`data_table`] view.
#[derive(Debug, Clone, Copy)]
pub struct Column<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub sortable: bool,
}

impl<'a> Column<'a> {
    #[must_use]
    pub const fn sortable(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            sortable: true,
        }
    }

    #[must_use]
    pub const fn fixed(key: &'a str, label: &'a str) -> Self {
        Self {
            key,
            label,
            sortable: false,
        }
    }
}

/// Render a sortable table.
///
/// Cells are pre-rendered `Markup` so callers can embed links or
/// inline forms (e.g. delete buttons). `extra_query` survives every
/// sort click — pass `[("q", "needle")]` to keep a search filter
/// stitched onto the rebuilt URL.
///
/// Empty `rows` renders `empty_message` inside a `<p>` instead of an
/// empty table.
#[must_use]
pub fn data_table(
    columns: &[Column<'_>],
    rows: &[Vec<Markup>],
    sort: &SortSpec,
    base_path: &str,
    empty_message: &str,
    extra_query: &[(&str, &str)],
) -> Markup {
    if rows.is_empty() {
        return html! { p.empty-state."text-body-secondary" { (empty_message) } };
    }
    html! {
        // Bootstrap-styled table: striped zebra rows, hover highlight,
        // and `align-middle` so the icon row in the action column lines
        // up with text cells vertically. `table-responsive` wraps the
        // table for horizontal scroll on narrow viewports.
        div."table-responsive" {
            table.table."table-striped"."table-hover"."align-middle" {
                thead {
                    tr {
                        @for column in columns {
                            (render_header(column, sort, base_path, extra_query))
                        }
                    }
                }
                tbody {
                    @for row in rows {
                        tr {
                            @for cell in row {
                                td { (cell) }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_header(
    column: &Column<'_>,
    sort: &SortSpec,
    base_path: &str,
    extra_query: &[(&str, &str)],
) -> Markup {
    if !column.sortable {
        return html! { th data-column-key=(column.key) { (column.label) } };
    }
    let next = sort.toggling(column.key);
    let href = sort_href(base_path, &next.encoded(), extra_query);
    html! {
        th data-column-key=(column.key) {
            a href=(href) {
                (column.label)
                @if let Some(d) = sort.direction_for(column.key) {
                    " "
                    span.sort-arrow { (d.arrow()) }
                }
            }
        }
    }
}

fn sort_href(base_path: &str, encoded_sort: &str, extra_query: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(&str, &str)> = extra_query
        .iter()
        .copied()
        .filter(|(_, v)| !v.is_empty())
        .collect();
    pairs.sort_by_key(|(k, _)| *k);
    let mut parts: Vec<String> = pairs
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, urlencode(v)))
        .collect();
    parts.push(format!("sort={}", urlencode(encoded_sort)));
    format!("{base_path}?{}", parts.join("&"))
}

/// Minimal `application/x-www-form-urlencoded` value encoder — enough
/// to handle the characters that legitimately appear in sort keys
/// and search needles (spaces, ampersands, equals signs, `+`, `#`).
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let safe = matches!(byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' | b',');
        if safe {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Convenience for embedding plain text in a cell while preserving
/// maud's automatic escaping.
#[must_use]
pub fn text_cell(value: &str) -> Markup {
    html! { (value) }
}

/// Raw HTML cell. Use sparingly — pages should prefer maud's `html!`
/// macro and pass the resulting `Markup` directly. This exists so
/// callers that already hold a sanitized snippet (rendered Markdown,
/// for example) don't have to round-trip through a string.
#[must_use]
pub fn raw_cell(html_fragment: &str) -> Markup {
    PreEscaped(html_fragment.to_string())
}

#[cfg(test)]
mod tests {
    use super::{data_table, raw_cell, text_cell, Column};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use maud::html;

    #[test]
    fn empty_rows_render_the_empty_message() {
        let columns = [Column::sortable("name", "Name")];
        let html = data_table(
            &columns,
            &[],
            &SortSpec::default(),
            "/portal/admin/people",
            "No people yet.",
            &[],
        )
        .into_string();
        assert!(html.contains("No people yet."));
        assert!(
            !html.contains("<table"),
            "no table element should render when rows are empty: {html}",
        );
    }

    #[test]
    fn rendered_table_carries_bootstrap_classes() {
        let columns = [Column::sortable("name", "Name")];
        let rows = vec![vec![text_cell("Libra")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(
            html.contains("class=\"table-responsive\""),
            "table should be wrapped in a responsive scroller, got: {html}",
        );
        assert!(
            html.contains("class=\"table table-striped table-hover align-middle\""),
            "table missing Bootstrap class set, got: {html}",
        );
    }

    #[test]
    fn renders_header_label_for_each_column() {
        let columns = [
            Column::sortable("name", "Name"),
            Column::sortable("email", "Email"),
        ];
        let rows = vec![vec![text_cell("Libra"), text_cell("libra@example.com")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(html.contains(">Name</a>"), "header label missing: {html}");
        assert!(html.contains(">Email</a>"), "header label missing: {html}");
    }

    #[test]
    fn sortable_header_links_to_ascending_when_inactive() {
        let columns = [Column::sortable("name", "Name")];
        let rows = vec![vec![text_cell("Libra")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(
            html.contains("href=\"/portal/admin/people?sort=name\""),
            "expected ascending sort link, got: {html}",
        );
    }

    #[test]
    fn sortable_header_flips_direction_when_active() {
        let columns = [Column::sortable("name", "Name")];
        let rows = vec![vec![text_cell("Libra")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::single("name", SortDirection::Ascending),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(
            html.contains("href=\"/portal/admin/people?sort=-name\""),
            "expected descending sort link, got: {html}",
        );
        assert!(
            html.contains("↑"),
            "expected active ascending arrow: {html}"
        );
    }

    #[test]
    fn fixed_header_renders_label_without_link() {
        let columns = [Column::fixed("actions", "Actions")];
        let rows = vec![vec![raw_cell(
            "<a href=\"/portal/admin/people/1/edit\">Edit</a>",
        )]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(html.contains("<th data-column-key=\"actions\">Actions</th>"));
        // The cell HTML survives verbatim (raw_cell uses PreEscaped):
        assert!(html.contains("<a href=\"/portal/admin/people/1/edit\">Edit</a>"));
    }

    #[test]
    fn extra_query_round_trips_through_sort_links_in_sorted_order() {
        let columns = [Column::sortable("name", "Name")];
        let rows = vec![vec![text_cell("Libra")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[("z_filter", "active"), ("q", "libra capricorn")],
        )
        .into_string();
        // maud entity-escapes the `&` separators inside the attribute
        // value — that's correct HTML, and the browser sees the raw
        // ampersand on the wire.
        assert!(
            html.contains(
                "href=\"/portal/admin/people?q=libra%20capricorn&amp;z_filter=active&amp;sort=name\""
            ),
            "expected alphabetized + url-encoded extra query, got: {html}",
        );
    }

    #[test]
    fn empty_extra_query_value_is_dropped_from_links() {
        let columns = [Column::sortable("name", "Name")];
        let rows = vec![vec![text_cell("Libra")]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[("q", "")],
        )
        .into_string();
        assert!(html.contains("href=\"/portal/admin/people?sort=name\""));
        assert!(!html.contains("q="), "empty query value leaked: {html}");
    }

    #[test]
    fn cells_can_embed_markup_links() {
        let columns = [Column::fixed("actions", "Actions")];
        let rows = vec![vec![html! { a href="/portal/admin/people/1" { "Open" } }]];
        let html = data_table(
            &columns,
            &rows,
            &SortSpec::default(),
            "/portal/admin/people",
            "No people.",
            &[],
        )
        .into_string();
        assert!(html.contains("<a href=\"/portal/admin/people/1\">Open</a>"));
    }
}
