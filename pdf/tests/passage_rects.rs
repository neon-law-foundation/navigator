//! Passage location proven through real PDFs, not through the string
//! the extractor happens to build (#893).
//!
//! # Why a hand-built fixture
//!
//! One fixture here is assembled operator by operator with `lopdf`: a
//! simple `WinAnsiEncoding` font whose `/Widths` are all exactly 500,
//! set at 12pt, shown at a known `Td`. That makes every coordinate the
//! pipeline emits arithmetic rather than approximate — a 6pt advance per
//! character on a 612 x 792 page — so the rect can be asserted to six
//! decimal places. A rect that drifts fails here, which is the whole
//! point: a highlight at the wrong coordinates asserts that someone
//! checked something they did not.
//!
//! The other fixtures are rendered through `pdf::render`, so the
//! Typst-produced court paper from #889 is exercised as an input on its
//! own terms — Type0/`Identity-H` with subset fonts, where
//! `pdf::page_text` sees only glyph ids.
//!
//! Every fixture is synthetic. No vendored document is read here.

use lopdf::{dictionary, Document, Object, Stream};
use pdf::{NormalisedRect, PassageError};

/// US Letter in PDF points, the paper every fixture is cut to. Every
/// fixture constant is an integer because it is written into the PDF as
/// one; [`pt`] lifts it for the arithmetic the assertions do.
const PAGE_W: i64 = 612;
const PAGE_H: i64 = 792;
/// The fixture font's uniform advance, in 1/1000 em.
const FIXTURE_WIDTH: i64 = 500;
const FIXTURE_SIZE: i64 = 12;
/// `/Ascent` and `/Descent` the fixture's descriptor declares.
const FIXTURE_ASCENT: i64 = 750;
const FIXTURE_DESCENT: i64 = -250;

/// A fixture constant as points.
fn pt(v: i64) -> f64 {
    f64::from(i32::try_from(v).expect("a fixture constant fits an i32"))
}

/// A one-page PDF showing `text` at `(x, baseline)` in a simple
/// `WinAnsiEncoding` font. `widths` present declares a `/Widths` array
/// of uniform [`FIXTURE_WIDTH`]; absent leaves the font unmeasurable.
fn simple_font_pdf(text: &str, x: f64, baseline: f64, widths: bool) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "NavigatorFixture",
        "Ascent" => FIXTURE_ASCENT,
        "Descent" => FIXTURE_DESCENT,
        "Flags" => 32,
        "ItalicAngle" => 0,
        "StemV" => 80,
        "FontBBox" => vec![0.into(), (-250).into(), 1000.into(), 750.into()],
    });
    let mut font = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "NavigatorFixture",
        "Encoding" => "WinAnsiEncoding",
        "FontDescriptor" => descriptor,
    };
    if widths {
        // Every printable ASCII code advances the same 500/1000 em, so a
        // rect's width is exactly 6pt per character at 12pt type.
        font.set("FirstChar", 32);
        font.set("LastChar", 126);
        font.set(
            "Widths",
            (32..=126)
                .map(|_| Object::Integer(FIXTURE_WIDTH))
                .collect::<Vec<_>>(),
        );
    }
    let font_id = doc.add_object(font);
    let escaped = text
        .replace('\\', r"\\")
        .replace('(', r"\(")
        .replace(')', r"\)");
    let content = format!("BT /F1 {FIXTURE_SIZE} Tf {x} {baseline} Td ({escaped}) Tj ET");
    let contents = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let tree_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => tree_id,
        "Contents" => contents,
        "MediaBox" => vec![0.into(), 0.into(), PAGE_W.into(), PAGE_H.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });
    doc.objects.insert(
        tree_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("write fixture pdf");
    bytes
}

/// The `MediaBox` of a rendered PDF's first page.
fn media_box(bytes: &[u8]) -> Vec<f64> {
    let doc = Document::load_mem(bytes).expect("load pdf");
    let (_, page_id) = doc.get_pages().into_iter().next().expect("a first page");
    doc.get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"MediaBox").ok().cloned())
        .expect("a MediaBox on the page")
        .as_array()
        .expect("MediaBox array")
        .iter()
        .map(|o| {
            o.as_float()
                .map(f64::from)
                .or_else(|_| o.as_i64().map(|i| f64::from(i32::try_from(i).unwrap_or(0))))
                .expect("number")
        })
        .collect()
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{what}: expected {expected}, got {actual}",
    );
}

fn assert_inside_page(rect: NormalisedRect) {
    for (name, v) in [
        ("x", rect.x),
        ("y", rect.y),
        ("width", rect.width),
        ("height", rect.height),
    ] {
        assert!(
            (0.0..=1.0).contains(&v),
            "{name} = {v} is not a page fraction",
        );
    }
    assert!(
        rect.x + rect.width <= 1.0 + 1e-9,
        "rect runs off the right edge"
    );
    assert!(
        rect.y + rect.height <= 1.0 + 1e-9,
        "rect runs off the bottom edge"
    );
}

#[test]
fn a_rect_is_the_arithmetic_of_the_glyphs_it_covers() {
    // "The stipulation" starts 4 characters into the shown string, so at
    // 6pt per character its left edge is 72 + 24 = 96pt, and it is 15
    // characters wide = 90pt. Its top is one ascent above the 700pt
    // baseline and its bottom one descent below.
    let pdf = simple_font_pdf("And The stipulation stands.", 72.0, 700.0, true);
    let found = pdf::locate(&pdf, "The stipulation", 1).expect("locate");

    assert_eq!(found.ordinal, 1);
    assert_eq!(found.occurrences, 1);
    assert_eq!(found.rects.len(), 1, "one line, one rect");
    let hit = found.rects[0];
    assert_eq!(hit.page_index, 0);

    let advance = pt(FIXTURE_WIDTH) / 1000.0 * pt(FIXTURE_SIZE);
    let top = 700.0 + pt(FIXTURE_ASCENT) / 1000.0 * pt(FIXTURE_SIZE);
    let bottom = 700.0 + pt(FIXTURE_DESCENT) / 1000.0 * pt(FIXTURE_SIZE);
    assert_close(hit.rect.x, (72.0 + 4.0 * advance) / pt(PAGE_W), "x");
    assert_close(hit.rect.width, 15.0 * advance / pt(PAGE_W), "width");
    assert_close(hit.rect.y, (pt(PAGE_H) - top) / pt(PAGE_H), "y");
    assert_close(hit.rect.height, (top - bottom) / pt(PAGE_H), "height");
    assert_inside_page(hit.rect);
}

#[test]
fn a_rect_is_measured_from_the_page_box_not_the_origin() {
    // The same text shown further right and lower must move the rect,
    // which is what proves the normalisation reads the real geometry
    // rather than emitting a constant.
    let near = pdf::locate(
        &simple_font_pdf("Reliance.", 72.0, 700.0, true),
        "Reliance",
        1,
    )
    .expect("locate near");
    let far = pdf::locate(
        &simple_font_pdf("Reliance.", 200.0, 300.0, true),
        "Reliance",
        1,
    )
    .expect("locate far");
    assert!(far.rects[0].rect.x > near.rects[0].rect.x, "moved right");
    assert!(
        far.rects[0].rect.y > near.rects[0].rect.y,
        "moved down the page"
    );
    assert_close(
        far.rects[0].rect.width,
        near.rects[0].rect.width,
        "same text, same width",
    );
}

#[test]
fn an_unfound_quote_fails_loudly_instead_of_pointing_at_the_origin() {
    // The failure mode this module exists to prevent: an image-only or
    // drifted quote must not come back as a rect at 0,0.
    let pdf = simple_font_pdf("The stipulation stands.", 72.0, 700.0, true);
    let err = pdf::locate(&pdf, "the stipulation was withdrawn", 1).unwrap_err();
    assert!(
        matches!(err, PassageError::QuoteNotFound { .. }),
        "expected QuoteNotFound, got {err:?}",
    );
    assert!(err.to_string().contains("not found"), "{err}");
}

#[test]
fn one_drifted_word_is_a_failure_not_a_fuzzy_match() {
    let pdf = simple_font_pdf("The stipulation stands.", 72.0, 700.0, true);
    assert!(matches!(
        pdf::locate(&pdf, "The stipulations stands", 1),
        Err(PassageError::QuoteNotFound { .. })
    ));
}

#[test]
fn whitespace_drift_is_the_one_thing_normalised() {
    // A quote transcribed across a line break carries a newline the
    // source does not; that is not drift, and must still match.
    let pdf = simple_font_pdf("The stipulation stands.", 72.0, 700.0, true);
    let found = pdf::locate(&pdf, "  The\n  stipulation\tstands ", 1).expect("locate");
    assert_eq!(found.rects.len(), 1);
}

#[test]
fn an_empty_quote_is_refused() {
    let pdf = simple_font_pdf("The stipulation stands.", 72.0, 700.0, true);
    assert_eq!(
        pdf::locate(&pdf, "   \n ", 1).unwrap_err(),
        PassageError::EmptyQuote,
    );
}

#[test]
fn a_font_without_widths_is_refused_rather_than_approximated() {
    // No `/Widths` means no horizontal extent can be measured. A guessed
    // width would put the mark over the wrong words, so the pipeline
    // names the font and stops.
    let pdf = simple_font_pdf("The stipulation stands.", 72.0, 700.0, false);
    let err = pdf::locate(&pdf, "stipulation", 1).unwrap_err();
    assert_eq!(
        err,
        PassageError::UnmeasurableFont {
            font: "NavigatorFixture".into()
        },
        "expected the font to be named in the refusal",
    );
}

#[test]
fn a_typst_rendered_draft_is_an_accepted_input() {
    // #889 produces court paper; #893 consumes any stored PDF, this one
    // included. Typst subsets its fonts, so the text-showing operators
    // carry glyph ids — `pdf::page_text` cannot read them, and this
    // pipeline must, through the font's `/ToUnicode` CMap.
    let source = format!(
        "{}{}",
        pdf::pleading::preamble(pdf::pleading::Variant::NumberedRailTrial),
        "Plaintiff moved to strike the untimely declaration.",
    );
    let rendered = pdf::render(&source).expect("render court paper");

    let raw = pdf::page_text(&rendered).expect("page text");
    assert!(
        !raw.contains("untimely declaration"),
        "the naive extractor is supposed to see glyph ids here; the fixture no longer proves anything",
    );

    let found = pdf::locate(&rendered, "moved to strike the untimely declaration", 1)
        .expect("locate in a Typst-rendered draft");
    assert_eq!(found.occurrences, 1);
    assert_eq!(found.rects.len(), 1, "one line of body text");
    assert_eq!(found.rects[0].page_index, 0);
    assert_inside_page(found.rects[0].rect);
    // Court paper's body sits inside the margins, never at the corner.
    assert!(found.rects[0].rect.x > 0.05, "inside the left margin");
    assert!(found.rects[0].rect.y > 0.0, "below the top edge");
    assert!(found.rects[0].rect.width > 0.0, "a measurable extent");
}

#[test]
fn a_quote_spanning_a_page_break_yields_one_rect_per_page() {
    // Two rects, not one wrong one covering the gap between them.
    let rendered = pdf::render(
        "The stipulation was entered on the record and\n\n#pagebreak()\n\nthe court so ordered.",
    )
    .expect("render");
    let found = pdf::locate(
        &rendered,
        "entered on the record and the court so ordered",
        1,
    )
    .expect("locate across the break");

    assert!(found.spans_page_break(), "the quote does span the break");
    assert_eq!(found.pages(), vec![0, 1]);
    assert_eq!(found.rects.len(), 2, "one rect per page, never one merged");
    assert_eq!(found.rects[0].page_index, 0);
    assert_eq!(found.rects[1].page_index, 1);
    for hit in &found.rects {
        assert_inside_page(hit.rect);
        assert!(hit.rect.width > 0.0);
    }
}

#[test]
fn a_repeated_passage_is_pinned_by_its_ordinal() {
    let rendered =
        pdf::render("The parties so stipulate.\n\n#v(4em)\n\nThe parties so stipulate.\n")
            .expect("render");
    assert_eq!(
        pdf::occurrence_count(&rendered, "The parties so stipulate").expect("count"),
        2,
    );

    let first = pdf::locate(&rendered, "The parties so stipulate", 1).expect("first");
    let second = pdf::locate(&rendered, "The parties so stipulate", 2).expect("second");
    assert_eq!(first.occurrences, 2, "the count travels with the location");
    assert_eq!(second.ordinal, 2);
    assert!(
        second.rects[0].rect.y > first.rects[0].rect.y,
        "the second occurrence is further down the page",
    );

    // Past the end is a refusal, never a silent clamp back to the first.
    assert_eq!(
        pdf::locate(&rendered, "The parties so stipulate", 3).unwrap_err(),
        PassageError::OrdinalOutOfRange {
            quote: "The parties so stipulate".into(),
            ordinal: 3,
            occurrences: 2,
        },
    );
    assert!(matches!(
        pdf::locate(&rendered, "The parties so stipulate", 0),
        Err(PassageError::OrdinalOutOfRange { .. })
    ));
}

#[test]
fn page_render_lifts_one_page_and_keeps_its_text_layer() {
    let rendered =
        pdf::render("Page one holds the recital.\n\n#pagebreak()\n\nPage two holds the order.")
            .expect("render");
    assert_eq!(pdf::page_count(&rendered).expect("pages"), 2);

    let second = pdf::page_render(&rendered, 1).expect("render page two");
    assert!(second.starts_with(b"%PDF-"), "the rendering is a PDF");
    assert_eq!(
        pdf::page_count(&second).expect("pages in the rendering"),
        1,
        "a page rendering holds exactly its own page",
    );

    // The rendering carries the page's own text and none of its
    // neighbour's, so a rect located against it lands on what a reviewer
    // is looking at.
    let found = pdf::locate(&second, "Page two holds the order", 1).expect("locate in rendering");
    assert_eq!(found.rects[0].page_index, 0, "re-indexed to its own page");
    assert!(matches!(
        pdf::locate(&second, "Page one holds the recital", 1),
        Err(PassageError::QuoteNotFound { .. })
    ));
}

#[test]
fn page_render_preserves_the_page_box_so_rects_stay_comparable() {
    let rendered = pdf::render("Recital.\n\n#pagebreak()\n\nOrder.").expect("render");
    let original = media_box(&rendered);
    let lifted = media_box(&pdf::page_render(&rendered, 1).expect("render page two"));
    assert_eq!(
        original, lifted,
        "a rect normalised against the source page must overlay the rendering",
    );
}

#[test]
fn page_render_refuses_a_page_past_the_end() {
    let rendered = pdf::render("One page only.").expect("render");
    assert_eq!(
        pdf::page_render(&rendered, 7).unwrap_err(),
        PassageError::PageOutOfRange {
            page_index: 7,
            pages: 1,
        },
    );
}

#[test]
fn malformed_bytes_are_a_parse_error_not_a_panic() {
    assert!(matches!(
        pdf::locate(b"not a pdf at all", "anything", 1),
        Err(PassageError::Pdf(_))
    ));
    assert!(matches!(
        pdf::page_render(b"not a pdf at all", 0),
        Err(PassageError::Pdf(_))
    ));
}
