//! GitHub call-to-action components.

use maud::{html, Markup};

use super::ExternalLink;

/// Render the footer button that invites visitors to star the public
/// Neon Law Navigator repository. It is first-party HTML enhanced by
/// `/public/js/github-stars.js`, not the third-party GitHub Buttons
/// script, so CSP and privacy stay simple.
#[must_use]
pub fn github_star_button(repo_url: &str, label: &str) -> Markup {
    ExternalLink::new(repo_url)
        .with_title(label)
        .with_class("btn btn-outline-secondary btn-sm d-inline-flex align-items-center gap-2")
        .render(html! {
            i.bi."bi-star-fill" aria-hidden="true" {}
            span data-github-star-label { (label) }
            span.badge."text-bg-secondary"
                data-github-star-count
                aria-label="GitHub stars"
                hidden
            {}
        })
}

#[cfg(test)]
mod tests {
    use super::github_star_button;

    #[test]
    fn github_star_button_links_to_repo_safely() {
        let html = github_star_button("https://github.com/example/repo", "Star Neon Law Navigator")
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
        let html = github_star_button("https://github.com/example/repo", "Star Neon Law Navigator")
            .into_string();
        assert!(html.contains("bi-star-fill"), "{html}");
        assert!(html.contains("aria-hidden=\"true\""), "{html}");
        assert!(html.contains(">Star Neon Law Navigator</span>"), "{html}");
        assert!(html.contains("data-github-star-count"), "{html}");
        assert!(html.contains("hidden"), "{html}");
    }
}
