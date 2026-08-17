//! Sortable data table as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::data_table`, and the reference
//! implementation of the epic's **URL contract**: table state lives in the
//! query string, not in wasm-only signals. Sortable headers are real `<a>`
//! anchors to the same path with a flipped `?sort=` value (JSON:API 1.1), so
//! sorting works pre-hydration and for crawlers, and deep links / back-forward
//! keep working. The header state is [`SortState`] — the render-side subset of
//! `views::components::SortSpec` (parse / encode / toggle / direction). The
//! `400`-on-unadvertised-field half of the contract stays server-side, enforced
//! by the route pre-handler through `views::SortSpec::validated`.
//!
//! `DataTable` renders the `<thead>` from the columns and wraps the caller's
//! `<tr>` rows (`children`) in a `<tbody>`, so a page maps its data rows to
//! `tr { td { … } }`, while the sortable header anchors are generated here
//! once.

use std::fmt::Write as _;

use dioxus::prelude::*;

/// A sort direction. Mirrors `views::components::SortDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

impl Direction {
    /// The BMP arrow glyph rendered in the active sortable header.
    #[must_use]
    pub const fn arrow(self) -> &'static str {
        match self {
            Self::Ascending => "\u{2191}",
            Self::Descending => "\u{2193}",
        }
    }

    #[must_use]
    const fn flipped(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// The render-side sort state parsed from `?sort=`. The JSON:API `sort`
/// contract: comma-separated keys, a leading `-` is descending. This carries
/// only what the header needs — parse, re-encode, direction lookup, and toggle;
/// server-side validation lives in `views::SortSpec::validated`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortState {
    fields: Vec<(String, Direction)>,
}

impl SortState {
    /// A single-field sort — the shape a header click always produces.
    #[must_use]
    pub fn single(key: impl Into<String>, direction: Direction) -> Self {
        Self {
            fields: vec![(key.into(), direction)],
        }
    }

    /// Parse a raw `?sort=` value: comma-separated, leading `-` descending,
    /// empty / lone-`-` segments dropped. Round-trips with [`Self::encoded`].
    #[must_use]
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return Self::default();
        };
        let fields = raw
            .split(',')
            .filter_map(|part| {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    return None;
                }
                match trimmed.strip_prefix('-') {
                    Some("") => None,
                    Some(key) => Some((key.to_string(), Direction::Descending)),
                    None => Some((trimmed.to_string(), Direction::Ascending)),
                }
            })
            .collect();
        Self { fields }
    }

    /// Re-encode into a JSON:API `sort=` value. Empty state → empty string.
    #[must_use]
    pub fn encoded(&self) -> String {
        self.fields
            .iter()
            .map(|(key, direction)| match direction {
                Direction::Descending => format!("-{key}"),
                Direction::Ascending => key.clone(),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The direction this state sorts `key`, if at all.
    #[must_use]
    pub fn direction_for(&self, key: &str) -> Option<Direction> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, d)| *d)
    }

    /// The state produced by clicking `key`'s header: flip if already active,
    /// else a fresh ascending single-field sort (multi-field is out of scope).
    #[must_use]
    pub fn toggling(&self, key: &str) -> Self {
        let direction = self
            .direction_for(key)
            .map_or(Direction::Ascending, Direction::flipped);
        Self::single(key, direction)
    }
}

/// One column in a [`DataTable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub key: String,
    pub label: String,
    pub sortable: bool,
}

impl Column {
    /// A sortable column — its header is a `?sort=`-toggling anchor.
    #[must_use]
    pub fn sortable(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            sortable: true,
        }
    }

    /// A fixed column (e.g. an actions column) — a plain header, no anchor.
    #[must_use]
    pub fn fixed(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            sortable: false,
        }
    }
}

/// Build the `?sort=`-toggling href for a header, preserving `extra_query`
/// (filters, page) in alphabetized, url-encoded order so a sort click never
/// drops the rest of the table state.
fn sort_href(base_path: &str, encoded_sort: &str, extra_query: &[(String, String)]) -> String {
    let mut pairs: Vec<(&str, &str)> = extra_query
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    pairs.sort_by_key(|(k, _)| *k);
    let mut parts: Vec<String> = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect();
    parts.push(format!("sort={}", urlencode(encoded_sort)));
    format!("{base_path}?{}", parts.join("&"))
}

/// Minimal `application/x-www-form-urlencoded` value encoder — enough for the
/// characters that appear in sort keys and search needles.
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

/// A sortable table. The `<thead>` is generated from `columns` (sortable ones
/// become `?sort=`-toggling anchors carrying `extra_query`); `children` are the
/// `<tr>` body rows the caller supplies. Styled by the Dioxus Components theme
/// (`.nav-table`), horizontally scrollable on narrow viewports.
#[component]
pub fn DataTable(
    columns: Vec<Column>,
    sort: SortState,
    base_path: String,
    #[props(default)] extra_query: Vec<(String, String)>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "nav-table-wrap",
            table { class: "nav-table",
                thead {
                    tr {
                        for column in columns.iter() {
                            {header_cell(column, &sort, &base_path, &extra_query)}
                        }
                    }
                }
                tbody { {children} }
            }
        }
    }
}

/// One `<th>`: a `?sort=`-toggling anchor for a sortable column (with the active
/// direction arrow), or a plain label for a fixed one.
fn header_cell(
    column: &Column,
    sort: &SortState,
    base_path: &str,
    extra_query: &[(String, String)],
) -> Element {
    if !column.sortable {
        return rsx! {
            th { "data-column-key": "{column.key}", "{column.label}" }
        };
    }
    let href = sort_href(
        base_path,
        &sort.toggling(&column.key).encoded(),
        extra_query,
    );
    let active = sort.direction_for(&column.key);
    rsx! {
        th { "data-column-key": "{column.key}",
            a { href: "{href}",
                "{column.label}"
                if let Some(direction) = active {
                    " "
                    span { class: "nav-sort-arrow", "{direction.arrow()}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn parse_encode_round_trips() {
        assert_eq!(
            SortState::parse(Some("name,-created_at")).encoded(),
            "name,-created_at"
        );
        assert_eq!(SortState::parse(Some("name, ,-,-x")).encoded(), "name,-x");
        assert_eq!(SortState::default().encoded(), "");
    }

    #[test]
    fn toggling_flips_active_and_resets_others() {
        assert_eq!(SortState::default().toggling("name").encoded(), "name");
        assert_eq!(
            SortState::single("name", Direction::Ascending)
                .toggling("name")
                .encoded(),
            "-name"
        );
        assert_eq!(
            SortState::single("name", Direction::Descending)
                .toggling("email")
                .encoded(),
            "email"
        );
    }

    #[test]
    fn inactive_sortable_header_links_ascending() {
        fn app() -> Element {
            rsx! {
                DataTable {
                    columns: vec![Column::sortable("name", "Name")],
                    sort: SortState::default(),
                    base_path: "/preview/table".to_string(),
                    tr { td { "Aries" } }
                }
            }
        }
        let html = ssr(app);
        assert!(
            html.contains(r#"href="/preview/table?sort=name""#),
            "{html}"
        );
        assert!(html.contains("nav-table"), "{html}");
    }

    #[test]
    fn active_header_flips_to_descending_and_shows_arrow() {
        fn app() -> Element {
            rsx! {
                DataTable {
                    columns: vec![Column::sortable("name", "Name")],
                    sort: SortState::single("name", Direction::Ascending),
                    base_path: "/preview/table".to_string(),
                    tr { td { "Aries" } }
                }
            }
        }
        let html = ssr(app);
        assert!(
            html.contains(r#"href="/preview/table?sort=-name""#),
            "{html}"
        );
        assert!(html.contains("\u{2191}"), "active ascending arrow: {html}");
    }

    #[test]
    fn extra_query_is_preserved_alphabetized_on_sort_links() {
        fn app() -> Element {
            rsx! {
                DataTable {
                    columns: vec![Column::sortable("name", "Name")],
                    sort: SortState::default(),
                    base_path: "/preview/table".to_string(),
                    extra_query: vec![
                        ("z_filter".to_string(), "active".to_string()),
                        ("q".to_string(), "libra capricorn".to_string()),
                    ],
                    tr { td { "Aries" } }
                }
            }
        }
        let html = ssr(app);
        // Dioxus SSR entity-escapes the `&` separators as `&#38;` (used
        // `&amp;`); both decode to a raw `&` on the wire. Assert the encoded
        // form and the alphabetized, url-encoded order.
        assert!(
            html.contains(
                r#"href="/preview/table?q=libra%20capricorn&#38;z_filter=active&#38;sort=name""#
            ),
            "{html}"
        );
    }

    #[test]
    fn fixed_header_has_no_anchor() {
        fn app() -> Element {
            rsx! {
                DataTable {
                    columns: vec![Column::fixed("actions", "Actions")],
                    sort: SortState::default(),
                    base_path: "/preview/table".to_string(),
                    tr { td { "x" } }
                }
            }
        }
        let html = ssr(app);
        assert!(html.contains(r#"data-column-key="actions""#), "{html}");
        // The label renders, but not inside an anchor.
        assert!(
            !html.contains(r#"<a href="/preview/table?sort=actions"#),
            "{html}"
        );
    }
}
