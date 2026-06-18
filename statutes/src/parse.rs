//! Parse a Nevada Legislature NRS chapter page into sections.
//!
//! The source is MS-Word-filtered HTML (confirmed 2026-06-06). A chapter
//! page carries, in order: chrome (`p.Chapter`, `p.COHead2`), a
//! table-of-contents (`p.COLeadline`, skipped), then the section bodies.
//! Each section body opens with a `<p class="SectBody">` whose header
//! spans are `span.Section` (the number, e.g. `649.005`) and
//! `span.Leadline` (the title, e.g. `Definitions.`), followed by the
//! body text as direct nodes. Multi-subsection sections continue in
//! further `p.SectBody` paragraphs (markers `1.`, `(a)` are plain text).
//! A `p.SourceNote` carries the amendment tail and ends the section.
//!
//! This module is pure: it takes a UTF-8 `&str` (the live path decodes
//! windows-1252 to UTF-8 upstream in [`crate::fetch`]) and is tested
//! against a saved fixture — never the live site.

use scraper::{ElementRef, Html, Node, Selector};
use sha2::{Digest, Sha256};

/// One parsed section, ready to hand to `store::statutes::upsert_section`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSection {
    /// Section number as the source prints it (`649.005`).
    pub section: String,
    /// Section heading (`Definitions.`).
    pub section_title: String,
    /// Normalized display body — whitespace collapsed within a
    /// paragraph, paragraphs joined by `\n`.
    pub body: String,
    /// Lowercase hex SHA-256 of `body` (which is already normalized), the
    /// change-detection key.
    pub body_sha256: String,
    /// The legislature's amendment tail, verbatim, or `None`.
    pub history_note: Option<String>,
}

/// A whole chapter as parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedChapter {
    /// Chapter number as printed (`649`, `118A`).
    pub chapter: String,
    /// Chapter title (`COLLECTION AGENCIES`).
    pub chapter_title: String,
    pub sections: Vec<ParsedSection>,
}

/// Errors parsing a chapter page.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The `p.Chapter` heading was missing — not an NRS chapter page.
    #[error("no chapter heading found (not an NRS chapter page?)")]
    NoChapterHeading,
}

/// Header spans whose text is metadata, not body. Excluded when
/// collecting a paragraph's body text.
const HEADER_CLASSES: [&str; 3] = ["Empty", "Section", "Leadline"];

/// Collapse every run of whitespace (ASCII plus the en-space `U+2002`
/// and nbsp `U+00A0` the source litters everywhere) to a single space
/// and trim. Stable across cosmetic re-formatting on the source.
#[must_use]
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lowercase hex SHA-256 of a string.
#[must_use]
pub fn sha256_hex(text: &str) -> String {
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.finalize().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Parse a chapter page. The chapter number and title are read from the
/// `p.Chapter` heading, so nothing about the chapter is assumed.
///
/// # Errors
///
/// Returns [`ParseError::NoChapterHeading`] when the page has no
/// `p.Chapter` element (e.g. a 404 body slipped through).
pub fn parse_chapter(html: &str) -> Result<ParsedChapter, ParseError> {
    let doc = Html::parse_document(html);

    // Selectors are static, valid literals — `unwrap` is unreachable.
    let chapter_sel = Selector::parse("p.Chapter").unwrap();
    let body_or_note = Selector::parse("p.SectBody, p.SourceNote").unwrap();
    let section_sel = Selector::parse("span.Section").unwrap();
    let leadline_sel = Selector::parse("span.Leadline").unwrap();

    let heading = doc
        .select(&chapter_sel)
        .next()
        .ok_or(ParseError::NoChapterHeading)?;
    let (chapter, chapter_title) = split_chapter_heading(&collect_all_text(heading));

    let mut sections: Vec<ParsedSection> = Vec::new();
    let mut current: Option<Builder> = None;

    for el in doc.select(&body_or_note) {
        let is_source_note = el.value().classes().any(|c| c == "SourceNote");
        if is_source_note {
            if let Some(b) = current.as_mut() {
                let note = normalize(&collect_all_text(el));
                if !note.is_empty() {
                    b.history_note = Some(note);
                }
            }
            continue;
        }

        // A SectBody. A `span.Section` child marks a new section start.
        if let Some(num_span) = el.select(&section_sel).next() {
            if let Some(done) = current.take() {
                sections.push(done.finish());
            }
            let section = normalize(&collect_all_text(num_span));
            let title = el
                .select(&leadline_sel)
                .next()
                .map(|s| normalize(&collect_all_text(s)))
                .unwrap_or_default();
            let mut b = Builder::new(section, title);
            b.push_body(&collect_body_text(el));
            current = Some(b);
        } else if let Some(b) = current.as_mut() {
            // Continuation paragraph (subsection) of the open section.
            b.push_body(&collect_body_text(el));
        }
    }
    if let Some(done) = current.take() {
        sections.push(done.finish());
    }

    Ok(ParsedChapter {
        chapter,
        chapter_title,
        sections,
    })
}

/// Accumulates one section's body chunks before normalization-join.
struct Builder {
    section: String,
    section_title: String,
    chunks: Vec<String>,
    history_note: Option<String>,
}

impl Builder {
    fn new(section: String, section_title: String) -> Self {
        Self {
            section,
            section_title,
            chunks: Vec::new(),
            history_note: None,
        }
    }

    fn push_body(&mut self, raw: &str) {
        let chunk = normalize(raw);
        if !chunk.is_empty() {
            self.chunks.push(chunk);
        }
    }

    fn finish(self) -> ParsedSection {
        let body = self.chunks.join("\n");
        let body_sha256 = sha256_hex(&body);
        ParsedSection {
            section: self.section,
            section_title: self.section_title,
            body,
            body_sha256,
            history_note: self.history_note,
        }
    }
}

/// Split `CHAPTER 649 - COLLECTION AGENCIES` into (`649`,
/// `COLLECTION AGENCIES`). Tolerant: if the dash form is absent, the
/// whole remainder is the title and the number is best-effort.
fn split_chapter_heading(heading: &str) -> (String, String) {
    let norm = normalize(heading);
    let rest = norm.strip_prefix("CHAPTER ").unwrap_or(&norm);
    if let Some((num, title)) = rest.split_once(" - ") {
        (num.trim().to_string(), title.trim().to_string())
    } else {
        // No dash: take the first token as the number if it looks like one.
        let num = rest.split_whitespace().next().unwrap_or("").to_string();
        (num, rest.to_string())
    }
}

/// All descendant text of an element, in document order.
fn collect_all_text(el: ElementRef) -> String {
    el.text().collect::<String>()
}

/// Body text of a `<p>`: the text of every child node that is **not** a
/// header span (`Empty`/`Section`/`Leadline`). Direct text nodes and
/// inline `<a>` cross-references are kept; the number/title/padding spans
/// are dropped.
fn collect_body_text(el: ElementRef) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => {
                let is_header = e.classes().any(|c| HEADER_CLASSES.contains(&c));
                if !is_header {
                    if let Some(child_el) = ElementRef::wrap(child) {
                        out.push_str(&collect_all_text(child_el));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize, parse_chapter, sha256_hex};

    const FIXTURE: &str = include_str!("../tests/fixtures/nrs-649-excerpt.html");

    #[test]
    fn normalize_collapses_word_whitespace_and_special_spaces() {
        // en-space U+2002 (the source's &#8194;) and nbsp U+00A0 collapse.
        assert_eq!(normalize("a\u{2002}\u{2002}b\n  c\u{00A0}d"), "a b c d");
        assert_eq!(normalize("   trim   me   "), "trim me");
    }

    #[test]
    fn parses_chapter_number_and_title_from_heading() {
        let ch = parse_chapter(FIXTURE).unwrap();
        assert_eq!(ch.chapter, "649");
        assert_eq!(ch.chapter_title, "COLLECTION AGENCIES");
    }

    #[test]
    fn skips_toc_and_chrome_and_finds_only_real_sections() {
        let ch = parse_chapter(FIXTURE).unwrap();
        // Two real section bodies; the COLeadline TOC entries are skipped.
        let nums: Vec<_> = ch.sections.iter().map(|s| s.section.as_str()).collect();
        assert_eq!(nums, vec!["649.005", "649.020"]);
    }

    #[test]
    fn single_paragraph_section_body_and_title_and_history() {
        let ch = parse_chapter(FIXTURE).unwrap();
        let s = &ch.sections[0];
        assert_eq!(s.section_title, "Definitions.");
        assert_eq!(
            s.body,
            "As used in this chapter, unless the context otherwise requires, the words \
             and terms defined in NRS 649.010 to 649.044, inclusive, have the meanings \
             ascribed to them in those sections."
        );
        // The "NRS 649.005 Definitions." header is NOT in the body.
        assert!(!s.body.contains("Definitions."));
        assert!(!s.body.starts_with("NRS"));
        assert_eq!(
            s.history_note.as_deref(),
            Some("(Added to NRS by 1969, 829; A 1983, 1710)")
        );
        // hash is over the (already normalized) body
        assert_eq!(s.body_sha256, sha256_hex(&s.body));
    }

    #[test]
    fn multi_subsection_section_joins_paragraphs_with_newlines() {
        let ch = parse_chapter(FIXTURE).unwrap();
        let s = &ch.sections[1];
        assert_eq!(s.section_title, "“Collection agency” defined.");
        let lines: Vec<&str> = s.body.split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("1. “Collection agency” means all persons"));
        assert!(lines[1].starts_with("2. “Collection agency” does not include"));
        assert_eq!(
            lines[2],
            "(a) Natural persons regularly employed by an exempt entity."
        );
        assert!(lines[3].starts_with("(b) Banks, savings banks"));
        assert_eq!(
            s.history_note.as_deref(),
            Some("[Part 4:237:1931]—(NRS A 1995, 999)")
        );
    }

    #[test]
    fn a_404_style_body_without_a_chapter_heading_is_an_error() {
        let err = parse_chapter("<html><body><p>Not found</p></body></html>");
        assert!(err.is_err());
    }
}
