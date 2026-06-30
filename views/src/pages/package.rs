#![allow(clippy::doc_markdown)]
//! `/foundation/navigator/<pkg>` — one page per shipped Neon Law Navigator
//! package.
//!
//! Neon Law Navigator is published as a set of open-source crates; the Foundation
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

use crate::brand::{deployed_release, foundation_github_url, FOUNDATION_BRAND};
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

/// A downloadable executable family attached to every `YY.M.D`
/// GitHub Release.
#[derive(Debug, Clone, Copy)]
pub enum ReleaseBinary {
    Cli,
    NavigatorLsp,
}

impl ReleaseBinary {
    const fn artifact_prefix(self) -> &'static str {
        match self {
            Self::Cli => "navigator",
            Self::NavigatorLsp => "navigator-lsp",
        }
    }

    const fn command_name(self) -> &'static str {
        match self {
            Self::Cli => "navigator",
            Self::NavigatorLsp => "navigator-lsp",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ReleasePlatform {
    slug: &'static str,
    label: &'static str,
    archive_ext: &'static str,
}

const RELEASE_PLATFORMS: &[ReleasePlatform] = &[
    ReleasePlatform {
        slug: "linux",
        label: "Linux",
        archive_ext: "tar.gz",
    },
    ReleasePlatform {
        slug: "macos",
        label: "macOS",
        archive_ext: "tar.gz",
    },
    ReleasePlatform {
        slug: "windows",
        label: "Windows",
        archive_ext: "zip",
    },
];

fn release_asset_name(binary: ReleaseBinary, tag: &str, platform: ReleasePlatform) -> String {
    format!(
        "{}-{tag}-{}.{}",
        binary.artifact_prefix(),
        platform.slug,
        platform.archive_ext
    )
}

fn release_asset_url(binary: ReleaseBinary, tag: &str, platform: ReleasePlatform) -> String {
    format!(
        "{}/releases/download/{tag}/{}",
        foundation_github_url(),
        release_asset_name(binary, tag, platform)
    )
}

/// Versioned download links for the public release artifacts. In prod,
/// `NAVIGATOR_RELEASE_TAG` is the `YY.M.D` tag baked into the image by
/// `deploy.yml`; in local/dev there is no release tag, so link to the
/// releases index rather than fabricating a semver asset name.
#[must_use]
pub fn release_downloads(binary: ReleaseBinary) -> Markup {
    html! {
        section."mt-4" {
            h2 { "Download" }
            @if let Some(tag) = deployed_release() {
                p {
                    "Version "
                    code { (tag) }
                    " is available for each supported desktop platform."
                }
                ul."lsp-downloads" {
                    @for platform in RELEASE_PLATFORMS {
                        @let href = release_asset_url(binary, tag, *platform);
                        @let asset = release_asset_name(binary, tag, *platform);
                        li {
                            a download href=(href) { (platform.label) }
                            " "
                            code { (asset) }
                        }
                    }
                }
                (post_download_steps(binary, tag))
            } @else {
                p {
                    "Release downloads are attached to each "
                    code { "YY.M.D" }
                    " tag on GitHub."
                }
                p {
                    a href=(format!("{}/releases", foundation_github_url())) {
                        "Open GitHub releases"
                    }
                }
            }
        }
    }
}

/// The shell steps shown beneath the download links once a release tag is
/// known: extract the archive, clear the macOS Gatekeeper quarantine, then
/// put the binary on `$PATH`. macOS stamps every downloaded file with
/// `com.apple.quarantine`, and these binaries are unsigned, so without the
/// `xattr` step Gatekeeper blocks the first run ("cannot be opened because
/// the developer cannot be verified"). It is the one platform that needs
/// more than extract-and-run, so the macOS asset name is the worked example.
fn post_download_steps(binary: ReleaseBinary, tag: &str) -> Markup {
    let cmd = binary.command_name();
    let macos = RELEASE_PLATFORMS
        .iter()
        .find(|p| p.slug == "macos")
        .expect("macOS is a supported release platform");
    let macos_asset = release_asset_name(binary, tag, *macos);
    html! {
        pre { code {
            "# extract the archive you downloaded above\n"
            "tar xzf " (macos_asset) "\n\n"
            "# macOS only: the binary is unsigned, so clear the quarantine\n"
            "# Gatekeeper adds to downloads, or the first run is blocked\n"
            "xattr -d com.apple.quarantine " (cmd) "\n\n"
            "# put it on your PATH, then confirm it runs\n"
            "sudo mv " (cmd) " /usr/local/bin/\n"
            (cmd) " --help\n"
        } }
    }
}

/// A responsive card grid linking every package, with `current` (a page
/// href) highlighted. Rendered on the hub and atop each package page so a
/// reader can hop between the LSP, CLI, MCP, and Web without going back.
#[must_use]
pub fn package_strip(current: Option<&str>) -> Markup {
    html! {
        nav."mb-4" aria-label="Neon Law Navigator packages" {
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

/// Render the CLI package page with first-class release downloads above
/// the README.
#[must_use]
pub fn render_cli(
    title: &str,
    description: &str,
    readme: &str,
    current: &str,
    auth: AuthState,
) -> Markup {
    let body = html! {
        (package_strip(Some(current)))
        (release_downloads(ReleaseBinary::Cli))
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
    use super::{
        package_strip, post_download_steps, release_asset_name, release_asset_url, render,
        render_cli, ReleaseBinary, PACKAGES, RELEASE_PLATFORMS,
    };
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
        let readme = "# CLI\n\nSee [the glossary](docs/glossary.md) and [a template](templates/forms/united_states/nevada/state/nv__llc_formation.md).\n";
        let html = render(
            "Neon Law Navigator CLI",
            "The navigator operator CLI.",
            readme,
            "/foundation/navigator/cli",
            AuthState::Anonymous,
        )
        .into_string();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains(&format!(
            "<title>{} | Neon Law Navigator CLI</title>",
            FOUNDATION_BRAND.site_name
        )));
        // Repo-relative links are retargeted for the web (same mapping as
        // the README page): a top-level doc → /docs/<slug>, a template →
        // the raw template API.
        assert!(html.contains("href=\"/docs/glossary\""), "got: {html}");
        assert!(
            html.contains(
                "href=\"/api/templates/forms/united-states/nevada/state/nv--llc-formation\""
            ),
            "got: {html}"
        );
        // The cross-package strip rides above the body.
        assert!(html.contains("aria-label=\"Neon Law Navigator packages\""));
    }

    #[test]
    fn release_asset_names_are_tagged_by_version_and_platform() {
        let tag = "26.6.24";
        assert_eq!(
            release_asset_name(ReleaseBinary::Cli, tag, RELEASE_PLATFORMS[0]),
            "navigator-26.6.24-linux.tar.gz"
        );
        assert_eq!(
            release_asset_name(ReleaseBinary::NavigatorLsp, tag, RELEASE_PLATFORMS[2]),
            "navigator-lsp-26.6.24-windows.zip"
        );
        assert_eq!(
            release_asset_url(ReleaseBinary::Cli, tag, RELEASE_PLATFORMS[1]),
            "https://github.com/neon-law-foundation/navigator/releases/download/26.6.24/navigator-26.6.24-macos.tar.gz"
        );
    }

    #[test]
    fn post_download_steps_give_a_working_macos_install() {
        // The macOS archive needs the Gatekeeper quarantine cleared before the
        // unsigned binary will run — "extract and add to PATH" alone is a dead
        // end on macOS. The worked example uses the macOS asset name.
        let html = post_download_steps(ReleaseBinary::NavigatorLsp, "26.6.25").into_string();
        assert!(
            html.contains("xattr -d com.apple.quarantine navigator-lsp"),
            "macOS install must clear the Gatekeeper quarantine, got: {html}"
        );
        assert!(
            html.contains("navigator-lsp-26.6.25-macos.tar.gz"),
            "the worked example should use the macOS asset name, got: {html}"
        );
    }

    #[test]
    fn cli_page_exposes_downloads_before_readme() {
        let html = render_cli(
            "Neon Law Navigator CLI",
            "The navigator operator CLI.",
            "# CLI\n\nBody.\n",
            "/foundation/navigator/cli",
            AuthState::Anonymous,
        )
        .into_string();
        assert!(html.contains(">Download</h2>"), "got: {html}");
        assert!(
            html.contains("https://github.com/neon-law-foundation/navigator/releases"),
            "got: {html}"
        );
        assert!(
            html.find(">Download</h2>").expect("download heading")
                < html
                    .find("<h1 id=\"cli\">CLI</h1>")
                    .expect("readme heading"),
            "downloads should precede the README: {html}"
        );
    }
}
