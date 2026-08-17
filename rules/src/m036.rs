//! `M036` — bold text must not stand in for a heading. A narrower take
//! on markdownlint MD036 (no-emphasis-as-heading).
//!
//! A whole-line paragraph that is nothing but **bold** text
//! (`**Overview**`) is usually a heading typed as emphasis; it should be
//! a real `##` so it lands in the table of contents and gets a heading
//! anchor. Three guards keep the false-positive rate low:
//!
//! - **Only strong (bold) markers count**, not single-marker italic:
//!   `*` / `_` standalone lines are far more often legitimate — a
//!   caption, a note, an `_Last updated: …_` metadata line — than a
//!   heading.
//! - **Trailing punctuation is exempt** (markdownlint's default): a
//!   bold sentence ending in `.`/`:`/`?`… reads as lead-in prose, not a
//!   heading.
//! - **Only a standalone paragraph counts:** the bold line must be
//!   surrounded by blank lines, so a bold lead-in that continues onto the
//!   next line is left alone.
//!
//! This rule is scoped to prose Markdown, not notation templates:
//! legal template bodies legitimately set standalone bold labels in
//! signature blocks (`**Employee**`, `**Contractor**`), which are not
//! headings.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M036NoEmphasisAsHeading;

impl M036NoEmphasisAsHeading {
    pub const CODE: &'static str = "M036";
    /// Trailing characters that mark the line as a sentence, not a
    /// heading (markdownlint's default `punctuation` set).
    const PUNCTUATION: &'static [char] = &['.', ',', ';', ':', '!', '?'];
}

impl Rule for M036NoEmphasisAsHeading {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn description(&self) -> &'static str {
        crate::description_for_code(Self::CODE)
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let body = frontmatter::body_lines(&file.contents);
        let mut violations = Vec::new();
        for (idx, &(line_no, line)) in body.iter().enumerate() {
            if !emphasis_only(line) {
                continue;
            }
            // Standalone paragraph: the neighbours in the body must be
            // blank (or the document boundary).
            let prev_blank = idx
                .checked_sub(1)
                .is_none_or(|i| body[i].1.trim().is_empty());
            let next_blank = body.get(idx + 1).is_none_or(|n| n.1.trim().is_empty());
            if prev_blank && next_blank {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: line_byte_range(&file.contents, line_no),
                    message: "Bold text used as a heading; use a `#` heading instead so it \
                              gets an anchor and lands in the outline"
                        .to_string(),
                });
            }
        }
        violations
    }
}

/// True when the whole trimmed line is a single **strong** (bold) span
/// (`**…**` or `__…__`) whose inner text does not end in sentence
/// punctuation. Single-marker italic (`*…*`, `_…_`), `**a** b **c**`
/// (two spans), and `**Done.**` (trailing period) all return false.
fn emphasis_only(line: &str) -> bool {
    let t = line.trim();
    let bytes = t.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    if first != b'*' && first != b'_' {
        return false;
    }
    let run = bytes.iter().take_while(|&&b| b == first).count();
    // Strong markers only (`**`/`__`); italic single markers are left
    // to prose. There must be inner text between the two runs.
    if run != 2 || t.len() <= run * 2 {
        return false;
    }
    // Must close with the same marker run of the same length.
    if bytes[bytes.len() - run..] != [first; 2] {
        return false;
    }
    let inner = &t[run..t.len() - run];
    // A single span: the marker character appears nowhere inside.
    if inner.is_empty()
        || inner.starts_with(char::is_whitespace)
        || inner.ends_with(char::is_whitespace)
        || inner.bytes().any(|b| b == first)
    {
        return false;
    }
    !inner
        .trim_end()
        .ends_with(M036NoEmphasisAsHeading::PUNCTUATION)
}

#[cfg(test)]
mod tests {
    use super::M036NoEmphasisAsHeading;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("guide.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn flags_a_standalone_bold_line() {
        let body = "# Title\n\n**Overview**\n\nSome prose.\n";
        let v = M036NoEmphasisAsHeading.lint(&file(body));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].code, "M036");
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn allows_a_standalone_italic_line() {
        // Single-marker italic is left to prose — captions, notes, and
        // `_Last updated: …_` metadata lines are legitimate.
        let body = "intro\n\n_Details_\n\nmore\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
        let meta = "_Last updated: May 29, 2026_\n\nbody\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(meta)).is_empty());
    }

    #[test]
    fn allows_a_bold_line_ending_in_punctuation() {
        // Reads as lead-in prose, not a heading.
        let body = "a\n\n**Note that this matters.**\n\nb\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
    }

    #[test]
    fn allows_a_bold_lead_in_that_continues_on_the_next_line() {
        let body = "a\n\n**Lead in**\ncontinues here without a blank.\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
    }

    #[test]
    fn allows_a_line_with_bold_plus_trailing_text() {
        let body = "a\n\n**How we work** — the firm does X.\n\nb\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
    }

    #[test]
    fn allows_two_emphasis_spans_on_one_line() {
        let body = "a\n\n**one** and **two**\n\nb\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
    }

    #[test]
    fn ignores_real_headings() {
        let body = "# Title\n\n## Section\n\nbody\n";
        assert!(M036NoEmphasisAsHeading.lint(&file(body)).is_empty());
    }
}
