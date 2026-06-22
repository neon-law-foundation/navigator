//! Output formats — the chrome wrapped around a rendered notation.
//!
//! A notation template's Markdown body says *what* the document says;
//! the [`OutputFormat`] says *how it is dressed*: a plain document, or
//! a firm **letter** on Neon Law letterhead with the logo at the top.
//! This is the extension seam — a new form (pleading paper, a fax
//! cover, an invoice) is a new [`OutputFormat`] variant plus the Typst
//! [`OutputFormat::preamble`] that frames it. The body conversion
//! ([`crate::markdown::to_typst`]) and the embedded logo are shared, so
//! a new variant only describes its own page chrome.
//!
//! The set of formats a template may *declare* in its `output:`
//! frontmatter field is validated by the `rules` crate's `N109` rule;
//! keep [`OutputFormat::FRONTMATTER_VALUES`] in step with it.

use crate::{render, PdfError, LOGO_PATH};

/// How a rendered notation is framed on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// No letterhead: page geometry and the firm typeface only. The
    /// default when a template declares no `output:` field.
    #[default]
    Plain,
    /// A firm letter on Neon Law letterhead — the logo and firm line
    /// head the first page, the body flows beneath.
    Letter,
}

impl OutputFormat {
    /// The `output:` frontmatter values that map to a non-default
    /// format. `Plain` is the implicit default and is not declared, so
    /// it is absent here. The `rules` `N109` validator accepts exactly
    /// these strings.
    pub const FRONTMATTER_VALUES: &'static [&'static str] = &["letter"];

    /// Parse a format name as it appears in `output:` frontmatter or on
    /// the CLI `--format` flag. Accepts `plain` and `letter`; returns
    /// `None` for anything else so callers can report it.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "plain" => Some(Self::Plain),
            "letter" => Some(Self::Letter),
            _ => None,
        }
    }

    /// The Typst chrome preamble for this format — page geometry, sizing,
    /// and any letterhead. Prepended to the body's Typst markup before
    /// [`render`]. The font family is set separately by [`render`].
    #[must_use]
    pub fn preamble(self) -> String {
        // Shared page sizing; the letterhead leaves extra top margin so
        // the logo block clears the body.
        match self {
            Self::Plain => concat!(
                "#set page(paper: \"us-letter\", margin: 1in)\n",
                "#set text(size: 11pt)\n",
                "#set par(justify: true, leading: 0.65em)\n\n",
            )
            .to_string(),
            Self::Letter => format!(
                concat!(
                    "#set page(paper: \"us-letter\", margin: (x: 1in, top: 1.1in, bottom: 1in))\n",
                    "#set text(size: 11pt)\n",
                    "#set par(justify: true, leading: 0.65em)\n",
                    "#block(below: 1.4em)[\n",
                    "  #align(center)[#image(\"{logo}\", width: 0.9in)]\n",
                    "  #v(0.3em)\n",
                    "  #align(center)[#text(size: 9pt, fill: luma(35%), tracking: 0.08em)[",
                    "NEON LAW · neonlaw.com]]\n",
                    "  #v(0.4em)\n",
                    "  #line(length: 100%, stroke: 0.5pt + luma(60%))\n",
                    "]\n\n",
                ),
                logo = LOGO_PATH,
            ),
        }
    }
}

/// Render a notation's Markdown `body` to PDF bytes, framed by `format`.
///
/// Converts the Markdown to Typst ([`crate::markdown::to_typst`]),
/// prepends the format's chrome ([`OutputFormat::preamble`]), and
/// compiles ([`render`]). Placeholder tokens are the caller's
/// responsibility — substitute them in `body` before calling.
///
/// # Errors
///
/// Returns [`PdfError::Compile`] / [`PdfError::Export`] when the
/// converted document fails to compile or export.
pub fn render_document(body: &str, format: OutputFormat) -> Result<Vec<u8>, PdfError> {
    let source = format!("{}{}", format.preamble(), crate::markdown::to_typst(body));
    render(&source)
}

#[cfg(test)]
mod tests {
    use super::OutputFormat;

    #[test]
    fn parse_accepts_known_names_and_rejects_others() {
        assert_eq!(OutputFormat::parse("plain"), Some(OutputFormat::Plain));
        assert_eq!(OutputFormat::parse("letter"), Some(OutputFormat::Letter));
        assert_eq!(OutputFormat::parse(" letter "), Some(OutputFormat::Letter));
        assert_eq!(OutputFormat::parse("demand_letter"), None);
        assert_eq!(OutputFormat::parse(""), None);
    }

    #[test]
    fn default_is_plain() {
        assert_eq!(OutputFormat::default(), OutputFormat::Plain);
    }

    #[test]
    fn frontmatter_values_parse_back_to_a_format() {
        // The validator's accepted strings must each map to a real
        // format, or a template could declare an output that can't be
        // rendered.
        for v in OutputFormat::FRONTMATTER_VALUES {
            assert!(OutputFormat::parse(v).is_some(), "unparseable: {v}");
        }
    }

    #[test]
    fn plain_render_produces_a_pdf() {
        let pdf = super::render_document("# Notice\n\nBody text.", OutputFormat::Plain)
            .expect("plain renders");
        assert_eq!(&pdf[..4], b"%PDF", "not a PDF");
    }

    #[test]
    fn letter_render_embeds_the_logo_and_produces_a_pdf() {
        // The letterhead path must actually compile the `#image(..)` —
        // i.e. the embedded logo resolves through the file resolver.
        let pdf = super::render_document(
            "Dear Counsel,\n\nThis letter concerns **NEON LAW**.",
            OutputFormat::Letter,
        )
        .expect("letter renders with embedded logo");
        assert_eq!(&pdf[..4], b"%PDF", "not a PDF");
        // A letter carries the embedded PNG, so the output is materially
        // larger than the same body rendered plain.
        let plain = super::render_document(
            "Dear Counsel,\n\nThis letter concerns **NEON LAW**.",
            OutputFormat::Plain,
        )
        .expect("plain renders");
        assert!(
            pdf.len() > plain.len(),
            "letter ({}) should be larger than plain ({}) — logo missing?",
            pdf.len(),
            plain.len()
        );
    }
}
