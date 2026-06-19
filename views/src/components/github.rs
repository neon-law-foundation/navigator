//! GitHub call-to-action components.

use maud::{html, Markup};

use super::ExternalLink;

/// Render the footer button that invites visitors to star the public
/// Navigator repository. It is deliberately static HTML instead of the
/// third-party GitHub Buttons script, so CSP and privacy stay simple.
#[must_use]
pub fn github_star_button(repo_url: &str, label: &str) -> Markup {
    ExternalLink::new(repo_url)
        .with_class("btn btn-outline-secondary btn-sm d-inline-flex align-items-center gap-2")
        .render(html! {
            i.bi."bi-star-fill" aria-hidden="true" {}
            span { (label) }
        })
}

#[cfg(test)]
mod tests {
    use super::github_star_button;

    #[test]
    fn github_star_button_links_to_repo_safely() {
        let html = github_star_button(
            "https://github.com/example/repo",
            "Star Navigator on GitHub",
        )
        .into_string();
        assert!(
            html.contains("href=\"https://github.com/example/repo\""),
            "{html}"
        );
        assert!(html.contains("target=\"_blank\""), "{html}");
        assert!(html.contains("rel=\"noopener noreferrer\""), "{html}");
    }

    #[test]
    fn github_star_button_renders_star_and_label() {
        let html = github_star_button(
            "https://github.com/example/repo",
            "Star Navigator on GitHub",
        )
        .into_string();
        assert!(html.contains("bi-star-fill"), "{html}");
        assert!(html.contains("aria-hidden=\"true\""), "{html}");
        assert!(html.contains(">Star Navigator on GitHub</span>"), "{html}");
    }
}
