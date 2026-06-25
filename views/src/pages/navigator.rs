//! `/navigator` — the repository README, rendered on the site.
//!
//! Foundation-branded. The body is the workspace `README.md`, baked in at
//! compile time with `include_str!`, so `neonlaw.com/navigator` is a
//! byte-for-byte copy of the project's front page. Repo-relative links —
//! written so they resolve for a git reader — are retargeted by
//! [`rewrite_link`] so they also resolve on the web:
//!
//! - a top-level `docs/<name>.md` → the published `/docs/<name>` route;
//! - a `notation_templates/**/*.md` link → the raw `/api/templates/**`
//!   endpoint (`web::template_api`);
//! - every other repo-relative path (nested docs, `LICENSE-*`) → the
//!   GitHub source, so no link dead-ends;
//! - absolute URLs and same-page anchors pass through untouched.

use maud::{html, Markup};

use crate::brand::FOUNDATION_BRAND;
use crate::markdown::render_with_link_rewrite;
use crate::{AuthState, PageLayout};

/// The repository README, baked in at compile time — the page is an exact
/// copy of this file. Resolved against the `views` crate manifest dir, so
/// it points at the workspace-root `README.md`.
const README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md"));

/// GitHub blob root for repo-relative links with no on-site route.
const REPO_BLOB_BASE: &str = "https://github.com/neon-law-foundation/Navigator/blob/main/";

#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let body = html! {
        // The hub fans out to the per-package pages (LSP / CLI / MCP /
        // Web) before the long-form README overview.
        (crate::pages::package::package_strip(None))
        article.docs-article {
            (render_with_link_rewrite(README, rewrite_link))
        }
    };
    PageLayout::new("Neon Law Navigator")
        .with_description(
            "Neon Law Navigator — open source legal software from the Neon Law \
             Foundation that helps lawyers finish more legal projects.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// Retarget one README link so it resolves on the website. See the module
/// docs for the mapping. `pub(crate)` so the per-package pages
/// ([`crate::pages::package`]) reuse the exact same retargeting.
pub(crate) fn rewrite_link(dest: &str) -> String {
    // Absolute URLs, mailto, and same-page anchors already resolve.
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
    // A top-level `docs/<name>.md` is a published doc; nested docs
    // (`docs/lsp/README.md`) are not, so they fall through to GitHub.
    if let Some(stem) = path
        .strip_prefix("docs/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        if !stem.contains('/') {
            // URLs are kebab-case; the file stem keeps its underscores.
            return with_anchor(&format!("/docs/{}", crate::slug::to_url(stem)), anchor);
        }
    }
    // A `notation_templates/**/*.md` link maps to the raw template API.
    if let Some(stem) = path
        .strip_prefix("notation_templates/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        return with_anchor(
            &format!("/api/templates/{}", crate::slug::to_url(stem)),
            anchor,
        );
    }
    // Everything else repo-relative resolves against the GitHub source.
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
    use crate::brand::FOUNDATION_BRAND;
    use crate::AuthState;

    #[test]
    fn navigator_renders_the_readme_under_foundation_brand() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Neon Law Navigator</title>",
            FOUNDATION_BRAND.site_name
        )));
        // The README's own H1 is the page heading — proof it's the README.
        // (Headings carry a stamped slug id, so match on the text + close.)
        assert!(
            html.contains(">Neon Law Navigator</h1>"),
            "expected the README heading on the page: {html}"
        );
    }

    #[test]
    fn navigator_is_an_exact_copy_of_the_readme() {
        // The baked constant is literally the repository README; the
        // include path must resolve to the workspace-root file.
        assert!(README.starts_with("# Neon Law Navigator"));
        assert!(README.contains("## License"));
    }

    #[test]
    fn top_level_doc_links_map_to_site_routes() {
        assert_eq!(
            rewrite_link("docs/glossary.md#project"),
            "/docs/glossary#project"
        );
        assert_eq!(
            rewrite_link("docs/notation.md#templates"),
            "/docs/notation#templates"
        );
        assert_eq!(rewrite_link("docs/glossary.md"), "/docs/glossary");
        // An underscore doc filename is rewritten to its kebab-case URL.
        assert_eq!(
            rewrite_link("docs/retainer_intake.md"),
            "/docs/retainer-intake"
        );
    }

    #[test]
    fn template_links_map_to_the_raw_api() {
        assert_eq!(
            rewrite_link("notation_templates/united_states/nevada/state/business_associations/entity_formation.md"),
            "/api/templates/united-states/nevada/state/business-associations/entity-formation"
        );
        // Underscores in either segment become hyphens in the URL.
        assert_eq!(
            rewrite_link(
                "notation_templates/united_states/federal/irs/taxation/form990_annual_report.md"
            ),
            "/api/templates/united-states/federal/irs/taxation/form990-annual-report"
        );
        assert_eq!(
            rewrite_link("notation_templates/united_states/nevada/state/business_associations/annual_report.md"),
            "/api/templates/united-states/nevada/state/business-associations/annual-report"
        );
    }

    #[test]
    fn nested_and_other_relative_links_point_at_github() {
        // Nested docs aren't published at `/docs/:slug`.
        assert_eq!(
            rewrite_link("docs/lsp/README.md"),
            "https://github.com/neon-law-foundation/Navigator/blob/main/docs/lsp/README.md"
        );
        // License files live in the repo root, served from GitHub.
        assert_eq!(
            rewrite_link("LICENSE-APACHE"),
            "https://github.com/neon-law-foundation/Navigator/blob/main/LICENSE-APACHE"
        );
    }

    #[test]
    fn absolute_and_anchor_links_pass_through() {
        assert_eq!(
            rewrite_link("https://www.neonlaw.com/foundation/mission"),
            "https://www.neonlaw.com/foundation/mission"
        );
        assert_eq!(rewrite_link("#trademarks"), "#trademarks");
        assert_eq!(rewrite_link("https://zed.dev"), "https://zed.dev");
    }

    #[test]
    fn rendered_page_carries_rewritten_hrefs() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(
            html.contains("href=\"/docs/glossary#project\""),
            "glossary link should resolve to the site route"
        );
        assert!(
            html.contains(
                "href=\"/api/templates/united-states/nevada/state/business-associations/entity-formation\""
            ),
            "template link should resolve to the raw API"
        );
        assert!(
            html.contains(
                "href=\"https://github.com/neon-law-foundation/Navigator/blob/main/LICENSE-APACHE\""
            ),
            "license link should resolve to the GitHub source"
        );
        // The in-page Trademarks anchor needs a matching heading id.
        assert!(html.contains("id=\"trademarks\""));
    }

    #[test]
    fn navigator_description_meta_is_emitted() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("name=\"description\""));
        assert!(html.contains("open source legal software"));
    }
}
