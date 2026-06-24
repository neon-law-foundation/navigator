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
