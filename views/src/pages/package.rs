#![allow(clippy::doc_markdown)]
//! `/foundation/navigator/<pkg>` — one page per shipped Navigator
//! package.
//!
//! Navigator is published as a set of open-source crates; the Foundation
//! surfaces the ones a reader actually runs as their own pages under the
//! `/foundation/navigator` hub:
//!
//! - `lsp` — the editor language server (its own, richer page in
//!   [`crate::pages::lsp`]);
//! - `cli` — the `navigator` operator CLI;
//! - `mcp` — the Model Context Protocol server (AIDA's tools);
//! - `web` — the web app + JSON API (this binary).
//!
//! The CLI / MCP / Web pages render that crate's `README.md` verbatim
//! (baked at compile time, so the page can never drift from the repo),
//! retargeting repo-relative links the same way [`crate::pages::navigator`]
//! does so nothing dead-ends. The shared [`package_strip`] is the
//! cross-package nav rendered on the hub and atop each package page.

use maud::{html, Markup};

use crate::brand::FOUNDATION_BRAND;
use crate::markdown::render_with_link_rewrite;
use crate::pages::navigator::rewrite_link;
use crate::{AuthState, PageLayout};

/// One shipped package, as shown in the [`package_strip`]. `href` is the
/// page under the `/foundation/navigator` hub; `icon` is a Bootstrap Icon
/// glyph name (without the `bi-` prefix).
pub struct Package {
    pub label: &'static str,
    pub href: &'static str,
    pub blurb: &'static str,
    pub icon: &'static str,
}

/// The four reader-facing packages, in install-order of tangibility:
/// the editor plugin first (Leo's call — most "this helps me today"),
/// then the CLI, the MCP server, and the web app the firm runs on.
pub const PACKAGES: &[Package] = &[
    Package {
        label: "LSP",
        href: "/foundation/navigator/lsp",
        blurb: "Live markdown-notation diagnostics and one-click fixes in any editor.",
        icon: "braces",
    },
    Package {
        label: "CLI",
        href: "/foundation/navigator/cli",
        blurb: "The navigator operator CLI — validate, import, seed, deploy.",
        icon: "terminal",
    },
    Package {
        label: "MCP",
        href: "/foundation/navigator/mcp",
        blurb: "AIDA's tools over the Model Context Protocol for any LLM client.",
        icon: "plugin",
    },
    Package {
        label: "Web",
        href: "/foundation/navigator/web",
        blurb: "The web app and JSON API — the product, served from one binary.",
        icon: "globe2",
    },
];

/// A responsive card grid linking every package, with `current` (a page
/// href) highlighted. Rendered on the hub and atop each package page so a
/// reader can hop between the LSP, CLI, MCP, and Web without going back.
#[must_use]
pub fn package_strip(current: Option<&str>) -> Markup {
    html! {
        nav."mb-4" aria-label="Navigator packages" {
            div."row"."row-cols-2"."row-cols-md-4"."g-3" {
                @for pkg in PACKAGES {
                    @let active = current == Some(pkg.href);
                    div."col" {
                        a."card"."h-100"."text-decoration-none"
                            .border-primary[active]
                            href=(pkg.href)
                            aria-current=[active.then_some("page")]
                        {
                            div."card-body" {
                                p."h5"."mb-1" {
                                    i class={ "bi bi-" (pkg.icon) " me-2" } aria-hidden="true" {}
                                    (pkg.label)
                                }
                                p."small"."text-body-secondary"."mb-0" { (pkg.blurb) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render a package page: the cross-package strip, then the crate's
/// `README.md` with repo-relative links retargeted for the web (see
/// [`rewrite_link`]). `current` highlights this package in the strip.
#[must_use]
pub fn render(
    title: &str,
    description: &str,
    readme: &str,
    current: &str,
    auth: AuthState,
) -> Markup {
    let body = html! {
        (package_strip(Some(current)))
        article.docs-article {
            (render_with_link_rewrite(readme, rewrite_link))
        }
    };
    PageLayout::new(title)
        .with_description(description)
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

#[cfg(test)]
mod tests {
    use super::{package_strip, render, PACKAGES};
    use crate::brand::FOUNDATION_BRAND;
    use crate::AuthState;

    #[test]
    fn strip_links_every_package_under_the_navigator_hub() {
        let html = package_strip(None).into_string();
        for pkg in PACKAGES {
            assert!(
                pkg.href.starts_with("/foundation/navigator/"),
                "every package lives under the hub: {}",
                pkg.href
            );
            assert!(
                html.contains(&format!("href=\"{}\"", pkg.href)),
                "strip missing link for {}",
                pkg.label
            );
        }
        // The four reader-facing packages: LSP, CLI, MCP, Web.
        let labels: Vec<&str> = PACKAGES.iter().map(|p| p.label).collect();
        assert_eq!(labels, ["LSP", "CLI", "MCP", "Web"]);
    }

    #[test]
    fn strip_marks_the_current_package() {
        let html = package_strip(Some("/foundation/navigator/cli")).into_string();
        assert!(
            html.contains("aria-current=\"page\""),
            "the current package should be marked, got: {html}"
        );
    }

    #[test]
    fn render_emits_readme_under_foundation_brand_with_rewritten_links() {
        let readme = "# CLI\n\nSee [the glossary](docs/glossary.md) and [a template](notation_templates/nest/nevada.md).\n";
        let html = render(
            "Navigator CLI",
            "The navigator operator CLI.",
            readme,
            "/foundation/navigator/cli",
            AuthState::Anonymous,
        )
        .into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Navigator CLI</title>",
            FOUNDATION_BRAND.site_name
        )));
        // Repo-relative links are retargeted for the web (same mapping as
        // the README page): a top-level doc → /docs/<slug>, a template →
        // the raw template API.
        assert!(html.contains("href=\"/docs/glossary\""), "got: {html}");
        assert!(
            html.contains("href=\"/api/templates/nest/nevada\""),
            "got: {html}"
        );
        // The cross-package strip rides above the body.
        assert!(html.contains("aria-label=\"Navigator packages\""));
    }
}
