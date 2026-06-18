//! Admin `/portal/admin/entities/:id/cap-table` page.
//!
//! Shows the ownership breakdown for one entity: every holder, the
//! shares they were issued, and their resulting percentage of the
//! total outstanding. The handler aggregates `share_issuances` rows
//! by holder name before rendering; this view only displays.
//!
//! Sortable headers let staff flip the table by holder, shares, or
//! percentage — useful when the cap table grows past a screen and
//! they need to spot the largest holder fast.

use maud::{html, Markup};
use uuid::Uuid;

use crate::components::data_table::{data_table, Column};
use crate::components::sort_spec::SortSpec;
use crate::PageLayout;

pub struct CapTableRow<'a> {
    pub holder_name: &'a str,
    pub shares: i64,
    /// Percentage of the entity's outstanding shares, rounded to
    /// two decimals. Caller computes; the view formats.
    pub percent: f64,
}

pub struct CapTablePage<'a> {
    pub entity_id: Uuid,
    pub entity_name: &'a str,
    pub total_shares: i64,
    pub rows: &'a [CapTableRow<'a>],
    pub sort: SortSpec,
}

#[must_use]
pub fn render(p: &CapTablePage<'_>) -> Markup {
    let base_path = format!("/portal/admin/entities/{}/cap-table", p.entity_id);
    let columns = [
        Column::sortable("holder_name", "Holder"),
        Column::sortable("shares", "Shares"),
        Column::sortable("percent", "% outstanding"),
    ];
    let table_rows: Vec<Vec<Markup>> = p
        .rows
        .iter()
        .map(|r| {
            vec![
                html! { (r.holder_name) },
                html! { (r.shares) },
                html! { (format!("{:.2}%", r.percent)) },
            ]
        })
        .collect();
    let body = html! {
        section.admin { div.container {
            header.page-header {
                h1 { "Cap table — " (p.entity_name) }
                p { a href={ "/portal/admin/entities/" (p.entity_id) } { "← Back to entity" } }
            }
            @if p.rows.is_empty() {
                p.empty {
                    "No share issuances recorded for this entity yet."
                }
            } @else {
                p.muted {
                    "Total outstanding: " strong { (p.total_shares) } " shares "
                    "across " (p.rows.len()) " holder(s)."
                }
                (data_table(
                    &columns,
                    &table_rows,
                    &p.sort,
                    &base_path,
                    "No share issuances recorded for this entity yet.",
                    &[],
                ))
            }
        } }
    };
    PageLayout::new(&format!("Cap table — {} — Admin", p.entity_name))
        .with_auth(crate::AuthState::Authenticated)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{render, CapTablePage, CapTableRow};
    use crate::components::sort_spec::{SortDirection, SortSpec};
    use uuid::Uuid;

    const ID1: Uuid = Uuid::from_u128(1);
    const ID2: Uuid = Uuid::from_u128(2);
    const ID42: Uuid = Uuid::from_u128(42);

    #[test]
    fn empty_cap_table_shows_friendly_message() {
        let html = render(&CapTablePage {
            entity_id: ID1,
            entity_name: "Acme Co",
            total_shares: 0,
            rows: &[],
            sort: SortSpec::default(),
        })
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | Cap table — Acme Co — Admin</title>",
            crate::brand::FIRM_BRAND.site_name
        )));
        assert!(html.contains("No share issuances recorded"));
        assert!(html.contains(&format!("href=\"/portal/admin/entities/{ID1}\"")));
    }

    #[test]
    fn populated_cap_table_lists_holders_and_percentages() {
        let rows = [
            CapTableRow {
                holder_name: "Aries",
                shares: 600,
                percent: 60.0,
            },
            CapTableRow {
                holder_name: "Taurus",
                shares: 400,
                percent: 40.0,
            },
        ];
        let html = render(&CapTablePage {
            entity_id: ID2,
            entity_name: "Foo Inc",
            total_shares: 1000,
            rows: &rows,
            sort: SortSpec::default(),
        })
        .into_string();
        assert!(html.contains("Cap table — Foo Inc"));
        assert!(html.contains("Aries"));
        assert!(html.contains("Taurus"));
        assert!(html.contains("60.00%"));
        assert!(html.contains("40.00%"));
        assert!(html.contains("1000"));
        assert!(html.contains("2 holder(s)"));
    }

    #[test]
    fn back_link_targets_the_entity_detail_page() {
        let html = render(&CapTablePage {
            entity_id: ID42,
            entity_name: "X Corp",
            total_shares: 0,
            rows: &[],
            sort: SortSpec::default(),
        })
        .into_string();
        assert!(html.contains(&format!("href=\"/portal/admin/entities/{ID42}\"")));
    }

    #[test]
    fn sortable_headers_target_the_cap_table_route() {
        let rows = [CapTableRow {
            holder_name: "Aries",
            shares: 1,
            percent: 100.0,
        }];
        let html = render(&CapTablePage {
            entity_id: ID2,
            entity_name: "Foo",
            total_shares: 1,
            rows: &rows,
            sort: SortSpec::default(),
        })
        .into_string();
        assert!(html.contains(&format!(
            "href=\"/portal/admin/entities/{ID2}/cap-table?sort=holder_name\""
        )));
        assert!(html.contains(&format!(
            "href=\"/portal/admin/entities/{ID2}/cap-table?sort=shares\""
        )));
        assert!(html.contains(&format!(
            "href=\"/portal/admin/entities/{ID2}/cap-table?sort=percent\""
        )));
    }

    #[test]
    fn active_sort_descending_arrow_renders() {
        let rows = [CapTableRow {
            holder_name: "Aries",
            shares: 1,
            percent: 100.0,
        }];
        let html = render(&CapTablePage {
            entity_id: ID2,
            entity_name: "Foo",
            total_shares: 1,
            rows: &rows,
            sort: SortSpec::single("shares", SortDirection::Descending),
        })
        .into_string();
        assert!(html.contains("↓"));
    }
}
