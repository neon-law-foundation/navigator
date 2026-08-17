//! Prev / next pagination as a Dioxus component (issue #641, Phase 2).
//!
//! The successor to the `views::components::pagination`, and part of the
//! URL contract: pagination lives in `?page=`, 1-indexed, rendered as real
//! anchors so it works pre-hydration and for crawlers. The boundary pages
//! render the unreachable side as a non-anchor `aria-disabled` span. Returns
//! nothing when there is a single page (or none). Styled by the Dioxus
//! Components theme (`.nav-pagination`).

use std::fmt::Write as _;

use dioxus::prelude::*;

/// A prev/next strip with a centered "Page X of Y" label. `current` is clamped
/// to `1..=total`. Renders empty when `total <= 1`. `extra_query` (e.g. the
/// active `?sort=`) rides on every page link so paging never drops the rest of
/// the table state.
///
/// `page_param` names the query parameter this pager writes, defaulting to
/// `page`. A page carrying two independent lists (the Nebula show-and-tell
/// archive pages "upcoming" and "past" separately) gives each pager its own
/// parameter and passes the other's current page in `extra_query`, so paging
/// one list never resets the other.
#[component]
pub fn Pagination(
    current: u32,
    total: u32,
    base_path: String,
    #[props(default)] extra_query: Vec<(String, String)>,
    #[props(default)] page_param: Option<String>,
) -> Element {
    if total <= 1 {
        return rsx! {};
    }
    let page_param = page_param.unwrap_or_else(|| "page".to_string());
    let current = current.clamp(1, total);
    let href = |page: u32| page_href(&base_path, page, &extra_query, &page_param);
    rsx! {
        nav { class: "nav-pagination", "aria-label": "Pagination",
            ul { class: "nav-pagination__list",
                if current > 1 {
                    li { class: "nav-pagination__item",
                        a { class: "nav-pagination__link", href: "{href(current - 1)}", "Previous" }
                    }
                } else {
                    li { class: "nav-pagination__item nav-pagination__item--disabled", "aria-disabled": "true",
                        span { class: "nav-pagination__link", "Previous" }
                    }
                }
                li { class: "nav-pagination__item nav-pagination__item--disabled", "aria-current": "page",
                    span { class: "nav-pagination__link", "Page {current} of {total}" }
                }
                if current < total {
                    li { class: "nav-pagination__item",
                        a { class: "nav-pagination__link", href: "{href(current + 1)}", "Next" }
                    }
                } else {
                    li { class: "nav-pagination__item nav-pagination__item--disabled", "aria-disabled": "true",
                        span { class: "nav-pagination__link", "Next" }
                    }
                }
            }
        }
    }
}

/// Build a `?{page_param}=`-carrying href that preserves `extra_query` in
/// alphabetized, url-encoded order — the same URL discipline the data table's
/// sort links use. The page parameter itself is appended last.
fn page_href(
    base_path: &str,
    page: u32,
    extra_query: &[(String, String)],
    page_param: &str,
) -> String {
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
    parts.push(format!("{page_param}={page}"));
    format!("{base_path}?{}", parts.join("&"))
}

/// Minimal url-encoder for query values (same rules as the data table's).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ssr(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn single_page_renders_nothing() {
        fn app() -> Element {
            rsx! { Pagination { current: 1, total: 1, base_path: "/preview/table".to_string() } }
        }
        assert!(!ssr(app).contains("<nav"));
    }

    #[test]
    fn first_page_disables_previous_and_links_next() {
        fn app() -> Element {
            rsx! { Pagination { current: 1, total: 3, base_path: "/preview/table".to_string() } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"aria-disabled="true""#), "{html}");
        assert!(html.contains(r#"href="/preview/table?page=2""#), "{html}");
        assert!(html.contains("Page 1 of 3"), "{html}");
    }

    #[test]
    fn last_page_disables_next_and_links_previous() {
        fn app() -> Element {
            rsx! { Pagination { current: 3, total: 3, base_path: "/preview/table".to_string() } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"href="/preview/table?page=2""#), "{html}");
        // "Next" renders as a span, not an anchor, on the last page.
        assert!(
            html.contains(r#"<span class="nav-pagination__link">Next</span>"#),
            "{html}"
        );
    }

    #[test]
    fn middle_page_links_both_directions() {
        fn app() -> Element {
            rsx! { Pagination { current: 2, total: 4, base_path: "/preview/table".to_string() } }
        }
        let html = ssr(app);
        assert!(html.contains(r#"href="/preview/table?page=1""#), "{html}");
        assert!(html.contains(r#"href="/preview/table?page=3""#), "{html}");
    }

    #[test]
    fn current_above_total_clamps_to_last() {
        fn app() -> Element {
            rsx! { Pagination { current: 99, total: 3, base_path: "/preview/table".to_string() } }
        }
        let html = ssr(app);
        assert!(html.contains("Page 3 of 3"), "{html}");
    }

    #[test]
    fn extra_query_rides_on_every_page_link() {
        fn app() -> Element {
            rsx! {
                Pagination {
                    current: 2,
                    total: 4,
                    base_path: "/preview".to_string(),
                    extra_query: vec![("sort".to_string(), "-name".to_string())],
                }
            }
        }
        let html = ssr(app);
        // The active sort is preserved across prev/next (`&` escapes to `&#38;`).
        assert!(
            html.contains(r#"href="/preview?sort=-name&#38;page=1""#),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/preview?sort=-name&#38;page=3""#),
            "{html}"
        );
    }
}
