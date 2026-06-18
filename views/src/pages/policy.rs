//! Shared rendering for "policy" style pages — a heading plus a
//! `CommonMark` body. Used by both [`privacy`] and [`terms`] so they
//! stay structurally identical and authors can format the body
//! (lists, links, emphasis) without crossing back into Rust.
//!
//! [`privacy`]: super::privacy
//! [`terms`]: super::terms

use maud::{html, Markup};

use crate::brand::SiteBrand;
use crate::{markdown, AuthState, PageLayout};

/// Render a policy-style page from `title`, description, and a
/// `CommonMark` `body`. `brand` is passed through to the surrounding
/// `PageLayout` so callers can keep their page on the foundation
/// surface even though the helper itself is brand-agnostic.
#[must_use]
pub fn render(
    title: &str,
    description: &str,
    body: &str,
    auth: AuthState,
    brand: SiteBrand,
) -> Markup {
    let rendered = markdown::render(body);
    let body = html! {
        article.policy {
            h1 { (title) }
            (rendered)
        }
    };
    PageLayout::new(title)
        .with_description(description)
        .with_brand(brand)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn policy_renders_each_section() {
        let body = "## First\n\nFirst body.\n\n## Second\n\nSecond body.\n";
        let html = render(
            "Demo Policy",
            "Demo description.",
            body,
            crate::AuthState::Anonymous,
            *crate::brand::FOUNDATION_BRAND,
        )
        .into_string();
        assert!(html.contains("<h2>First</h2>"));
        assert!(html.contains("First body."));
        assert!(html.contains("<h2>Second</h2>"));
        assert!(html.contains("Second body."));
    }

    #[test]
    fn policy_renders_title_with_caller_brand() {
        let html = render(
            "X Policy",
            "Y Desc.",
            "",
            crate::AuthState::Anonymous,
            *crate::brand::FOUNDATION_BRAND,
        )
        .into_string();
        assert!(html.contains(&format!(
            "<title>{} | X Policy</title>",
            crate::brand::FOUNDATION_BRAND.site_name
        )));
        assert!(html.contains("name=\"description\" content=\"Y Desc.\""));
    }
}
