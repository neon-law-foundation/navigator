//! `/foundation/templates` — the Template tree README,
//! rendered on the site under the Foundation brand.
//!
//! The body is `templates/README.md`, baked in at compile time
//! with `include_str!`, so the public page stays tied to the repository
//! instructions for how the template tree is organized and named.

use maud::{html, Markup};

use crate::brand::FOUNDATION_BRAND;
use crate::markdown::render_with_link_rewrite;
use crate::{AuthState, PageLayout};

const README: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../templates/README.md"
));

const REPO_BLOB_BASE: &str =
    "https://github.com/neon-law-foundation/navigator/blob/main/templates/";

#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let body = html! {
        article.docs-article {
            (render_with_link_rewrite(README, rewrite_link))
        }
    };
    PageLayout::new("Templates")
        .with_description(
            "The Neon Law Navigator Template tree: markdown blueprints for legal \
             intake, workflows, and attorney-reviewed documents.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

fn rewrite_link(dest: &str) -> String {
    if dest.starts_with("http://")
        || dest.starts_with("https://")
        || dest.starts_with("mailto:")
        || dest.starts_with('#')
    {
        return dest.to_string();
    }
    let (path, anchor) = match dest.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (dest, None),
    };
    if let Some(stem) = path
        .strip_prefix("../docs/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        if !stem.contains('/') {
            return with_anchor(&format!("/docs/{}", crate::slug::to_url(stem)), anchor);
        }
    }
    if path == "../README.md" {
        return with_anchor("/foundation/navigator", anchor);
    }
    if let Some(stem) = path.strip_suffix(".md") {
        return with_anchor(
            &format!("/api/templates/{}", crate::slug::to_url(stem)),
            anchor,
        );
    }
    with_anchor(&format!("{REPO_BLOB_BASE}{path}"), anchor)
}

fn with_anchor(base: &str, anchor: Option<&str>) -> String {
    match anchor {
        Some(a) => format!("{base}#{a}"),
        None => base.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{render, rewrite_link, README};
    use crate::AuthState;

    #[test]
    fn templates_renders_the_readme_under_foundation_brand() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Neon Law Foundation | Templates</title>"));
        assert!(html.contains(">Templates</h1>"));
        assert!(html.contains("Every template has YAML frontmatter"));
    }

    #[test]
    fn templates_page_is_tied_to_the_readme() {
        assert!(README.starts_with("# Templates"));
        assert!(README.contains("## Naming convention"));
    }

    #[test]
    fn doc_links_map_to_site_routes() {
        assert_eq!(
            rewrite_link("../docs/notation.md#template"),
            "/docs/notation#template"
        );
        assert_eq!(rewrite_link("../docs/glossary.md"), "/docs/glossary");
    }

    #[test]
    fn root_readme_link_maps_to_the_navigator_hub() {
        assert_eq!(
            rewrite_link("../README.md#trademarks"),
            "/foundation/navigator#trademarks"
        );
    }

    #[test]
    fn template_links_map_to_the_raw_api() {
        assert_eq!(
            rewrite_link("forms/united_states/nevada/state/nv__llc_formation.md"),
            "/api/templates/forms/united-states/nevada/state/nv--llc-formation"
        );
        assert_eq!(
            rewrite_link("forms/united_states/nevada/state/nv__annual_report.md"),
            "/api/templates/forms/united-states/nevada/state/nv--annual-report"
        );
    }

    #[test]
    fn other_relative_links_point_at_the_github_source() {
        assert_eq!(
            rewrite_link("forms/united_states/nevada/state/nv__llc_formation.fields.toml"),
            "https://github.com/neon-law-foundation/navigator/blob/main/templates/forms/united_states/nevada/state/nv__llc_formation.fields.toml"
        );
    }
}
