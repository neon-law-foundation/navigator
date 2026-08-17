//! Server-side syntax-highlighted code blocks.
//!
//! The workshop and presentation slides (the "Rust in Peace" talk) and the
//! `/design` gallery all render fenced code the same way. Highlighting runs
//! **on the server** through `syntect` (pure Rust): [`highlight`] turns a code
//! string into a `<pre>` whose tokens carry inline `style="color:…"` spans, so
//! the colours are in the server-rendered HTML with no client JavaScript and no
//! vendored highlighter. Inline styles need only `style-src 'unsafe-inline'`
//! (already granted by the app CSP); nothing is added to `script-src`.
//!
//! [`code_block`] is for pages that build a Rust block by hand;
//! [`crate::markdown`] highlights pulldown-cmark's fenced blocks through the
//! same [`highlight`] path.

use std::sync::LazyLock;

use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

/// The bundled syntax definitions, loaded once. `load_defaults_newlines`
/// carries the common languages (Rust, TOML, JSON, YAML, Bash, …); an
/// unrecognised fence falls back to plain text (readable, just uncoloured).
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// The WCAG AA contrast floor for body text. A code block is body text, not
/// decoration, so its tokens are held to the reading floor rather than the
/// 3:1 large-text one.
const AA_CONTRAST_FLOOR: f64 = 4.5;

/// A dark theme, loaded once, matching the previous github-dark palette's
/// intent. Ships with syntect, so no theme file is vendored.
///
/// `base16-eighties.dark` rather than `base16-ocean.dark`: ocean's red
/// (`#bf616a`) sits at **3.2:1** on its own `#2b303b` ground, well under the
/// reading floor, and the browser accessibility gate reports it as a
/// `color-contrast` violation on every code block Navigator renders
/// (`server/tests/accessibility_e2e.rs`, the `/design` component audit).
/// Eighties keeps the same base16 structure and the same dark intent while
/// clearing it — its red is `#f2777a` on `#2d2d2d`, 5.0:1.
///
/// Swapping the theme is not on its own enough, and this is worth stating
/// plainly: **no theme syntect bundles clears the floor on every colour it
/// paints.** Measured across all seven, the best is eighties with one failing
/// token and the worst is `base16-ocean.light` with seven. The survivor in
/// every base16 ramp is the comment, which the palette deliberately mutes
/// (`#747369`, 2.9:1) — and a comment is prose a reader is *more* likely to
/// need than the code around it. So the loaded theme is normalised by
/// [`raise_to_reading_contrast`] rather than accepted as shipped.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let mut theme = ThemeSet::load_defaults().themes["base16-eighties.dark"].clone();
    raise_to_reading_contrast(&mut theme);
    theme
});

/// Lift every token colour in `theme` that falls below [`AA_CONTRAST_FLOOR`]
/// until it clears, by mixing it toward the theme's own foreground.
///
/// Applied once at load. Mixing toward the theme's foreground rather than to
/// flat white keeps the result inside the palette's own range — a muted
/// comment becomes a lighter grey of the same family, not a white one — and
/// keeps the tokens distinguishable from each other, which is the whole point
/// of highlighting. A colour that still cannot clear the floor after fully
/// blending is left as the foreground itself, which by construction reads.
///
/// This is a normalisation rather than a hand-picked override so it survives a
/// theme swap: choosing a different theme cannot reintroduce an unreadable
/// token, because whatever that theme's low-contrast colours are, they are
/// lifted here too.
fn raise_to_reading_contrast(theme: &mut Theme) {
    let (Some(background), Some(foreground)) =
        (theme.settings.background, theme.settings.foreground)
    else {
        return;
    };
    for item in &mut theme.scopes {
        let Some(colour) = item.style.foreground else {
            continue;
        };
        if contrast_ratio(colour, background) >= AA_CONTRAST_FLOOR {
            continue;
        }
        // Step toward the theme foreground in twentieths, stopping at the
        // first blend that clears the floor, so a colour is lightened as
        // little as it takes.
        item.style.foreground = Some(
            (1..=MIX_STEPS)
                .map(|step| mix(colour, foreground, step))
                .find(|candidate| contrast_ratio(*candidate, background) >= AA_CONTRAST_FLOOR)
                .unwrap_or(foreground),
        );
    }
}

/// How many steps [`raise_to_reading_contrast`] may take toward the theme
/// foreground. Twentieths are fine enough that a lifted colour is never more
/// than 5% further from its original than it had to be.
const MIX_STEPS: u16 = 20;

/// Linear blend of `from` toward `to`, `step` twentieths of the way.
///
/// Integer arithmetic throughout: the endpoints are `u8` and the weights are
/// small integers, so the blend stays exactly representable and needs no
/// float round-trip (and no lossy cast back).
fn mix(from: Color, to: Color, step: u16) -> Color {
    let channel = |a: u8, b: u8| {
        // `+ MIX_STEPS / 2` rounds to nearest rather than truncating toward
        // zero, so a lift is never a step short of the floor it just cleared.
        let blended =
            (u16::from(a) * (MIX_STEPS - step) + u16::from(b) * step + MIX_STEPS / 2) / MIX_STEPS;
        u8::try_from(blended).unwrap_or(u8::MAX)
    };
    Color {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
        a: 255,
    }
}

/// One sRGB channel, linearised per WCAG's relative-luminance definition.
fn linearise(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
fn luminance(colour: Color) -> f64 {
    0.2126f64.mul_add(
        linearise(colour.r),
        0.7152f64.mul_add(linearise(colour.g), 0.0722 * linearise(colour.b)),
    )
}

/// WCAG contrast ratio between two colours, lighter over darker.
fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// Highlight `code` as `lang` (a fence token like `rust`, `toml`, `bash`) into
/// a `<pre><code>` of inline-styled spans, server-side. An unknown or empty
/// `lang` renders as plain text; a highlighter error falls back to an
/// HTML-escaped `<pre><code>` so the source is always shown.
#[must_use]
pub fn highlight(code: &str, lang: &str) -> String {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    // On a highlighter error, fall back to an HTML-escaped block so the source
    // is always shown. On success, wrap syntect's `<pre>` tokens in a semantic
    // `<code>` element so the coloured block keeps the same `<pre><code>` shape
    // as the fallback (and as standard code-block markup).
    highlighted_html_for_string(code, &SYNTAX_SET, syntax, &THEME).map_or_else(
        |_| format!("<pre><code>{}</code></pre>", html_escape(code)),
        |html| wrap_tokens_in_code(&html),
    )
}

/// Insert a `<code>` element between syntect's `<pre …>` wrapper and its
/// highlighted token spans, so a successful highlight emits the same semantic
/// `<pre><code>…</code></pre>` shape as the escaped fallback. syntect renders
/// `<pre style="background-color:…">…spans…</pre>`, and its token text is
/// already HTML-escaped, so the first `>` closes the opening tag and the final
/// `</pre>` is the real close; if that shape is ever absent the markup is
/// returned unchanged rather than mangled.
fn wrap_tokens_in_code(html: &str) -> String {
    let (Some(open_end), Some(close_start)) = (html.find('>'), html.rfind("</pre>")) else {
        return html.to_string();
    };
    if open_end >= close_start {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + "<code></code>".len());
    out.push_str(&html[..=open_end]);
    out.push_str("<code>");
    out.push_str(&html[open_end + 1..close_start]);
    out.push_str("</code>");
    out.push_str(&html[close_start..]);
    out
}

/// A fenced Rust code block, highlighted server-side. The same call sites that
/// previously emitted `<pre><code class="language-rust">` for client-side
/// `hljs` now get coloured markup directly.
#[must_use]
pub fn code_block(code: &str) -> String {
    highlight(code, "rust")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{code_block, contrast_ratio, highlight, AA_CONTRAST_FLOOR, THEME};

    /// Source covering the token classes Navigator's fences actually produce:
    /// a comment, a doc comment, keywords, a string, a number, a type, a
    /// macro, and an attribute.
    const CONTRAST_SAMPLE: &str = r#"/// A doc comment.
// An ordinary comment, which is the lowest-contrast token in every base16 ramp.
#[derive(Debug)]
pub struct Matter { pub id: u32, pub name: String }

pub fn open(name: &str) -> Result<Matter, Error> {
    let id = 42;
    println!("opening {name}");
    Ok(Matter { id, name: name.to_string() })
}"#;

    /// Every colour a rendered code block actually paints, as `(hex, colour)`.
    ///
    /// Read back out of the emitted HTML rather than off the theme, because
    /// the theme declares scopes Navigator never reaches — editor furniture
    /// like git-gutter markers and `invalid.illegal` — and holding those to a
    /// reading-contrast floor would fail on colours no reader ever sees.
    fn emitted_colours(source: &str) -> Vec<(String, syntect::highlighting::Color)> {
        let html = highlight(source, "rust");
        let mut seen: Vec<(String, syntect::highlighting::Color)> = Vec::new();
        // syntect emits `style="color:#rrggbb;"` per token span.
        for fragment in html.split("style=\"color:#").skip(1) {
            let hex: String = fragment.chars().take(6).collect();
            if hex.len() < 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let channel = |i: usize| {
                u8::from_str_radix(&hex[i..i + 2], 16).expect("two hex digits per channel")
            };
            let colour = syntect::highlighting::Color {
                r: channel(0),
                g: channel(2),
                b: channel(4),
                a: 255,
            };
            if !seen.iter().any(|(existing, _)| *existing == hex) {
                seen.push((hex, colour));
            }
        }
        seen
    }

    /// Every colour a rendered code block paints clears WCAG AA for body text.
    ///
    /// This is the proof that `raise_to_reading_contrast` actually holds for
    /// what ships: it measures the colours the highlighter emits, after
    /// normalisation, rather than the colours the theme declares. The browser
    /// accessibility gate reports the same class of failure — it is what
    /// surfaced `base16-ocean.dark`'s red at 3.2:1 on the `/design` component
    /// audit — but that gate needs a live KIND cluster and a browser, so this
    /// catches a regression in the ordinary workspace run, at the module that
    /// chose the theme.
    ///
    /// It fails if the normalisation is removed, if a theme arrives without the
    /// background or foreground the lift needs, or if the emitted markup stops
    /// carrying inline colours.
    #[test]
    fn code_theme_meets_wcag_aa_contrast() {
        let background = THEME
            .settings
            .background
            .expect("the code theme declares a background");

        let painted = emitted_colours(CONTRAST_SAMPLE);
        assert!(
            painted.len() > 3,
            "the sample must actually paint a range of tokens, or this proves \
             nothing; got {painted:?}"
        );

        let failures: Vec<String> = painted
            .into_iter()
            .filter_map(|(hex, foreground)| {
                let ratio = contrast_ratio(foreground, background);
                (ratio < AA_CONTRAST_FLOOR).then(|| {
                    format!(
                        "#{hex} on #{:02x}{:02x}{:02x} is {ratio:.2}:1",
                        background.r, background.g, background.b,
                    )
                })
            })
            .collect();

        assert!(
            failures.is_empty(),
            "{} colour(s) a code block actually paints fall below the {AA_CONTRAST_FLOOR}:1 \
             WCAG AA floor for body text:\n  {}\n\
             Code is body text, so a token below the floor is a token some \
             readers cannot make out.",
            failures.len(),
            failures.join("\n  "),
        );
    }

    /// The lift only touches what fails, and never flattens the palette.
    ///
    /// Without this, a normalisation that replaced every colour with the
    /// theme foreground would satisfy the contrast test above while destroying
    /// the highlighting it exists to keep readable.
    #[test]
    fn raising_contrast_keeps_the_tokens_distinguishable() {
        let painted = emitted_colours(CONTRAST_SAMPLE);
        let distinct: std::collections::BTreeSet<&String> =
            painted.iter().map(|(hex, _)| hex).collect();
        assert!(
            distinct.len() >= 4,
            "the lift must leave the token colours distinct, not collapse them \
             onto one readable colour; got {distinct:?}"
        );
    }

    #[test]
    fn code_block_highlights_rust_server_side_with_inline_styles() {
        let out = code_block("let x = 1;");
        assert!(out.contains("<pre"), "renders a pre block: {out}");
        // The coloured block keeps the semantic `<pre><code>` shape, matching
        // the escaped fallback rather than dropping the `<code>` element.
        assert!(
            out.contains("<pre") && out.contains("<code>") && out.contains("</code></pre>"),
            "wraps tokens in a semantic code element: {out}",
        );
        // syntect emits inline-styled spans — the colour is in the HTML, no JS.
        assert!(
            out.contains("style=\"color:"),
            "tokens carry inline colour styles: {out}",
        );
        // No client-side highlighter is referenced any more.
        assert!(!out.contains("language-rust"), "no hljs class convention");
        assert!(!out.contains("highlight.min.js"), "no vendored highlighter");
    }

    #[test]
    fn code_block_escapes_html_in_the_source() {
        // syntect HTML-escapes token text, so `<` / `>` never reach the DOM raw.
        let out = code_block("let v: Vec<String> = vec![];");
        assert!(
            out.contains("Vec&lt;String&gt;"),
            "angle brackets escaped: {out}"
        );
        assert!(!out.contains("Vec<String>"), "no raw angle brackets: {out}");
    }

    #[test]
    fn highlight_falls_back_to_plain_text_for_an_unknown_language() {
        // An unrecognised fence still renders the source, just uncoloured.
        let out = highlight("greeting: hello", "no-such-lang");
        assert!(out.contains("<pre"), "still a pre block: {out}");
        assert!(out.contains("greeting"), "source text present: {out}");
    }
}
