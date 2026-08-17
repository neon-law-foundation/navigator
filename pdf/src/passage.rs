// PDF's own vocabulary for a transform is `[a b c d e f]` and for a
// glyph's advance is `w0`; renaming those to satisfy the naming lints
// would make this module harder to check against the specification, not
// easier.
#![allow(clippy::many_single_char_names, clippy::similar_names)]
//! Locating a quoted passage inside a stored PDF and pinning it to a
//! **normalised rectangle** — the evidence half of the citation
//! apparatus (#893).
//!
//! A citation is checkable when the reviewer can see the source page
//! with the relied-on passage marked. That takes two artifacts, and this
//! module produces both:
//!
//! 1. [`page_render`] — one page of a stored PDF lifted out as a
//!    self-contained single-page PDF, the cacheable thing a viewer
//!    displays.
//! 2. [`locate`] — the quote's position on that page as a
//!    [`NormalisedRect`]: fractions of the page box, never pixels, so
//!    one rendering serves every viewport and every zoom level.
//!
//! Splitting them is deliberate. The rendering is per page and
//! cacheable; the rect is per passage. Several passages of the same page
//! share one rendering, and the mark is composed by whatever displays it
//! rather than baked into the bytes.
//!
//! # Any stored PDF, including our own drafts
//!
//! The input is any PDF with a text layer — a vendored court filing, an
//! exhibit, or a draft this workspace rendered through
//! [`crate::render`] / [`crate::pleading`]. That both ends can be
//! located by the same pipeline is what lets a verification pin the
//! *draft passage* and the *source region* it rests on.
//!
//! Typst's PDFs are Type0/`Identity-H` with subset fonts, so their
//! text-showing operators carry glyph ids rather than Unicode — which is
//! why [`crate::page_text`] returns mojibake for them. This module reads
//! the font's `/ToUnicode` `CMap` instead, which Typst does emit, and
//! so sees real text where the naive extractor cannot.
//!
//! # Fail closed
//!
//! A highlight at the wrong coordinates asserts that someone checked
//! something they did not, so every uncertainty is an error rather than
//! a guess:
//!
//! - **Quote not found** — [`PassageError::QuoteNotFound`]. A scanned,
//!   image-only PDF has no text layer and a transcribed quote drifts
//!   from its source; both land here rather than at `0,0`.
//! - **Quote spans a page break** — the returned [`PassageLocation`]
//!   carries one rect per line per page, so a passage broken across two
//!   pages comes back as two rects with different
//!   [`PassageRect::page_index`], never one rect spanning nothing.
//! - **Quote appears more than once** — [`locate`] takes a 1-based
//!   `ordinal` and reports [`PassageLocation::occurrences`], so which
//!   occurrence was pinned is recorded rather than assumed. An ordinal
//!   past the end is [`PassageError::OrdinalOutOfRange`], not a silent
//!   clamp to the first.
//! - **Unmeasurable font** — a font declaring no glyph widths cannot
//!   yield a horizontal extent, so it is
//!   [`PassageError::UnmeasurableFont`] rather than an approximate box.
//!
//! # What it does not model
//!
//! Text drawn inside a form `XObject`, vertical writing mode, and
//! multi-column reading order. Runs are ordered top-to-bottom then
//! left-to-right within a line, which is right for single-column court
//! paper and wrong for a two-column brief. Only whitespace is
//! normalised when matching — a quote whose words drift from the source
//! fails loudly, which is the point.

use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// Everything [`locate`] and [`page_render`] refuse to guess at.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PassageError {
    /// The bytes are not a parseable PDF, or a page is malformed.
    #[error("pdf parse: {0}")]
    Pdf(String),
    /// The quote normalised to nothing — there is no passage to locate.
    #[error("the quote is empty")]
    EmptyQuote,
    /// The quote does not appear in the document's text layer. Either
    /// the PDF is image-only, or the quote has drifted from its source.
    #[error("quote not found in the document's text layer: `{quote}`")]
    QuoteNotFound { quote: String },
    /// The quote appears, but fewer times than the requested occurrence.
    #[error("quote `{quote}`: asked for occurrence {ordinal} of {occurrences}")]
    OrdinalOutOfRange {
        quote: String,
        ordinal: usize,
        occurrences: usize,
    },
    /// A page index past the end of the document.
    #[error("page {page_index} requested, document has {pages}")]
    PageOutOfRange { page_index: usize, pages: usize },
    /// A font on the page declares no glyph widths, so no horizontal
    /// extent can be measured for text set in it.
    #[error("font `{font}` declares no glyph widths, so no rect can be measured")]
    UnmeasurableFont { font: String },
}

/// A rectangle as **fractions of the page box**, origin at the page's
/// top-left and `y` growing downward — the browser's convention, not
/// PDF user space's. Every component is in `0.0..=1.0`, so the rect
/// survives any rendering resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalisedRect {
    /// Distance from the page's left edge, as a fraction of page width.
    pub x: f64,
    /// Distance from the page's top edge, as a fraction of page height.
    pub y: f64,
    /// Width as a fraction of page width.
    pub width: f64,
    /// Height as a fraction of page height.
    pub height: f64,
}

/// One marked region: which page, and where on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassageRect {
    /// Zero-based page index, matching [`page_render`]'s argument.
    pub page_index: usize,
    /// The region, normalised against that page's box.
    pub rect: NormalisedRect,
}

/// Where a quote sits, and which occurrence of it was pinned.
#[derive(Debug, Clone, PartialEq)]
pub struct PassageLocation {
    /// The 1-based occurrence this location describes.
    pub ordinal: usize,
    /// How many times the quote appears in the whole document. Recorded
    /// so a citation to a repeated passage says which one it means.
    pub occurrences: usize,
    /// One rect per line the passage covers. A passage broken across a
    /// page break yields rects with differing
    /// [`PassageRect::page_index`].
    pub rects: Vec<PassageRect>,
}

impl PassageLocation {
    /// The pages this passage touches, in order, without repeats.
    #[must_use]
    pub fn pages(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for r in &self.rects {
            if out.last() != Some(&r.page_index) {
                out.push(r.page_index);
            }
        }
        out
    }

    /// Whether the passage is broken across a page break.
    #[must_use]
    pub fn spans_page_break(&self) -> bool {
        self.pages().len() > 1
    }
}

/// How many pages `pdf` has.
///
/// # Errors
/// [`PassageError::Pdf`] if the bytes are not a parseable PDF.
pub fn page_count(pdf: &[u8]) -> Result<usize, PassageError> {
    Ok(load(pdf)?.get_pages().len())
}

/// How many times `quote` appears in the document's text layer.
/// Whitespace is normalised on both sides; nothing else is.
///
/// # Errors
/// [`PassageError::Pdf`], [`PassageError::EmptyQuote`], or
/// [`PassageError::UnmeasurableFont`].
pub fn occurrence_count(pdf: &[u8], quote: &str) -> Result<usize, PassageError> {
    let needle = normalise(quote);
    if needle.is_empty() {
        return Err(PassageError::EmptyQuote);
    }
    let doc = extract(&load(pdf)?)?;
    Ok(doc.matches(&needle).len())
}

/// Locate the `ordinal`-th (1-based) occurrence of `quote` and return
/// its normalised rects.
///
/// # Errors
/// [`PassageError::EmptyQuote`] for a blank quote,
/// [`PassageError::QuoteNotFound`] when the text layer does not carry
/// it, [`PassageError::OrdinalOutOfRange`] when it carries fewer
/// occurrences than asked for, and [`PassageError::UnmeasurableFont`]
/// when a font on the page has no widths to measure with.
pub fn locate(pdf: &[u8], quote: &str, ordinal: usize) -> Result<PassageLocation, PassageError> {
    let needle = normalise(quote);
    if needle.is_empty() {
        return Err(PassageError::EmptyQuote);
    }
    let doc = extract(&load(pdf)?)?;
    let hits = doc.matches(&needle);
    let occurrences = hits.len();
    if occurrences == 0 {
        return Err(PassageError::QuoteNotFound {
            quote: needle.clone(),
        });
    }
    if ordinal == 0 || ordinal > occurrences {
        return Err(PassageError::OrdinalOutOfRange {
            quote: needle,
            ordinal,
            occurrences,
        });
    }
    let start = hits[ordinal - 1];
    Ok(PassageLocation {
        ordinal,
        occurrences,
        rects: doc.rects(start, needle.chars().count()),
    })
}

/// Lift page `page_index` (zero-based) out of `pdf` as a self-contained
/// single-page PDF, carrying only the objects that page reaches.
///
/// This is the "page render" the reviewer looks at. It stays a PDF
/// rather than a raster: the page keeps its text layer, prints at any
/// resolution, and the [`NormalisedRect`] overlays it exactly, with no
/// native rasteriser in the dependency tree.
///
/// # Errors
/// [`PassageError::PageOutOfRange`] past the end, [`PassageError::Pdf`]
/// on a malformed document.
pub fn page_render(pdf: &[u8], page_index: usize) -> Result<Vec<u8>, PassageError> {
    let doc = load(pdf)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let page_id = *pages.get(page_index).ok_or(PassageError::PageOutOfRange {
        page_index,
        pages: pages.len(),
    })?;
    extract_single_page(&doc, page_id)
}

fn load(pdf: &[u8]) -> Result<Document, PassageError> {
    Document::load_mem(pdf).map_err(|e| PassageError::Pdf(e.to_string()))
}

// ---------------------------------------------------------------------
// Extracted text
// ---------------------------------------------------------------------

/// One glyph's box in PDF user space (y grows upward), tagged with the
/// page and visual line it belongs to.
#[derive(Debug, Clone)]
struct Glyph {
    text: String,
    page: usize,
    line: usize,
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
}

/// A page's box, for normalising against.
#[derive(Debug, Clone, Copy)]
struct PageBox {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl PageBox {
    fn width(self) -> f64 {
        (self.x1 - self.x0).abs().max(f64::EPSILON)
    }
    fn height(self) -> f64 {
        (self.y1 - self.y0).abs().max(f64::EPSILON)
    }
}

/// The whole document's text layer: the whitespace-normalised string,
/// and for each of its chars the glyph it came from (`None` for a
/// separator this module inserted).
struct ExtractedText {
    chars: Vec<char>,
    origin: Vec<Option<usize>>,
    glyphs: Vec<Glyph>,
    boxes: Vec<PageBox>,
}

impl ExtractedText {
    /// Every start index at which `needle` occurs.
    fn matches(&self, needle: &str) -> Vec<usize> {
        let needle: Vec<char> = needle.chars().collect();
        if needle.is_empty() || needle.len() > self.chars.len() {
            return Vec::new();
        }
        (0..=self.chars.len() - needle.len())
            .filter(|&i| self.chars[i..i + needle.len()] == needle[..])
            .collect()
    }

    /// The rects covering `len` chars from `start`: one per (page, line)
    /// the matched glyphs fall on, in reading order.
    fn rects(&self, start: usize, len: usize) -> Vec<PassageRect> {
        let mut out: Vec<PassageRect> = Vec::new();
        let mut current: Option<(usize, usize, f64, f64, f64, f64)> = None;
        for idx in self.origin[start..(start + len).min(self.origin.len())]
            .iter()
            .flatten()
        {
            let g = &self.glyphs[*idx];
            match &mut current {
                Some((page, line, x0, y0, x1, y1)) if *page == g.page && *line == g.line => {
                    *x0 = x0.min(g.x0);
                    *x1 = x1.max(g.x1);
                    *y0 = y0.min(g.bottom);
                    *y1 = y1.max(g.top);
                }
                slot => {
                    if let Some((page, _, x0, y0, x1, y1)) = *slot {
                        out.push(self.normalise_rect(page, x0, y0, x1, y1));
                    }
                    *slot = Some((g.page, g.line, g.x0, g.bottom, g.x1, g.top));
                }
            }
        }
        if let Some((page, _, x0, y0, x1, y1)) = current {
            out.push(self.normalise_rect(page, x0, y0, x1, y1));
        }
        out
    }

    fn normalise_rect(&self, page: usize, x0: f64, y0: f64, x1: f64, y1: f64) -> PassageRect {
        let b = self.boxes[page];
        let clamp = |v: f64| v.clamp(0.0, 1.0);
        let nx = clamp((x0 - b.x0.min(b.x1)) / b.width());
        let ny = clamp((b.y0.max(b.y1) - y1) / b.height());
        PassageRect {
            page_index: page,
            rect: NormalisedRect {
                x: nx,
                y: ny,
                width: clamp((x1 - x0) / b.width()).min(1.0 - nx),
                height: clamp((y1 - y0) / b.height()).min(1.0 - ny),
            },
        }
    }
}

/// Collapse every whitespace run to a single space and trim. The only
/// normalisation applied to either side of a match: a quote whose
/// *words* drift from the source must fail, not be fuzzily matched.
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract(doc: &Document) -> Result<ExtractedText, PassageError> {
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut boxes: Vec<PageBox> = Vec::new();
    let mut line_counter = 0usize;

    for (page_index, page_id) in doc.get_pages().into_values().enumerate() {
        boxes.push(media_box(doc, page_id));
        let fonts = page_fonts(doc, page_id);
        let runs = walk_page(doc, page_id, &fonts)?;
        for line in into_lines(runs) {
            line_counter += 1;
            for mut g in line {
                g.page = page_index;
                g.line = line_counter;
                glyphs.push(g);
            }
        }
    }

    // Build the flat string, inserting a separator where the geometry
    // says one belongs: always between lines, and inside a line wherever
    // the horizontal gap is wide enough to read as a space.
    let mut chars: Vec<char> = Vec::new();
    let mut origin: Vec<Option<usize>> = Vec::new();
    let mut prev: Option<&Glyph> = None;
    for (idx, g) in glyphs.iter().enumerate() {
        let separate = match prev {
            None => false,
            Some(p) => {
                p.line != g.line || g.x0 - p.x1 > 0.2 * (p.top - p.bottom).max(g.top - g.bottom)
            }
        };
        if separate {
            chars.push(' ');
            origin.push(None);
        }
        for c in g.text.chars() {
            chars.push(c);
            origin.push(Some(idx));
        }
        prev = Some(g);
    }

    // Normalise the assembled string the same way the quote is, keeping
    // the origin map aligned char for char.
    let mut norm_chars: Vec<char> = Vec::new();
    let mut norm_origin: Vec<Option<usize>> = Vec::new();
    let mut pending_space = false;
    for (c, o) in chars.into_iter().zip(origin) {
        if c.is_whitespace() {
            pending_space = !norm_chars.is_empty();
            continue;
        }
        if pending_space {
            norm_chars.push(' ');
            norm_origin.push(None);
            pending_space = false;
        }
        norm_chars.push(c);
        norm_origin.push(o);
    }

    Ok(ExtractedText {
        chars: norm_chars,
        origin: norm_origin,
        glyphs,
        boxes,
    })
}

/// Group a page's runs into visual lines: top to bottom by baseline,
/// left to right within a line.
fn into_lines(mut runs: Vec<Run>) -> Vec<Vec<Glyph>> {
    runs.sort_by(|a, b| {
        b.baseline
            .partial_cmp(&a.baseline)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut lines: Vec<Vec<Run>> = Vec::new();
    for run in runs {
        match lines.last_mut() {
            Some(line) if (line[0].baseline - run.baseline).abs() <= run.tolerance => {
                line.push(run);
            }
            _ => lines.push(vec![run]),
        }
    }
    lines
        .into_iter()
        .map(|mut line| {
            line.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
            line.into_iter().flat_map(|r| r.glyphs).collect()
        })
        .collect()
}

fn media_box(doc: &Document, page_id: ObjectId) -> PageBox {
    let mut node = doc.get_dictionary(page_id).ok().cloned();
    while let Some(dict) = node {
        if let Some(vals) = dict.get(b"MediaBox").ok().and_then(|o| numbers(doc, o)) {
            if vals.len() >= 4 {
                return PageBox {
                    x0: vals[0],
                    y0: vals[1],
                    x1: vals[2],
                    y1: vals[3],
                };
            }
        }
        node = dict
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok()
            .and_then(|id| doc.get_dictionary(id).ok().cloned());
    }
    // US Letter, the paper this workspace's own renderer cuts to.
    PageBox {
        x0: 0.0,
        y0: 0.0,
        x1: 612.0,
        y1: 792.0,
    }
}

fn numbers(doc: &Document, obj: &Object) -> Option<Vec<f64>> {
    let obj = match obj {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    Some(obj.as_array().ok()?.iter().filter_map(number).collect())
}

fn number(obj: &Object) -> Option<f64> {
    match obj {
        // Page coordinates and glyph widths sit far inside the range an
        // `i32` holds exactly, so the narrowing conversion is lossless
        // for every value a real PDF carries here.
        Object::Integer(i) => i32::try_from(*i).ok().map(f64::from),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

/// A PDF integer — character codes, `/FirstChar`, CID range bounds. The
/// specification makes each of these an integer, so a real operand is
/// never read through a float here.
fn integer(obj: &Object) -> Option<i64> {
    match obj {
        Object::Integer(i) => Some(*i),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// Content-stream walk
// ---------------------------------------------------------------------

/// A 2-D affine transform, PDF's `[a b c d e f]` row order.
#[derive(Debug, Clone, Copy)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` applied first, then `other`.
    fn then(self, other: Self) -> Self {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Self([
            a1 * a2 + b1 * c2,
            a1 * b2 + b1 * d2,
            c1 * a2 + d1 * c2,
            c1 * b2 + d1 * d2,
            e1 * a2 + f1 * c2 + e2,
            e1 * b2 + f1 * d2 + f2,
        ])
    }

    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn translate(tx: f64, ty: f64) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, tx, ty])
    }
}

/// One text-showing operation's glyphs, kept together so lines can be
/// assembled from whole runs rather than loose glyphs.
struct Run {
    baseline: f64,
    x0: f64,
    tolerance: f64,
    glyphs: Vec<Glyph>,
}

#[derive(Clone)]
struct TextState {
    font: Option<String>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn walk_page(
    doc: &Document,
    page_id: ObjectId,
    fonts: &BTreeMap<String, Font>,
) -> Result<Vec<Run>, PassageError> {
    let data = doc.get_page_content(page_id);
    let Ok(content) = Content::decode(&data) else {
        return Ok(Vec::new());
    };

    let mut runs: Vec<Run> = Vec::new();
    let mut ctm = Matrix::IDENTITY;
    let mut ctm_stack: Vec<Matrix> = Vec::new();
    let mut ts = TextState::default();
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;

    for op in content.operations {
        let nums: Vec<f64> = op.operands.iter().filter_map(number).collect();
        match op.operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => ctm = ctm_stack.pop().unwrap_or(Matrix::IDENTITY),
            "cm" if nums.len() >= 6 => {
                ctm = Matrix([nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]]).then(ctm);
            }
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
            }
            "Tf" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    ts.font = Some(String::from_utf8_lossy(name).into_owned());
                }
                ts.size = nums.last().copied().unwrap_or(ts.size);
            }
            "Tc" if !nums.is_empty() => ts.char_spacing = nums[0],
            "Tw" if !nums.is_empty() => ts.word_spacing = nums[0],
            "Tz" if !nums.is_empty() => ts.h_scale = nums[0] / 100.0,
            "TL" if !nums.is_empty() => ts.leading = nums[0],
            "Ts" if !nums.is_empty() => ts.rise = nums[0],
            "Td" if nums.len() >= 2 => {
                tlm = Matrix::translate(nums[0], nums[1]).then(tlm);
                tm = tlm;
            }
            "TD" if nums.len() >= 2 => {
                ts.leading = -nums[1];
                tlm = Matrix::translate(nums[0], nums[1]).then(tlm);
                tm = tlm;
            }
            "Tm" if nums.len() >= 6 => {
                tlm = Matrix([nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]]);
                tm = tlm;
            }
            "T*" => {
                tlm = Matrix::translate(0.0, -ts.leading).then(tlm);
                tm = tlm;
            }
            "Tj" | "'" | "\"" => {
                if op.operator != "Tj" {
                    if op.operator == "\"" && nums.len() >= 2 {
                        ts.word_spacing = nums[0];
                        ts.char_spacing = nums[1];
                    }
                    tlm = Matrix::translate(0.0, -ts.leading).then(tlm);
                    tm = tlm;
                }
                if let Some(Object::String(bytes, _)) =
                    op.operands.iter().find(|o| matches!(o, Object::String(..)))
                {
                    let run = show(bytes, &ts, fonts, &mut tm, ctm)?;
                    push_run(&mut runs, run);
                }
            }
            "TJ" => {
                let Some(Object::Array(items)) = op.operands.first() else {
                    continue;
                };
                let mut glyphs: Vec<Glyph> = Vec::new();
                let mut start: Option<(f64, f64, f64)> = None;
                for item in items {
                    match item {
                        Object::String(bytes, _) => {
                            let run = show(bytes, &ts, fonts, &mut tm, ctm)?;
                            if start.is_none() {
                                start = Some((run.baseline, run.x0, run.tolerance));
                            }
                            glyphs.extend(run.glyphs);
                        }
                        other => {
                            if let Some(adj) = number(other) {
                                let tx = -adj / 1000.0 * ts.size * ts.h_scale;
                                tm = Matrix::translate(tx, 0.0).then(tm);
                            }
                        }
                    }
                }
                if let Some((baseline, x0, tolerance)) = start {
                    push_run(
                        &mut runs,
                        Run {
                            baseline,
                            x0,
                            tolerance,
                            glyphs,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    Ok(runs)
}

fn push_run(runs: &mut Vec<Run>, run: Run) {
    if !run.glyphs.is_empty() {
        runs.push(run);
    }
}

/// Lay out one shown string, advancing the text matrix as it goes.
fn show(
    bytes: &[u8],
    ts: &TextState,
    fonts: &BTreeMap<String, Font>,
    tm: &mut Matrix,
    ctm: Matrix,
) -> Result<Run, PassageError> {
    let font = ts.font.as_ref().and_then(|n| fonts.get(n));
    let (origin_x, origin_y) = tm.then(ctm).apply(0.0, 0.0);
    let mut run = Run {
        baseline: origin_y,
        x0: origin_x,
        tolerance: (ts.size * 0.5).max(1.0),
        glyphs: Vec::new(),
    };
    let Some(font) = font else {
        return Ok(run);
    };

    for (code, text, is_single_space) in font.decode(bytes) {
        let w0 = font.width(code)?;
        let advance = (w0 * ts.size
            + ts.char_spacing
            + if is_single_space {
                ts.word_spacing
            } else {
                0.0
            })
            * ts.h_scale;
        let trm = tm.then(ctm);
        let (x0, _) = trm.apply(0.0, ts.rise);
        let (x1, _) = trm.apply(w0 * ts.size * ts.h_scale, ts.rise);
        let (_, top) = trm.apply(0.0, ts.rise + font.ascent * ts.size);
        let (_, bottom) = trm.apply(0.0, ts.rise + font.descent * ts.size);
        if !text.is_empty() {
            run.glyphs.push(Glyph {
                text,
                page: 0,
                line: 0,
                x0: x0.min(x1),
                x1: x0.max(x1),
                top: top.max(bottom),
                bottom: top.min(bottom),
            });
        }
        *tm = Matrix::translate(advance, 0.0).then(*tm);
    }
    Ok(run)
}

// ---------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------

/// How many bytes one character code takes in a shown string.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeWidth {
    /// Simple fonts (`Type1`, `TrueType`, `Type3`): one byte per code.
    Single,
    /// Composite fonts through `Identity-H`: two bytes, big-endian.
    Double,
}

/// Where a code's advance width comes from.
enum Widths {
    /// A simple font's `/FirstChar` + `/Widths`, in 1/1000 em.
    Simple { first_char: u32, widths: Vec<f64> },
    /// A CID font's `/W` map with its `/DW` default, in 1/1000 em.
    Cid {
        default: f64,
        widths: BTreeMap<u32, f64>,
    },
    /// No width source at all — every lookup fails loudly.
    None,
}

struct Font {
    name: String,
    codes: CodeWidth,
    widths: Widths,
    to_unicode: BTreeMap<u32, String>,
    /// Em-relative, from `/FontDescriptor`.
    ascent: f64,
    descent: f64,
}

impl Font {
    /// Split a shown string into `(code, text, is_single_byte_space)`.
    fn decode(&self, bytes: &[u8]) -> Vec<(u32, String, bool)> {
        match self.codes {
            CodeWidth::Single => bytes
                .iter()
                .map(|&b| {
                    let code = u32::from(b);
                    (code, self.text_for(code, char::from(b)), b == b' ')
                })
                .collect(),
            CodeWidth::Double => bytes
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| {
                    let code = u32::from(u16::from_be_bytes([c[0], c[1]]));
                    (code, self.text_for(code, '\u{FFFD}'), false)
                })
                .collect(),
        }
    }

    /// The Unicode a code stands for: `/ToUnicode` when the font carries
    /// one, else the fallback the caller derived from the raw byte. A
    /// code with neither becomes U+FFFD, which never matches a quote —
    /// unlocatable rather than located wrongly.
    fn text_for(&self, code: u32, fallback: char) -> String {
        self.to_unicode
            .get(&code)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Advance width in em units.
    fn width(&self, code: u32) -> Result<f64, PassageError> {
        match &self.widths {
            Widths::Simple { first_char, widths } => code
                .checked_sub(*first_char)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| widths.get(i))
                .map(|w| w / 1000.0)
                .ok_or_else(|| PassageError::UnmeasurableFont {
                    font: self.name.clone(),
                }),
            Widths::Cid { default, widths } => {
                Ok(widths.get(&code).copied().unwrap_or(*default) / 1000.0)
            }
            Widths::None => Err(PassageError::UnmeasurableFont {
                font: self.name.clone(),
            }),
        }
    }
}

/// Every font the page's resource tree reaches, keyed by the resource
/// name `Tf` names it by. A page whose resources cannot be read yields
/// none, which makes its text unlocatable rather than mis-measured.
fn page_fonts(doc: &Document, page_id: ObjectId) -> BTreeMap<String, Font> {
    let mut out = BTreeMap::new();
    let Ok(fonts) = doc.get_page_fonts(page_id) else {
        return out;
    };
    for (name, dict) in fonts {
        let key = String::from_utf8_lossy(&name).into_owned();
        out.insert(key.clone(), read_font(doc, &key, dict));
    }
    out
}

fn read_font(doc: &Document, resource_name: &str, dict: &Dictionary) -> Font {
    let base = dict.get(b"BaseFont").and_then(Object::as_name).map_or_else(
        |_| resource_name.to_string(),
        |n| String::from_utf8_lossy(n).into_owned(),
    );
    let to_unicode = dict
        .get(b"ToUnicode")
        .and_then(Object::as_reference)
        .ok()
        .and_then(|id| doc.get_object(id).and_then(Object::as_stream).ok())
        .map(|s| {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            parse_to_unicode(&data)
        })
        .unwrap_or_default();

    let is_type0 = matches!(
        dict.get(b"Subtype").and_then(Object::as_name),
        Ok(n) if n == b"Type0"
    );
    if is_type0 {
        let descendant = dict
            .get(b"DescendantFonts")
            .ok()
            .and_then(|o| resolve(doc, o).cloned())
            .and_then(|o| o.as_array().ok().and_then(|a| a.first()).cloned())
            .and_then(|o| resolve_owned(doc, &o))
            .and_then(|o| o.as_dict().ok().cloned());
        let (widths, ascent, descent) = descendant.as_ref().map_or_else(
            || (Widths::None, 0.75, -0.25),
            |d| {
                let default = d.get(b"DW").ok().and_then(number).unwrap_or(1000.0);
                let widths = d
                    .get(b"W")
                    .ok()
                    .and_then(|o| resolve(doc, o).cloned())
                    .and_then(|o| o.as_array().ok().cloned())
                    .map(|a| parse_cid_widths(doc, &a))
                    .unwrap_or_default();
                let (a, dsc) = vertical_metrics(doc, d);
                (Widths::Cid { default, widths }, a, dsc)
            },
        );
        return Font {
            name: base,
            codes: CodeWidth::Double,
            widths,
            to_unicode,
            ascent,
            descent,
        };
    }

    let first_char = dict
        .get(b"FirstChar")
        .ok()
        .and_then(integer)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0);
    let widths = dict
        .get(b"Widths")
        .ok()
        .and_then(|o| resolve(doc, o).cloned())
        .and_then(|o| o.as_array().ok().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|o| resolve(doc, o).and_then(number))
                .collect::<Vec<_>>()
        });
    let (ascent, descent) = vertical_metrics(doc, dict);
    Font {
        name: base,
        codes: CodeWidth::Single,
        widths: widths.map_or(Widths::None, |widths| Widths::Simple { first_char, widths }),
        to_unicode,
        ascent,
        descent,
    }
}

/// `/Ascent` and `/Descent` off the font descriptor, em-relative. A font
/// with no descriptor falls back to a conventional 0.75 / -0.25 line
/// box: vertical slack widens the mark slightly, where a wrong
/// *horizontal* extent would point at the wrong words.
fn vertical_metrics(doc: &Document, dict: &Dictionary) -> (f64, f64) {
    let descriptor = dict
        .get(b"FontDescriptor")
        .ok()
        .and_then(|o| resolve(doc, o).cloned())
        .and_then(|o| o.as_dict().ok().cloned());
    let Some(d) = descriptor else {
        return (0.75, -0.25);
    };
    let ascent = d.get(b"Ascent").ok().and_then(number).unwrap_or(750.0) / 1000.0;
    let descent = d.get(b"Descent").ok().and_then(number).unwrap_or(-250.0) / 1000.0;
    (ascent, descent.min(0.0))
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    }
}

fn resolve_owned(doc: &Document, obj: &Object) -> Option<Object> {
    resolve(doc, obj).cloned()
}

/// Parse a CID font's `/W` array: alternating `c [w …]` runs and
/// `c_first c_last w` ranges.
fn parse_cid_widths(doc: &Document, arr: &[Object]) -> BTreeMap<u32, f64> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < arr.len() {
        let Some(first) = resolve(doc, &arr[i]).and_then(integer) else {
            i += 1;
            continue;
        };
        let Some(next) = arr.get(i + 1).and_then(|o| resolve(doc, o)) else {
            break;
        };
        if let Ok(list) = next.as_array() {
            for (n, w) in list
                .iter()
                .filter_map(|o| resolve(doc, o))
                .filter_map(number)
                .enumerate()
            {
                if let Some(code) = i64::try_from(n)
                    .ok()
                    .and_then(|n| first.checked_add(n))
                    .and_then(|c| u32::try_from(c).ok())
                {
                    out.insert(code, w);
                }
            }
            i += 2;
        } else if let (Some(last), Some(w)) = (
            integer(next),
            arr.get(i + 2)
                .and_then(|o| resolve(doc, o))
                .and_then(number),
        ) {
            for code in first..=last.min(first + 65_535) {
                if let Ok(code) = u32::try_from(code) {
                    out.insert(code, w);
                }
            }
            i += 3;
        } else {
            i += 2;
        }
    }
    out
}

/// Parse the `bfchar` / `bfrange` sections of a `/ToUnicode` `CMap`.
/// The `CMap` is `PostScript`, but only these two section shapes carry
/// the code → Unicode mapping, so the parser reads hex tokens rather
/// than interpreting the language.
fn parse_to_unicode(data: &[u8]) -> BTreeMap<u32, String> {
    let text = String::from_utf8_lossy(data);
    let mut out = BTreeMap::new();
    for (open, close, is_range) in [
        ("beginbfchar", "endbfchar", false),
        ("beginbfrange", "endbfrange", true),
    ] {
        let mut rest = text.as_ref();
        while let Some(start) = rest.find(open) {
            let body_start = start + open.len();
            let Some(end) = rest[body_start..].find(close) else {
                break;
            };
            let body = &rest[body_start..body_start + end];
            if is_range {
                parse_bfrange(body, &mut out);
            } else {
                parse_bfchar(body, &mut out);
            }
            rest = &rest[body_start + end + close.len()..];
        }
    }
    out
}

/// The tokens of a `CMap` section: `<hex>` strings and `[` / `]`.
fn cmap_tokens(body: &str) -> Vec<CmapToken> {
    let mut out = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '<' => {
                let mut hex = String::new();
                for (_, c) in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                    if c.is_ascii_hexdigit() {
                        hex.push(c);
                    }
                }
                out.push(CmapToken::Hex(hex));
            }
            '[' => out.push(CmapToken::Open),
            ']' => out.push(CmapToken::Close),
            _ => {
                let _ = i;
            }
        }
    }
    out
}

enum CmapToken {
    Hex(String),
    Open,
    Close,
}

fn hex_code(hex: &str) -> Option<u32> {
    u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() <= 8)
}

/// A destination hex string is UTF-16BE, and may hold several code
/// units — a ligature maps one code to `"ff"`.
fn hex_text(hex: &str) -> String {
    let units: Vec<u16> = hex
        .as_bytes()
        .chunks(4)
        .filter(|c| c.len() == 4)
        .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .collect();
    String::from_utf16_lossy(&units)
}

fn parse_bfchar(body: &str, out: &mut BTreeMap<u32, String>) {
    let tokens = cmap_tokens(body);
    let mut i = 0;
    while i + 1 < tokens.len() {
        if let (CmapToken::Hex(src), CmapToken::Hex(dst)) = (&tokens[i], &tokens[i + 1]) {
            if let Some(code) = hex_code(src) {
                out.insert(code, hex_text(dst));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
}

fn parse_bfrange(body: &str, out: &mut BTreeMap<u32, String>) {
    let tokens = cmap_tokens(body);
    let mut i = 0;
    while i + 2 < tokens.len() {
        let (CmapToken::Hex(lo), CmapToken::Hex(hi)) = (&tokens[i], &tokens[i + 1]) else {
            i += 1;
            continue;
        };
        let (Some(lo), Some(hi)) = (hex_code(lo), hex_code(hi)) else {
            i += 2;
            continue;
        };
        match &tokens[i + 2] {
            CmapToken::Hex(dst) => {
                // `<lo> <hi> <dst>` — consecutive codes map to
                // consecutive scalars from `dst`.
                let text = hex_text(dst);
                let mut chars: Vec<char> = text.chars().collect();
                for (n, code) in (lo..=hi.min(lo + 65_535)).enumerate() {
                    if let Some(last) = chars.last_mut() {
                        *last = char::from_u32(u32::from(*last) + u32::try_from(n).unwrap_or(0))
                            .unwrap_or(*last);
                    }
                    out.insert(code, chars.iter().collect());
                    if let Some(last) = chars.last_mut() {
                        *last = char::from_u32(u32::from(*last) - u32::try_from(n).unwrap_or(0))
                            .unwrap_or(*last);
                    }
                }
                i += 3;
            }
            CmapToken::Open => {
                // `<lo> <hi> [ <d0> <d1> … ]` — one destination each.
                let mut code = lo;
                let mut j = i + 3;
                while j < tokens.len() {
                    match &tokens[j] {
                        CmapToken::Hex(dst) => {
                            if code <= hi {
                                out.insert(code, hex_text(dst));
                            }
                            code += 1;
                            j += 1;
                        }
                        CmapToken::Close => {
                            j += 1;
                            break;
                        }
                        CmapToken::Open => j += 1,
                    }
                }
                i = j;
            }
            CmapToken::Close => i += 3,
        }
    }
}

// ---------------------------------------------------------------------
// Single-page extraction
// ---------------------------------------------------------------------

/// Copy `page_id` and everything it reaches into a fresh document with
/// its own catalog and one-page tree. `/Parent` is not followed, so the
/// original page tree — and every other page — stays behind.
fn extract_single_page(doc: &Document, page_id: ObjectId) -> Result<Vec<u8>, PassageError> {
    let mut out = Document::with_version(doc.version.clone());
    let pages_id = out.new_object_id();
    let mut mapping: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();

    let page_dict = doc
        .get_dictionary(page_id)
        .map_err(|e| PassageError::Pdf(e.to_string()))?;
    let media = inherited(doc, page_dict, b"MediaBox");
    let resources = inherited(doc, page_dict, b"Resources");
    let rotate = inherited(doc, page_dict, b"Rotate");

    let mut new_page = Dictionary::new();
    for (key, value) in page_dict {
        if key.as_slice() == b"Parent" {
            continue;
        }
        new_page.set(key.clone(), copy_object(doc, &mut out, value, &mut mapping));
    }
    if let Some(res) = resources {
        if !new_page.has(b"Resources") {
            new_page.set("Resources", copy_object(doc, &mut out, &res, &mut mapping));
        }
    }
    if let Some(rot) = rotate {
        if !new_page.has(b"Rotate") {
            new_page.set("Rotate", rot);
        }
    }
    // Materialise the box the page inherited, so the lifted page keeps
    // exactly the geometry every rect was normalised against.
    if let Some(media) = media {
        new_page.set("MediaBox", copy_object(doc, &mut out, &media, &mut mapping));
    }
    new_page.set("Type", Object::Name(b"Page".to_vec()));
    new_page.set("Parent", Object::Reference(pages_id));

    let new_page_id = out.add_object(Object::Dictionary(new_page));
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", vec![Object::Reference(new_page_id)]);
    pages.set("Count", 1);
    out.objects.insert(pages_id, Object::Dictionary(pages));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = out.add_object(Object::Dictionary(catalog));
    out.trailer.set("Root", Object::Reference(catalog_id));

    let mut bytes = Vec::new();
    out.save_to(&mut bytes)
        .map_err(|e| PassageError::Pdf(e.to_string()))?;
    Ok(bytes)
}

/// A page attribute, taken from the page or inherited from its
/// ancestors in the original tree.
fn inherited(doc: &Document, page: &Dictionary, key: &[u8]) -> Option<Object> {
    let mut node = Some(page.clone());
    while let Some(dict) = node {
        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }
        node = dict
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok()
            .and_then(|id| doc.get_dictionary(id).ok().cloned());
    }
    None
}

/// Deep-copy an object into `out`, remapping every reference it reaches
/// and reusing an already-copied object rather than duplicating it.
fn copy_object(
    doc: &Document,
    out: &mut Document,
    obj: &Object,
    mapping: &mut BTreeMap<ObjectId, ObjectId>,
) -> Object {
    match obj {
        Object::Reference(id) => {
            if let Some(existing) = mapping.get(id) {
                return Object::Reference(*existing);
            }
            let new_id = out.new_object_id();
            mapping.insert(*id, new_id);
            let copied = doc
                .get_object(*id)
                .map_or(Object::Null, |o| copy_object(doc, out, o, mapping));
            out.objects.insert(new_id, copied);
            Object::Reference(new_id)
        }
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|o| copy_object(doc, out, o, mapping))
                .collect(),
        ),
        Object::Dictionary(dict) => {
            let mut copy = Dictionary::new();
            for (key, value) in dict {
                copy.set(key.clone(), copy_object(doc, out, value, mapping));
            }
            Object::Dictionary(copy)
        }
        Object::Stream(stream) => {
            let mut copy = stream.clone();
            let mut dict = Dictionary::new();
            for (key, value) in &stream.dict {
                dict.set(key.clone(), copy_object(doc, out, value, mapping));
            }
            copy.dict = dict;
            Object::Stream(copy)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{hex_text, normalise, parse_to_unicode, Matrix};

    #[test]
    fn normalise_collapses_whitespace_but_nothing_else() {
        assert_eq!(
            normalise("  the\nplaintiff   alleges\t"),
            "the plaintiff alleges"
        );
        assert_eq!(normalise("Plaintiff's"), "Plaintiff's");
        assert_eq!(normalise("   "), "");
    }

    #[test]
    fn matrix_then_composes_in_pdf_order() {
        let scale = Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let shift = Matrix::translate(10.0, 5.0);
        // Scale first, then translate: (1,1) -> (2,2) -> (12,7).
        let (x, y) = scale.then(shift).apply(1.0, 1.0);
        assert!((x - 12.0).abs() < 1e-9, "x = {x}");
        assert!((y - 7.0).abs() < 1e-9, "y = {y}");
    }

    #[test]
    fn hex_text_decodes_utf16be_including_ligatures() {
        assert_eq!(hex_text("0041"), "A");
        assert_eq!(hex_text("00660066"), "ff");
    }

    #[test]
    fn to_unicode_reads_bfchar_and_bfrange_sections() {
        let cmap = b"begincmap
2 beginbfchar
<0001> <0054>
<0015> <00660066>
endbfchar
1 beginbfrange
<0020> <0022> <0041>
<0030> <0031> [<0061> <0062>]
endbfrange
endcmap";
        let map = parse_to_unicode(cmap);
        assert_eq!(map.get(&0x0001).map(String::as_str), Some("T"));
        assert_eq!(map.get(&0x0015).map(String::as_str), Some("ff"));
        assert_eq!(map.get(&0x0020).map(String::as_str), Some("A"));
        assert_eq!(map.get(&0x0021).map(String::as_str), Some("B"));
        assert_eq!(map.get(&0x0022).map(String::as_str), Some("C"));
        assert_eq!(map.get(&0x0030).map(String::as_str), Some("a"));
        assert_eq!(map.get(&0x0031).map(String::as_str), Some("b"));
    }
}
