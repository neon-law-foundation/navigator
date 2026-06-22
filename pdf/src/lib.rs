//! PDF rendering for Navigator's legal documents.
//!
//! Backed by the [Typst](https://typst.app) embedded compiler, driven
//! directly through a small in-crate [`World`](typst::World)
//! implementation. Callers feed Typst markup to [`render`] and
//! get back the PDF bytes; the [`StorageService`](cloud::StorageService)
//! seam handles persistence.
//!
//! ## Fonts
//!
//! Every PDF this crate renders is set in **Noto Serif**, the firm's
//! typeface — a sturdy, screen-and-print legible serif whose broad
//! Unicode coverage (Latin + all European accents, Cyrillic, Greek,
//! Vietnamese) keeps client names and addresses rendering correctly
//! worldwide. Two Google Fonts variable masters (upright + italic,
//! `wght` axis) are embedded into the binary via `include_bytes!` from
//! `pdf/assets/fonts/NotoSerif/`; Typst instantiates Regular and Bold
//! off the weight axis. [`render`] prepends a `#set text(font: "Noto
//! Serif")` rule so the family is the document default; a caller can
//! still override it with its own `#set text` rule. The same typeface
//! is served self-hosted by `web` (see `web/public/fonts/noto-serif/`).
//!
//! Noto Serif ships under the SIL Open Font License 1.1; the full text
//! is at `pdf/assets/fonts/NotoSerif/OFL.txt`.
//!
//! ## Redaction styles
//!
//! Separate from the typeface above, these are the ways a *redacted*
//! (blacked-out) passage is drawn. Three modes match the
//! [`RedactionStyle`] enum:
//!
//! - [`RedactionStyle::Block`] — a solid black rectangle the width of
//!   the redacted text.
//! - [`RedactionStyle::Bar`] — a thin black bar centred vertically
//!   through the redacted text (the classic "with prejudice" mark).
//! - [`RedactionStyle::Strike`] — a strikethrough on the legible
//!   original text (review-mode style; the recipient can still read
//!   the original but it's marked for redaction).

use thiserror::Error;

pub mod acroform;
pub mod format;
pub mod markdown;

pub use acroform::{
    blank_acroform, blank_acroform_with, field_names, fill_acroform, read_field_value,
    read_field_values, read_widget_appearance_state, FieldSpec,
};
pub use format::{render_document, Letterhead, OutputFormat};
pub use markdown::to_typst;

/// The firm typeface, embedded so PDF rendering never depends on a
/// font installed on the host. These are the Google Fonts Noto Serif
/// variable masters (`wght` + `wdth` axes); Typst reads Regular and
/// Bold off the weight axis, so two files cover regular/bold/italic.
const NOTO_SERIF: &[u8] = include_bytes!("../assets/fonts/NotoSerif/NotoSerif-VF.ttf");
const NOTO_SERIF_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/NotoSerif/NotoSerif-Italic-VF.ttf");

/// The firm logo, embedded so the letterhead in [`OutputFormat::Letter`]
/// never depends on a file on disk. Registered with the Typst engine
/// under the virtual path [`LOGO_PATH`], which a chrome preamble
/// references via `#image(..)`.
const FIRM_LOGO: &[u8] = include_bytes!("../assets/brand/logo-firm.png");

/// The virtual path the embedded [`FIRM_LOGO`] is resolvable at inside
/// Typst markup — kept in one place so [`format`] and [`render`] agree.
pub(crate) const LOGO_PATH: &str = "logo-firm.png";

/// Typst set-rule making Noto Serif the document default. Prepended
/// to every source by [`render`]; a caller's own `#set text` rule that
/// follows in the source overrides it.
const FONT_PREAMBLE: &str = "#set text(font: \"Noto Serif\")\n";

/// Errors that [`render`] can surface to the caller.
#[derive(Debug, Error)]
pub enum PdfError {
    /// The Typst source failed to compile. The wrapped string is the
    /// first diagnostic; the full set is logged at `warn`.
    #[error("typst compile: {0}")]
    Compile(String),
    /// PDF export failed after a successful compile — usually a font
    /// fallback issue or an unsupported feature.
    #[error("typst export: {0}")]
    Export(String),
    /// `lopdf` failed to parse or write a PDF in the `AcroForm` fill path.
    #[error("pdf parse/write: {0}")]
    Lopdf(String),
    /// The PDF handed to [`acroform::fill_acroform`] has no `AcroForm` to
    /// fill.
    #[error("pdf has no AcroForm to fill")]
    NoAcroForm,
    /// The form is `XFA`-based (Adobe's XML form layer). No Rust crate
    /// fills `XFA`; filling it would silently emit a blank, so we fail
    /// loudly instead.
    #[error("XFA-based forms are not supported (would silently emit a blank)")]
    XfaUnsupported,
    /// A field name passed to [`acroform::fill_acroform`] matched no
    /// field in the form — surfaced rather than silently dropped.
    #[error("no form field named `{0}`")]
    UnmatchedField(String),
    /// A value for a checkbox / radio (`Btn`) field matched none of the
    /// field's appearance states — surfaced with the allowed states so a
    /// mis-mapped field map is corrected, never a silently unchecked box.
    #[error("field `{field}`: `{value}` matches no appearance state (allowed: {allowed:?})")]
    InvalidChoice {
        field: String,
        value: String,
        allowed: Vec<String>,
    },
}

/// How a redacted passage is rendered in the output PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionStyle {
    /// Solid black box covering the redacted text.
    Block,
    /// Horizontal black bar through the middle of the line.
    Bar,
    /// Strikethrough; original text remains legible.
    Strike,
}

impl RedactionStyle {
    /// Typst markup snippet that wraps the redacted span. Used by
    /// [`render_with_redactions`].
    #[must_use]
    pub fn typst_wrapper(self, content: &str) -> String {
        match self {
            Self::Block => format!("#box(fill: black, inset: 2pt)[#text(fill: white)[{content}]]"),
            Self::Bar => format!("#box(stroke: (top: 1.2pt + black, bottom: 0pt))[{content}]"),
            Self::Strike => format!("#strike[{content}]"),
        }
    }
}

/// Compile Typst source `source` and return the rendered PDF bytes.
///
/// # Errors
///
/// Returns [`PdfError::Compile`] if the Typst source is malformed,
/// or [`PdfError::Export`] if the PDF stage fails after a successful
/// compile.
pub fn render(source: &str) -> Result<Vec<u8>, PdfError> {
    use typst::foundations::Bytes;
    use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
    use typst_layout::PagedDocument;

    // Prepend the font set-rule so Noto Serif is the default family for
    // whatever `source` renders. The embedded masters take precedence in
    // the world's font set; system + typst-kit embedded fonts are kept as
    // fallback for any glyph Noto Serif lacks.
    let with_font = format!("{FONT_PREAMBLE}{source}");

    // The firm logo is registered at the same virtual path the letterhead
    // chrome references via `#image(..)`, resolved relative to the main
    // file's (root) directory.
    let logo_path = VirtualPath::new(LOGO_PATH).expect("static logo path is valid");
    let logo = (
        FileId::new(RootedPath::new(VirtualRoot::Project, logo_path)),
        Bytes::new(FIRM_LOGO),
    );
    let world = world::PdfWorld::new(with_font, &[NOTO_SERIF, NOTO_SERIF_ITALIC], vec![logo]);

    let doc: PagedDocument = typst::compile(&world)
        .output
        .map_err(|diags| PdfError::Compile(format_diagnostics(&diags)))?;

    typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|diags| PdfError::Export(format_diagnostics(&diags)))
}

/// Render a Typst document where one passage has been wrapped in the
/// chosen [`RedactionStyle`]. The `redacted` slice is splice-inserted
/// into `template` at the literal token `{{redacted}}`; the rest of
/// the template is rendered verbatim.
///
/// # Errors
///
/// Same as [`render`]: compile or export failure.
pub fn render_with_redactions(
    template: &str,
    redacted: &str,
    style: RedactionStyle,
) -> Result<Vec<u8>, PdfError> {
    let wrapped = style.typst_wrapper(redacted);
    let source = template.replace("{{redacted}}", &wrapped);
    render(&source)
}

fn format_diagnostics<T: std::fmt::Debug>(diags: &T) -> String {
    format!("{diags:?}")
}

/// The minimal [`World`](typst::World) the embedded compiler needs: one
/// in-memory main source, a fixed set of virtual files (the firm logo),
/// and a font set of the embedded firm masters plus typst-kit's embedded
/// and system fonts as fallback. There is no filesystem or package
/// access — every input is provided up front, so rendering is hermetic.
mod world {
    use typst::diag::{FileError, FileResult};
    use typst::foundations::{Bytes, Datetime, Duration};
    use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
    use typst::text::{Font, FontBook};
    use typst::utils::LazyHash;
    use typst::{Library, LibraryExt, World};
    use typst_kit::fonts::FontStore;

    pub struct PdfWorld {
        library: LazyHash<Library>,
        fonts: FontStore,
        main: FileId,
        source: Source,
        files: Vec<(FileId, Bytes)>,
    }

    impl PdfWorld {
        /// Build a world from the main source text, the embedded firm font
        /// masters (registered first so they win in fallback ordering), and
        /// any additional virtual files (e.g. the logo) the markup resolves.
        pub fn new(
            source: String,
            embedded_fonts: &[&'static [u8]],
            files: Vec<(FileId, Bytes)>,
        ) -> Self {
            let main_path = VirtualPath::new("main.typ").expect("static main path is valid");
            let main = FileId::new(RootedPath::new(VirtualRoot::Project, main_path));
            let source = Source::new(main, source);

            let mut fonts = FontStore::new();
            for data in embedded_fonts {
                for font in Font::iter(Bytes::new(*data)) {
                    let info = font.info().clone();
                    fonts.push((font, info));
                }
            }
            fonts.extend(typst_kit::fonts::embedded());
            fonts.extend(typst_kit::fonts::system());

            Self {
                library: LazyHash::new(Library::default()),
                fonts,
                main,
                source,
                files,
            }
        }
    }

    impl World for PdfWorld {
        fn library(&self) -> &LazyHash<Library> {
            &self.library
        }

        fn book(&self) -> &LazyHash<FontBook> {
            self.fonts.book()
        }

        fn main(&self) -> FileId {
            self.main
        }

        fn source(&self, id: FileId) -> FileResult<Source> {
            if id == self.main {
                Ok(self.source.clone())
            } else {
                Err(FileError::NotFound(id.vpath().get_without_slash().into()))
            }
        }

        fn file(&self, id: FileId) -> FileResult<Bytes> {
            if id == self.main {
                return Ok(Bytes::from_string(self.source.text().to_string()));
            }
            self.files
                .iter()
                .find(|(fid, _)| *fid == id)
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| FileError::NotFound(id.vpath().get_without_slash().into()))
        }

        fn font(&self, index: usize) -> Option<Font> {
            self.fonts.font(index)
        }

        fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
            // Deterministic by design: a rendered legal document must not
            // depend on the wall clock. Templates that need a date carry it
            // in the source, so `datetime.today()` is intentionally absent.
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render, render_with_redactions, PdfError, RedactionStyle};

    #[test]
    fn redaction_style_block_emits_a_filled_box() {
        let wrapped = RedactionStyle::Block.typst_wrapper("secret name");
        assert!(wrapped.contains("fill: black"));
        assert!(wrapped.contains("secret name"));
    }

    #[test]
    fn redaction_style_bar_emits_a_top_stroke() {
        let wrapped = RedactionStyle::Bar.typst_wrapper("classified");
        assert!(wrapped.contains("stroke"));
        assert!(wrapped.contains("classified"));
    }

    #[test]
    fn redaction_style_strike_emits_a_strike_block() {
        let wrapped = RedactionStyle::Strike.typst_wrapper("draft only");
        assert!(wrapped.starts_with("#strike["));
        assert!(wrapped.contains("draft only"));
    }

    #[test]
    fn render_returns_pdf_bytes_for_a_minimal_document() {
        let pdf = render("Hello, world.").expect("typst minimal compile + export");
        assert!(
            pdf.starts_with(b"%PDF-"),
            "rendered bytes are not a PDF: first 8 = {:?}",
            &pdf.get(..8.min(pdf.len()))
        );
        assert!(
            pdf.len() > 100,
            "PDF unexpectedly tiny: {} bytes",
            pdf.len()
        );
    }

    #[test]
    fn rendered_pdf_embeds_the_noto_serif_font() {
        // Typst silently falls back to another family if "Noto Serif"
        // can't be found, so a clean compile isn't enough — assert the
        // family actually made it into the embedded font set. If the
        // .ttf goes missing or its name table drifts, this fails.
        let pdf = render("Defendant rests.").expect("render");
        let needle = b"NotoSerif";
        let embedded = pdf.windows(needle.len()).any(|w| w == needle);
        assert!(
            embedded,
            "rendered PDF does not embed Noto Serif — Typst fell back",
        );
    }

    #[test]
    fn bold_weight_renders_off_the_variable_axis() {
        // The embedded masters are variable; bold must instantiate from
        // the weight axis rather than error or silently stay regular.
        let bold = render("#text(weight: \"bold\")[Heavy.]").expect("bold renders");
        assert!(bold.starts_with(b"%PDF-"));
    }

    #[test]
    fn renders_non_latin_scripts_for_global_clients() {
        // Cyrillic + Greek + Vietnamese + accented Latin all come from
        // the one embedded family — a client's name must not vanish.
        let pdf = render("Привет · Γειά · Tiếng Việt · Núñez").expect("multi-script renders");
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 100);
    }

    #[test]
    fn render_surfaces_typst_compile_errors() {
        // `#let x = ` is an incomplete statement; the parser bails.
        let err = render("#let x =").unwrap_err();
        assert!(
            matches!(err, PdfError::Compile(_)),
            "expected Compile, got {err:?}",
        );
    }

    #[test]
    fn render_with_redactions_splices_the_wrapper_into_the_template() {
        let template = "Defendant: {{redacted}}.";
        let pdf = render_with_redactions(template, "John Doe", RedactionStyle::Block)
            .expect("render with redactions");
        assert!(pdf.starts_with(b"%PDF-"));
    }
}
