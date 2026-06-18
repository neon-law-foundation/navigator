//! Syntax-highlighted code blocks.
//!
//! One highlighter for the whole app — the workshop and presentation
//! slides (the "Rust in Peace" talk) and the `/design` gallery all render
//! fenced code the same way: a `<pre><code class="language-rust">` block in
//! the convention `hljs.highlightAll()` consumes, plus the vendored
//! highlight.js assets loaded once per page. pulldown-cmark emits exactly
//! this markup for the talk's fenced blocks, so [`code_block`] is its maud
//! twin for pages that build the block by hand.

use maud::{html, Markup};

/// Highlight.js assets for the code samples in workshop / presentation
/// material and the design gallery. Vendored under `web/public` next to
/// Alpine and htmx (no CDN), with the Gherkin grammar appended to the core
/// bundle so feature-file slides highlight too. pulldown-cmark already
/// emits `class="language-…"` on fenced blocks, which is exactly the
/// convention `hljs.highlightAll()` consumes.
///
/// The `highlightAll()` call lives in the first-party external
/// `js/highlight-init.js`, NOT an inline `<script>`: the app's
/// `script-src 'self'` CSP (see `web::api`) blocks inline script, so an
/// inline init silently never runs and nothing highlights. Pair with one or
/// more [`code_block`]s (or pulldown-cmark output); load once per page —
/// marketing pages ship no highlighter.
///
/// These are linked from the same-origin `/public` mount — not through
/// [`assets::asset_url`] — exactly like Bootstrap, htmx, and Alpine in
/// [`crate::layout`]. The `asset_url` seam points at the cross-origin
/// photo CDN in production (`NAVIGATOR_ASSET_BASE_URL`), which the
/// `script-src 'self'` CSP would block and where these vendored scripts
/// aren't uploaded anyway (`cli assets build` ships only the photo
/// variants). Vendored code stays same-origin; only photos go to the CDN.
#[must_use]
pub fn syntax_highlight_assets() -> Markup {
    html! {
        link rel="stylesheet" href="/public/css/highlight-github-dark.min.css";
        script src="/public/js/highlight.min.js" {}
        // External init (CSP-safe) — runs hljs.highlightAll() after the
        // bundle above defines `hljs`.
        script src="/public/js/highlight-init.js" {}
    }
}

/// A fenced Rust code block in the highlight.js convention
/// (`<pre><code class="language-rust">`) — the same markup pulldown-cmark
/// emits for the talk slides, so `hljs.highlightAll()` styles it. The body
/// is HTML-escaped by maud. Call [`syntax_highlight_assets`] once on the
/// same page so the highlighter actually runs.
#[must_use]
pub fn code_block(code: &str) -> Markup {
    html! {
        pre { code."language-rust" { (code) } }
    }
}

#[cfg(test)]
mod tests {
    use super::{code_block, syntax_highlight_assets};

    #[test]
    fn code_block_uses_the_highlightjs_language_convention() {
        let out = code_block("let x = 1;").into_string();
        assert!(out.contains("<pre>"));
        assert!(out.contains("class=\"language-rust\""));
        assert!(out.contains("let x = 1;"));
    }

    #[test]
    fn code_block_escapes_html_in_the_source() {
        let out = code_block("Vec<String>").into_string();
        assert!(out.contains("Vec&lt;String&gt;"));
    }

    #[test]
    fn assets_load_the_vendored_highlighter_and_init() {
        let out = syntax_highlight_assets().into_string();
        assert!(out.contains("highlight.min.js"));
        assert!(out.contains("highlight-github-dark.min.css"));
        // The init is an external first-party file, not inline — the CSP
        // (`script-src 'self'`) blocks an inline init call.
        assert!(out.contains("highlight-init.js"));
        assert!(!out.contains("<script>hljs.highlightAll()</script>"));
    }
}
