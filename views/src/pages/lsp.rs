#![allow(clippy::doc_markdown)]
//! `/lsp` — the public showcase + install page for the
//! `navigator-lsp` language server.
//!
//! The "here is the editor experience" half of the markdown-notation
//! demo: one page that says what the LSP does (live rule diagnostics +
//! one-click `source.fixAll`, zero telemetry), how to install it
//! (`cargo install --path lsp`), and — reusing the per-editor snippets
//! already written under `docs/lsp/` — how to wire it into VS Code,
//! Neovim, Helix, Emacs, and Zed. The editor snippets are baked verbatim
//! via `include_str!` so this page can never drift from those docs.
//!
//! Prebuilt binaries are served straight from the public assets bucket:
//! `cli lsp publish` pushes one `navigator-lsp` per platform to
//! `lsp/<triple>/navigator-lsp`, and the download buttons here resolve
//! through the `views::assets::asset_url` seam — `/public` in dev, the
//! `<project>-assets` GCS bucket in production. A published `.vsix`
//! stays a follow-up. The shared blueprint disclaimer rides this page
//! too.

use maud::{html, Markup};

use crate::assets::asset_url;
use crate::brand::FOUNDATION_BRAND;
use crate::components::{external_link, legal_blueprint_disclaimer};
use crate::lsp::{lsp_binary_key, LSP_TARGETS};
use crate::{markdown, AuthState, PageLayout};

/// The Navigator monorepo — home of the `lsp/` crate and the bundled
/// editor extensions linked from this page.
const REPO_URL: &str = "https://github.com/neon-law-foundation/Navigator";
const VSCODE_EXT_URL: &str =
    "https://github.com/neon-law-foundation/Navigator/tree/main/lsp/vscode-ext";
const ZED_EXT_URL: &str = "https://github.com/neon-law-foundation/Navigator/tree/main/lsp/zed-ext";

// Per-editor setup snippets, single-sourced from `docs/lsp/`. The path
// is relative to this crate's manifest dir (`views/`); the docs tree is
// one level up. Baking them keeps this page byte-identical to the docs a
// git reader sees.
const NEOVIM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/lsp/neovim.md"
));
const HELIX: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/lsp/helix.md"));
const EMACS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/lsp/emacs.md"));
const VSCODE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/lsp/vscode.md"
));
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
                    " into any LSP-aware editor: live diagnostics as you type "
                    "and a one-click "
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
                    "Prefer to build it yourself? It takes one command. (A "
                    "published VS Code "
                    code { ".vsix" }
                    " is still a planned follow-up.)"
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
                h2 { "Bundled editor extensions" }
                p {
                    "VS Code and Zed ship ready-to-sideload extensions in the "
                    "Navigator repo:"
                }
                ul {
                    li { (external_link(VSCODE_EXT_URL, html! { code { "lsp/vscode-ext/" } })) }
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
                (editor_section(VSCODE))
                (editor_section(NEOVIM))
                (editor_section(HELIX))
                (editor_section(EMACS))
                (editor_section(ZED))
            }
        }
    };
    PageLayout::new("Navigator LSP")
        .with_description(
            "Install navigator-lsp — live markdown-notation diagnostics and \
             one-click fixes in any LSP-aware editor. Zero telemetry.",
        )
        .with_brand(*FOUNDATION_BRAND)
        .with_auth(auth)
        .render(&body)
}

/// The "Download a prebuilt binary" section: one download link per
/// [`LSP_TARGETS`] entry, each resolved through [`asset_url`] so the URL
/// points at `/public` in dev and the `<project>-assets` GCS bucket in
/// production — the same key `cli lsp publish` uploads to.
fn prebuilt_downloads() -> Markup {
    html! {
        section."mt-4" {
            h2 { "Download a prebuilt binary" }
            p {
                "Grab the binary for your platform, make it executable, "
                "and put it on your "
                code { "$PATH" }
                " (or point your editor's "
                code { "binary.path" }
                " at it). It's the same "
                code { "navigator-lsp" }
                " — JSON-RPC over stdio, zero telemetry."
            }
            ul.lsp-downloads {
                @for target in LSP_TARGETS {
                    li {
                        a download href=(asset_url(&lsp_binary_key(target.triple))) {
                            (target.label)
                        }
                        " "
                        code { (target.triple) }
                    }
                }
            }
            pre { code {
                "# make it runnable, then put it on your $PATH\n"
                "chmod +x navigator-lsp\n"
                "mv navigator-lsp /usr/local/bin/\n"
            } }
        }
    }
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
    fn shows_install_command_and_all_five_editors() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("cargo install --path lsp"));
        assert!(html.contains("source.fixAll"));
        for editor in ["VS Code", "Neovim", "Helix", "Emacs", "Zed"] {
            assert!(html.contains(editor), "missing editor section {editor}");
        }
    }

    #[test]
    fn carries_the_shared_disclaimer() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("not legal advice"));
    }

    #[test]
    fn offers_a_prebuilt_download_link_for_every_target() {
        // Each registry target gets a download link resolving through the
        // `asset_url` seam (the `lsp/<triple>/navigator-lsp` key). With no
        // `NAVIGATOR_ASSET_BASE_URL` set (tests/dev), that resolves to the
        // `/public` mount; in prod it points at the assets bucket.
        let html = render(AuthState::Anonymous).into_string();
        for target in crate::lsp::LSP_TARGETS {
            let href = crate::assets::asset_url(&crate::lsp::lsp_binary_key(target.triple));
            assert!(
                html.contains(&format!("href=\"{href}\"")),
                "missing download link for {}",
                target.triple
            );
            assert!(
                html.contains(target.label),
                "missing label {}",
                target.label
            );
        }
    }

    #[test]
    fn still_offers_build_from_source() {
        let html = render(AuthState::Anonymous).into_string();
        assert!(html.contains("cargo install --path lsp"));
    }
}
