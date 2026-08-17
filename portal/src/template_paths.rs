//! Shared path helpers for notation-template routes.

/// Convert a slash-separated template path to its public kebab URL form.
#[must_use]
pub(crate) fn slug_path(path: &str) -> String {
    path.split('/')
        .map(views::slug::to_url)
        .collect::<Vec<_>>()
        .join("/")
}

/// Compare template paths after applying the same kebab normalization URLs use.
#[must_use]
pub(crate) fn kebab_path_eq(a: &str, b: &str) -> bool {
    let a_parts: Vec<&str> = a.split('/').collect();
    let b_parts: Vec<&str> = b.split('/').collect();
    a_parts.len() == b_parts.len()
        && a_parts
            .iter()
            .zip(b_parts)
            .all(|(left, right)| views::slug::to_url(left) == views::slug::to_url(right))
}

#[cfg(test)]
mod tests {
    use super::{kebab_path_eq, slug_path};

    #[test]
    fn slug_path_normalizes_each_segment_without_flattening_the_tree() {
        assert_eq!(
            slug_path("forms/federal/form_990_annual_report"),
            "forms/federal/form-990-annual-report",
        );
    }

    #[test]
    fn kebab_path_eq_compares_url_forms_segment_by_segment() {
        assert!(kebab_path_eq(
            "forms/federal/form_990_annual_report",
            "forms/federal/form-990-annual-report",
        ));
        assert!(!kebab_path_eq(
            "forms/federal/form_990_annual_report",
            "forms/form-990-annual-report",
        ));
        assert!(!kebab_path_eq(
            "forms/federal/form_990_annual_report",
            "forms/state/form-990-annual-report",
        ));
    }
}
