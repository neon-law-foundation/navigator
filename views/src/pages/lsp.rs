#![allow(clippy::doc_markdown)]
//! `/lsp` — the public showcase + install page for the
//! `navigator-lsp` language server.
//!
//! The "here is the editor experience" half of the markdown-notation
//! demo: one page that says what the LSP does (live rule diagnostics +
//! one-click `source.fixAll`, zero telemetry), how to install it
//! (`cargo install --path lsp`), and — reusing the Zed setup snippet
//! already written under `docs/lsp/` — how to wire it into Zed. The snippet
//! is baked verbatim via `include_str!` so this page can never drift from the
//! local docs.
//!
//! Prebuilt binaries are served straight from the public assets bucket:
//! `cli lsp publish` pushes one `navigator-lsp` per platform to
//! the tag's GitHub Release, and the download buttons here resolve to
//! those versioned `YY.MM.DD` release assets. The shared blueprint
//! disclaimer rides this page too.

use maud::{html, Markup};

use crate::brand::FOUNDATION_BRAND;
use crate::components::{external_link, legal_blueprint_disclaimer};
use crate::pages::package::{release_downloads, ReleaseBinary};
use crate::{markdown, AuthState, PageLayout};

/// The Navigator monorepo — home of the `lsp/` crate and the bundled
/// Zed extension linked from this page.
const REPO_URL: &str = "https://github.com/neon-law-foundation/Navigator";
const ZED_EXT_URL: &str = "https://github.com/neon-law-foundation/Navigator/tree/main/lsp/zed-ext";

// The public page currently shows only the Zed setup path. Keep it
// single-sourced from `docs/lsp/zed.md` so the local setup and the website
// stay in sync.
const ZED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/lsp/zed.md"));

#[must_use]
pub fn render(auth: AuthState) -> Markup {
    let body = html! {
        // Cross-package nav: the LSP page sits under the Navigator hub
        // beside the CLI, MCP, and Web pages.
        (crate::pages::package::package_strip(Some("/foundation/navigator/lsp")))
        article {
            header {
                h1 { "Navigator LSP — the editor experience" }
                p.lead {
                    "Our legal templates are plain markdown, so they get "
                    "first-class editor tooling. "
                    code { "navigator-lsp" }
                    " is one binary — JSON-RPC over stdio, zero telemetry — "
                    "that brings the same rule engine as "
                    code { "cli validate" }
                    " into Zed: live diagnostics as you type and a one-click "
                    code { "source.fixAll" }
                    " that cleans every safe-by-construction rule."
                }
                p {
                    "New here? "
                    a href="/templates" { "Browse the template gallery" }
                    " to see the plain-markdown notation these tools work on."
                }
            }
            (legal_blueprint_disclaimer())

            (prebuilt_downloads())

            section."mt-4" {
                h2 { "…or build from source" }
                p {
                    "Prefer to build it yourself? It takes one command."
                }
                pre { code {
                    "# put navigator-lsp on your $PATH\n"
                    "cargo install --path lsp\n\n"
                    "# OR build once without installing:\n"
                    "cargo build --release -p lsp\n"
                    "# binary at: target/release/navigator-lsp\n"
                } }
            }

            section."mt-4" {
                h2 { "Bundled editor extension" }
                p {
                    "Zed ships a ready-to-sideload extension in the Navigator "
                    "repo:"
                }
                ul {
                    li { (external_link(ZED_EXT_URL, html! { code { "lsp/zed-ext/" } })) }
                }
                p {
                    "Everything else — the crate, the rules, this page's "
                    "source — lives at "
                    (external_link(REPO_URL, html! { (REPO_URL) }))
                    "."
                }
            }

            section."mt-4" {
                h2 { "Editor setup" }
                (editor_section(ZED))
            }
        }
    };
    PageLayout::new("Navigator LSP")
        .with_description(
            "Install navigator-lsp for Zed — live markdown-notation diagnostics \
             and one-click fixes. Zero telemetry.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// The "Download" section: one archive per supported desktop platform,
/// resolved to the current `YY.MM.DD` GitHub Release when this page is
/// running from a deployed image.
fn prebuilt_downloads() -> Markup {
    release_downloads(ReleaseBinary::NavigatorLsp)
}

/// Render one editor's `docs/lsp/*.md` snippet verbatim. Each source
/// file already opens with its own `# <Editor>` heading, so we drop it
/// straight in.
fn editor_section(source: &str) -> Markup {
    html! {
        div."lsp-editor"."mt-3" {
            (markdown::render(source))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::AuthState;

    #[test]
    fn shows_install_command_and_zed_setup_only() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("cargo install --path lsp"));
        assert!(html.contains("source.fixAll"));
        assert!(html.contains("Zed"), "missing Zed setup section");
        for editor in ["VS Code", "Neovim", "Helix", "Emacs"] {
            assert!(!html.contains(editor), "unexpected editor section {editor}");
        }
    }

    #[test]
    fn carries_the_shared_disclaimer() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("not legal advice"));
    }

    #[test]
    fn offers_release_download_links() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains(">Download</h2>"), "got: {html}");
        assert!(
            html.contains("https://github.com/neon-law-foundation/Navigator/releases"),
            "got: {html}"
        );
        assert!(html.contains("YY.MM.DD"), "got: {html}");
    }

    #[test]
    fn still_offers_build_from_source() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("cargo install --path lsp"));
    }
}
