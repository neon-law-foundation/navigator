//! `S102` — body lines (and folded-scalar content inside frontmatter)
//! should be packed as close to the 120-character limit as possible.
//! Flags a line whose **next** line begins with a word that would
//! still fit if appended (with a single separating space), so the
//! prose can be reflowed tighter.
//!
//! Companion to `S101`, which polices the upper bound. `S102` polices
//! the *lower* bound by asking: "could you have pulled the next line's
//! first word up here without going over?" If yes, the wrap is too
//! eager.
//!
//! Two contexts are linted:
//! 1. **Body prose** (outside fenced code blocks).
//! 2. **Folded block scalars** (`description: >`) inside YAML
//!    frontmatter — newlines fold to spaces there, so packing is
//!    value-preserving. Literal blocks (`|`) are deliberately
//!    excluded since their newlines are part of the parsed value.
//!
//! The rule skips:
//! - fenced code blocks (triple-backtick / triple-tilde)
//! - non-folded frontmatter (plain `key: value`, flow scalars,
//!   mappings, sequences, literal `|` blocks)
//! - blank lines
//! - ATX headings (`#`, `##`, …)
//! - table rows (`|`)
//! - block-quote lines (`>`)
//! - horizontal rules (`---`, `***`, `___`)
//! - lines ending in a markdown hard break (two trailing spaces or
//!   trailing backslash) — those breaks are intentional
//! - pairs whose two lines have different leading whitespace (the
//!   next line belongs to a different block)
//! - cases where the next line begins a new list item (`-`, `*`, `+`,
//!   or an ordered marker like `1.`)
//! - pairs of lines that aren't directly adjacent in the source (a
//!   fence or frontmatter sat between them)

use crate::frontmatter;
use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct S102LinePacking {
    pub max: usize,
}

impl S102LinePacking {
    pub const CODE: &'static str = "S102";
    pub const DEFAULT_MAX: usize = 120;
}

impl Default for S102LinePacking {
    fn default() -> Self {
        Self {
            max: Self::DEFAULT_MAX,
        }
    }
}

impl Rule for S102LinePacking {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut out = Vec::new();
        // Pass 1: body prose (the original behavior).
        let body = frontmatter::body_lines(&file.contents);
        self.scan_pairs(file, &body, &mut out);
        // Pass 2: folded `>` block scalars inside frontmatter. Each
        // region's content lines fold to a single string, so packing
        // is value-preserving. We build a per-region `(line_no, line)`
        // slice and run the same pair check over it.
        let all_lines: Vec<&str> = file.contents.lines().collect();
        for region in frontmatter::folded_scalar_lines(&file.contents) {
            let mut region_lines = Vec::new();
            for line_no in region {
                region_lines.push((line_no, all_lines[line_no - 1]));
            }
            self.scan_pairs(file, &region_lines, &mut out);
        }
        out
    }
}

impl S102LinePacking {
    fn scan_pairs(&self, file: &SourceFile, lines: &[(usize, &str)], out: &mut Vec<Violation>) {
        let max = self.max;
        for pair in lines.windows(2) {
            let (a_no, a) = pair[0];
            let (b_no, b) = pair[1];
            // Only consecutive source lines — a fence or frontmatter
            // sitting between them means they're different blocks.
            if b_no != a_no + 1 {
                continue;
            }
            if a.trim().is_empty() || b.trim().is_empty() {
                continue;
            }
            if has_hard_break(a) {
                continue;
            }
            if is_non_prose(a) || is_non_prose(b) {
                continue;
            }
            let a_indent = leading_ws_len(a);
            let b_indent = leading_ws_len(b);
            if a_indent != b_indent {
                continue;
            }
            let b_body = &b[b_indent..];
            if starts_with_list_marker(b_body) {
                continue;
            }
            let Some(first_word) = b_body.split_whitespace().next() else {
                continue;
            };
            let a_chars = a.chars().count();
            let first_chars = first_word.chars().count();
            let joined = a_chars + 1 + first_chars;
            if joined <= max {
                out.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: a_no,
                    range: line_byte_range(&file.contents, a_no),
                    message: format!(
                        "Line is {a_chars} characters; could absorb \"{first_word}\" from line \
                         {b_no} to reach {joined} (max {max})",
                    ),
                });
            }
        }
    }
}

fn leading_ws_len(line: &str) -> usize {
    line.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

fn has_hard_break(line: &str) -> bool {
    // Two trailing spaces or a single trailing backslash are CommonMark
    // hard-break markers that the author put there on purpose. Don't
    // suggest unwrapping them.
    let trimmed = line.trim_end_matches('\r');
    trimmed.ends_with("  ") || trimmed.ends_with('\\')
}

fn is_non_prose(line: &str) -> bool {
    let s = line.trim_start();
    if s.is_empty() {
        return true;
    }
    if s.starts_with('#') || s.starts_with('|') || s.starts_with('>') {
        return true;
    }
    if s.starts_with("```") || s.starts_with("~~~") {
        return true;
    }
    is_horizontal_rule(s)
}

fn is_horizontal_rule(s: &str) -> bool {
    let stripped = s.trim_end();
    let Some(c0) = stripped.chars().next() else {
        return false;
    };
    if !matches!(c0, '-' | '*' | '_') {
        return false;
    }
    let count = stripped.chars().filter(|c| *c == c0).count();
    if count < 3 {
        return false;
    }
    stripped.chars().all(|c| c == c0 || c == ' ' || c == '\t')
}

fn starts_with_list_marker(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    if matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        return true;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i + 1 < bytes.len() && matches!(bytes[i], b'.' | b')') && bytes[i + 1] == b' ' {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::S102LinePacking;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(contents: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: contents.to_string(),
        }
    }

    #[test]
    fn reports_its_code() {
        assert_eq!(S102LinePacking::default().code(), "S102");
    }

    #[test]
    fn flags_short_line_that_could_absorb_next_word() {
        let body = "Short line.\nAnother short line.\n";
        let v = S102LinePacking::default().lint(&file(body));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "S102");
        assert_eq!(v[0].line, 1);
        assert!(v[0].message.contains("Another"));
    }

    #[test]
    fn passes_when_joining_would_exceed_max() {
        // 110-char first line + space + 12-char first word of next line
        // would be 123 — over the default 120-char max.
        let first = "x".repeat(110);
        let second = "yyyyyyyyyyyy and more.";
        let body = format!("{first}\n{second}\n");
        let v = S102LinePacking::default().lint(&file(&body));
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn passes_when_line_is_exactly_at_the_limit_and_next_word_fits_exactly() {
        // Edge: 113 chars + ' ' + 'word' (4) = 118 <= 120 → flag.
        let first = "x".repeat(113);
        let body = format!("{first}\nword foo bar\n");
        let v = S102LinePacking::default().lint(&file(&body));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn skips_blank_lines() {
        let body = "A line.\n\nB line.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_a_is_a_heading() {
        let body = "## A heading\nA body paragraph follows.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_b_is_a_heading() {
        let body = "Some prose.\n## Heading\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_b_is_a_list_marker() {
        let body = "Lead-in sentence.\n- next bullet\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_b_is_an_ordered_list_marker() {
        let body = "Lead-in sentence.\n1. first item\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_indent_differs() {
        // Bullet header (no indent) vs continuation (2-space indent).
        let body = "- **Item title.**\n  Continuation prose.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_pair_of_continuation_lines_with_matching_indent() {
        let body = "- **Item title.**\n  First continuation line.\n  Second continuation line.\n";
        let v = S102LinePacking::default().lint(&file(body));
        assert_eq!(v.len(), 1);
        // Only the first continuation gets flagged (it could absorb
        // "Second" from line 3); the second has no successor.
        assert_eq!(v[0].line, 2);
    }

    #[test]
    fn skips_inside_fenced_code_block() {
        let body = "Before.\n\n```\nshort\nlines inside fence\n```\n\nAfter.\n";
        // Pairs inside the fence are skipped by body_lines. The
        // "Before." / "After." pair is non-adjacent in source — they
        // belong to different blocks — and is also separated by a
        // fence, so no flag should fire.
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_when_a_ends_with_hard_break() {
        // Two trailing spaces on `a` mark a CommonMark hard break.
        let body = "Hard break here.  \nNext line of stanza.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_table_rows() {
        let body = "| col | val |\n| --- | --- |\n| a | b |\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_blockquotes() {
        let body = "> A quote line.\n> Continuation of the quote.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_horizontal_rules() {
        let body = "Before rule.\n---\nAfter rule.\n";
        // `---` is a horizontal rule (since the prior line is body,
        // not a setext title — but the rule treats it as non-prose
        // either way). No flag from pairs touching it.
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_plain_key_value_frontmatter() {
        // Plain `key: value` lines aren't a folded block — different
        // keys can't legally absorb each other's text.
        let body = "---\ntitle: Short\ndescription: Short too\n---\n\nBody.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn skips_literal_block_scalar_in_frontmatter() {
        // Literal `|` preserves newlines as part of the value; packing
        // would change what downstream code reads.
        let body = "---\ndescription: |\n  one\n  two\n---\n\nBody.\n";
        assert!(S102LinePacking::default().lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_short_lines_inside_folded_scalar_frontmatter() {
        // Folded `>` scalars collapse newlines to spaces — packing is
        // value-preserving and S102 should suggest it.
        let body = "---\ndescription: >\n  short\n  text\n---\n\nBody.\n";
        let v = S102LinePacking::default().lint(&file(body));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
        assert!(v[0].message.contains("text"));
    }

    #[test]
    fn respects_configurable_max() {
        // With max = 30, "AAAAA" (5) + ' ' + "BBBB" (4) = 10 — under
        // 30 → flag. With default 120 it'd still flag, so set max
        // high enough that the join would exceed it.
        let rule = S102LinePacking { max: 8 };
        let body = "AAAAA\nBBBB CCCC\n";
        // 5 + 1 + 4 = 10 > 8 → no flag.
        assert!(rule.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_with_helpful_message_naming_the_next_word_and_target_line() {
        let body = "Short.\nNext-word here.\n";
        let v = S102LinePacking::default().lint(&file(body));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("Next-word"));
        assert!(v[0].message.contains("line 2"));
    }

    #[test]
    fn counts_unicode_scalars_not_bytes() {
        // 118 Greek alphas + ' ' + "α" (1 char) = 120 ≤ 120 → flag.
        let first = "α".repeat(118);
        let body = format!("{first}\nα more.\n");
        let v = S102LinePacking::default().lint(&file(&body));
        assert_eq!(v.len(), 1);
    }
}
