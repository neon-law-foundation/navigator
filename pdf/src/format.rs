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

/// The firm identity printed on an [`OutputFormat::Letter`] letterhead.
///
/// The `pdf` crate is brand-agnostic: the caller supplies these lines.
/// The CLI reads them from `views::brand::FIRM_BRAND`, so the rendered
/// letterhead honors the same `NAVIGATOR_*` env overrides as the
/// website footer. [`Default`] is the Neon Law canonical deployment, so
/// the crate's own tests and any indifferent caller stay self-contained.
#[derive(Debug, Clone)]
pub struct Letterhead {
    /// Firm display name, e.g. `Neon Law`.
    pub name: String,
    /// Contact line beside the address, e.g. `neonlaw.com`.
    pub contact: String,
    /// One-line postal address, e.g. `5150 Mae Anne Ave Ste 405-9002,
    /// Reno, NV 89523`.
    pub address: String,
}

impl Default for Letterhead {
    fn default() -> Self {
        Self {
            name: "Neon Law".to_string(),
            contact: "neonlaw.com".to_string(),
            address: "5150 Mae Anne Ave Ste 405-9002, Reno, NV 89523".to_string(),
        }
    }
}

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
    /// `letterhead` is used only by [`OutputFormat::Letter`].
    #[must_use]
    pub fn preamble(self, letterhead: &Letterhead) -> String {
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
                    "  #v(0.35em)\n",
                    "  #align(center)[#text(size: 10pt, weight: \"bold\", tracking: 0.1em)[{name}]]\n",
                    "  #v(0.15em)\n",
                    "  #align(center)[#text(size: 8pt, fill: luma(40%))[{contact} · {address}]]\n",
                    "  #v(0.5em)\n",
                    "  #line(length: 100%, stroke: 0.5pt + luma(60%))\n",
                    "]\n\n",
                ),
                logo = LOGO_PATH,
                name = esc(&letterhead.name),
                contact = esc(&letterhead.contact),
                address = esc(&letterhead.address),
            ),
        }
    }
}

/// Escape the Typst markup sigils so a letterhead string renders
/// verbatim in content context. Mirrors `markdown::escape_text`'s set;
/// a white-label fork's firm name/address may carry arbitrary
/// characters, so this is a correctness guard, not cosmetic.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '@' | '[' | ']'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Render a notation's Markdown `body` to PDF bytes, framed by `format`.
///
/// Converts the Markdown to Typst ([`crate::markdown::to_typst`]),
/// prepends the format's chrome ([`OutputFormat::preamble`]), and
/// compiles ([`render`]). `letterhead` supplies the firm identity for
/// [`OutputFormat::Letter`] (ignored by `Plain`). Placeholder tokens
/// are the caller's responsibility — substitute them in `body` first.
///
/// # Errors
///
/// Returns [`PdfError::Compile`] / [`PdfError::Export`] when the
/// converted document fails to compile or export.
pub fn render_document(
    body: &str,
    format: OutputFormat,
    letterhead: &Letterhead,
) -> Result<Vec<u8>, PdfError> {
    let source = format!(
        "{}{}",
        format.preamble(letterhead),
        crate::markdown::to_typst(body)
    );
    render(&source)
}

#[cfg(test)]
mod tests {
    use super::{Letterhead, OutputFormat};

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
        let pdf = super::render_document(
            "# Notice\n\nBody text.",
            OutputFormat::Plain,
            &Letterhead::default(),
        )
        .expect("plain renders");
        assert_eq!(&pdf[..4], b"%PDF", "not a PDF");
    }

    #[test]
    fn letter_render_embeds_the_logo_and_produces_a_pdf() {
        // The letterhead path must actually compile the `#image(..)` —
        // i.e. the embedded logo resolves through the file resolver.
        let lh = Letterhead::default();
        let pdf = super::render_document(
            "Dear Counsel,\n\nThis letter concerns **NEON LAW**.",
            OutputFormat::Letter,
            &lh,
        )
        .expect("letter renders with embedded logo");
        assert_eq!(&pdf[..4], b"%PDF", "not a PDF");
        // A letter carries the embedded PNG, so the output is materially
        // larger than the same body rendered plain.
        let plain = super::render_document(
            "Dear Counsel,\n\nThis letter concerns **NEON LAW**.",
            OutputFormat::Plain,
            &lh,
        )
        .expect("plain renders");
        assert!(
            pdf.len() > plain.len(),
            "letter ({}) should be larger than plain ({}) — logo missing?",
            pdf.len(),
            plain.len()
        );
    }

    #[test]
    fn letter_preamble_prints_the_firm_address_and_plain_does_not() {
        let lh = Letterhead {
            name: "Neon Law".into(),
            contact: "neonlaw.com".into(),
            address: "5150 Mae Anne Ave Ste 405-9002, Reno, NV 89523".into(),
        };
        let letter = OutputFormat::Letter.preamble(&lh);
        assert!(
            letter.contains("5150 Mae Anne Ave Ste 405-9002, Reno, NV 89523"),
            "letterhead must carry the firm address: {letter}"
        );
        assert!(letter.contains("neonlaw.com"));
        assert!(letter.contains("Neon Law"));
        assert!(letter.contains("logo-firm.png"));
        // The plain format carries no letterhead at all.
        assert!(!OutputFormat::Plain.preamble(&lh).contains("Mae Anne"));
    }

    #[test]
    fn letterhead_strings_are_escaped_against_typst_injection() {
        // A white-label address with a `#` must not start a Typst call.
        let lh = Letterhead {
            address: "#1 Main St".into(),
            ..Letterhead::default()
        };
        // Must still compile (escaped), not error.
        super::render_document("Body.", OutputFormat::Letter, &lh)
            .expect("letterhead with a sigil must render");
    }
}
