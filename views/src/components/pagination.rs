//! Prev / next pagination control for paged index views (admin
//! lists). 1-indexed; the boundary pages render the inactive
//! side as a non-link `aria-disabled` span so the strip is still
//! readable but unreachable. Intentionally generic so admin lists
//! can adopt it without changes.

use maud::{html, Markup};

/// Render a `<nav>` strip with previous/next links and a centered
/// "Page X of Y" label. Returns an empty fragment when `total <= 1`
/// — there is no useful pagination control for a single-page list.
///
/// Renders Bootstrap pagination chrome: `nav` wrapping a `ul.pagination`
/// with `li.page-item.page-link` children. Disabled boundary items
/// keep their `.disabled` class so the visual + accessibility states
/// line up with what Bootstrap CSS expects. The "Page X of Y" label
/// rides as a `li.page-item.disabled` so it sits in the same row as
/// the active controls.
#[must_use]
pub fn pagination(current: u32, total: u32, base_path: &str) -> Markup {
    if total <= 1 {
        return html! {};
    }
    let current = current.max(1).min(total);
    html! {
        nav aria-label="Pagination" {
            ul.pagination."justify-content-center" {
                @if current > 1 {
                    li."page-item" {
                        a."page-link" href=(format!("{base_path}?page={}", current - 1)) {
                            "Previous"
                        }
                    }
                } @else {
                    li."page-item"."disabled" aria-disabled="true" {
                        span."page-link" { "Previous" }
                    }
                }
                li."page-item"."disabled" aria-current="page" {
                    span."page-link" { "Page " (current) " of " (total) }
                }
                @if current < total {
                    li."page-item" {
                        a."page-link" href=(format!("{base_path}?page={}", current + 1)) {
                            "Next"
                        }
                    }
                } @else {
                    li."page-item"."disabled" aria-disabled="true" {
                        span."page-link" { "Next" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pagination;

    #[test]
    fn single_page_renders_empty_fragment() {
        let html = pagination(1, 1, "/blog").into_string();
        assert!(html.is_empty() || !html.contains("<nav"));
    }

    #[test]
    fn zero_pages_renders_empty_fragment() {
        let html = pagination(0, 0, "/blog").into_string();
        assert!(!html.contains("<nav"));
    }

    #[test]
    fn first_page_disables_previous() {
        let html = pagination(1, 3, "/blog").into_string();
        assert!(html.contains("aria-disabled=\"true\""));
        assert!(
            html.contains(">Previous</span>"),
            "Previous should render as a span inside the disabled .page-item: {html}",
        );
        assert!(html.contains("href=\"/blog?page=2\""));
    }

    #[test]
    fn last_page_disables_next() {
        let html = pagination(3, 3, "/blog").into_string();
        assert!(html.contains("href=\"/blog?page=2\""));
        assert!(html.contains(">Next</span>"));
    }

    #[test]
    fn wears_bootstrap_pagination_chrome() {
        let html = pagination(2, 4, "/blog").into_string();
        assert!(
            html.contains("class=\"pagination justify-content-center\""),
            "expected Bootstrap .pagination on the <ul>, got: {html}",
        );
        assert!(
            html.contains("class=\"page-item\""),
            "expected .page-item on active controls, got: {html}",
        );
        assert!(
            html.contains("class=\"page-link\""),
            "expected .page-link on inner anchor/span, got: {html}",
        );
    }

    #[test]
    fn disabled_boundary_items_carry_disabled_class() {
        // Bootstrap's .disabled paints the inactive Previous/Next
        // controls grey + non-clickable. Both the boundary item and
        // the "Page X of Y" status get .disabled because none of them
        // should look like an active link.
        let html = pagination(1, 3, "/blog").into_string();
        assert!(
            html.contains("class=\"page-item disabled\""),
            "boundary item missing .disabled, got: {html}",
        );
    }

    #[test]
    fn middle_page_has_both_active_links() {
        let html = pagination(2, 4, "/blog").into_string();
        assert!(html.contains("href=\"/blog?page=1\""));
        assert!(html.contains("href=\"/blog?page=3\""));
    }

    #[test]
    fn status_renders_current_and_total() {
        let html = pagination(2, 7, "/portal/admin/people").into_string();
        assert!(html.contains("Page 2 of 7"));
    }

    #[test]
    fn current_above_total_clamps_to_last_page() {
        let html = pagination(99, 3, "/blog").into_string();
        assert!(html.contains("Page 3 of 3"));
        // Clamped to last page → Next is disabled, Previous active.
        assert!(html.contains("href=\"/blog?page=2\""));
        assert!(html.contains(">Next</span>"));
    }

    #[test]
    fn aria_label_is_present_for_screen_readers() {
        let html = pagination(1, 2, "/blog").into_string();
        assert!(html.contains("aria-label=\"Pagination\""));
    }
}
