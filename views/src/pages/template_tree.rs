//! `/foundation/notations` — the Notations page.
//!
//! It opens with a services-style neon hero and a short story about what
//! a notation *is*, then renders `templates/README.md` (baked in at
//! compile time with `include_str!`) for the tree-organization detail, so
//! the public page stays tied to the repository instructions. The hero
//! owns the page title, so the README's leading `# Notations` heading is
//! stripped before rendering to avoid a duplicate.

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

/// Drop the leading top-level `# ...` heading line (and the blank lines
/// after it) so the hero band, not the body, carries the page title.
fn strip_leading_h1(md: &str) -> &str {
    match md.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => rest.trim_start_matches('\n'),
        _ => md,
    }
}

#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let body = html! {
        // The services-style neon hero band. `product-hero.css` is linked
        // on every page and is inert without this `.product-hero` element.
        section."product-hero" {
            div."product-hero__bg" aria-hidden="true" {
                div."product-hero__glow" {}
                div."product-hero__grid" {}
                div."product-hero__horizon" {}
                div."product-hero__sweep" {}
            }
            div."product-hero__content" {
                h1."product-hero__title"."display-3"."fw-bold" { "Notations" }
                p."product-hero__tagline"."lead" {
                    "Every Markdown file we publish is checked the moment we type it. A notation is what that "
                    "same checked Markdown becomes when it carries the questions and the workflow of real legal work."
                }
            }
        }
        article.docs-article {
            // The story: ordinary checked Markdown → add a questionnaire and
            // a workflow → an executable, attorney-gated legal instrument.
            section."notations-story" {
                p {
                    "We hold every Markdown file in Neon Law Navigator to the same standard — a "
                    a href="/foundation/navigator" { "language server" }
                    " checks each one as it is written, and underlines what is wrong in red before it is ever "
                    "saved. Our READMEs, our docs, our blog posts: all of them, the same way."
                }
                p {
                    "A "
                    strong { "notation template" }
                    " starts life as one more Markdown file held to that standard. What sets it apart is its "
                    "frontmatter: declare a "
                    code { "questionnaire" }
                    " (the questions a client answers) and a "
                    code { "workflow" }
                    " (the path the document walks, with a mandatory attorney-review step), and the file stops "
                    "being a document about the law and becomes an instrument that "
                    em { "runs" }
                    " it."
                }
                p {
                    "That is the whole idea of a "
                    strong { "notation" }
                    ": the executable form of legal work. The template is the prose a client signs, the "
                    "questionnaire fills it in, and the workflow carries it from intake to attorney review to "
                    "signature — three faces of one checked file. Plain documentation, elevated, and verified the "
                    "entire way down. The pages below show how the tree is organized; the keys are explained, in "
                    "plain English, in "
                    a href="/docs/frontmatter" { "the frontmatter guide" }
                    "."
                }
            }
            (render_with_link_rewrite(strip_leading_h1(README), rewrite_link))
        }
    };
    PageLayout::new("Notations")
        .with_description(
            "Neon Law Navigator notations: the executable markdown form of the firm's \
             legal work — template, questionnaire, and workflow in one file, checked live by the LSP.",
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
    fn notations_render_the_readme_under_foundation_brand() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Neon Law Foundation | Notations</title>"));
        assert!(html.contains(">Notations</h1>"));
        assert!(html.contains("Every notation has YAML frontmatter"));
    }

    #[test]
    fn notations_page_opens_with_a_hero_and_a_story() {
        let html = render(AuthState::Anonymous).into_string();
        // The services-style neon hero band.
        assert!(
            html.contains("product-hero__title"),
            "expected the hero band"
        );
        // The story arc: checked Markdown → questionnaire + workflow → notation.
        assert!(
            html.contains("notations-story"),
            "expected the story section"
        );
        assert!(html.contains("executable form of legal work"));
        assert!(
            html.contains("/docs/frontmatter"),
            "story links the frontmatter guide"
        );
        // The hero owns the title, so the body must not repeat the README's H1.
        assert_eq!(
            html.matches(">Notations</h1>").count(),
            1,
            "exactly one Notations <h1> (the hero), not a duplicate from the README"
        );
    }

    #[test]
    fn strip_leading_h1_drops_only_the_first_heading_line() {
        assert_eq!(
            super::strip_leading_h1("# Notations\n\nBody first line.\n"),
            "Body first line.\n"
        );
        assert_eq!(
            super::strip_leading_h1("No heading here.\n"),
            "No heading here.\n"
        );
    }

    #[test]
    fn notations_page_is_tied_to_the_readme() {
        assert!(README.starts_with("# Notations"));
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
