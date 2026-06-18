//! YAML frontmatter helpers.
//!
//! Markdown files in this project carry a small set of top-level
//! `key: value` pairs between two `---` markers at the very start of
//! the file. Field lookup goes through `serde_yaml` so that folded
//! (`>`) and literal (`|`) block scalars parse correctly — needed so
//! authors can wrap long values across multiple lines and still pass
//! the 120-character line-length rule (S101).

use std::ops::RangeInclusive;

/// Extract the raw text of the leading YAML frontmatter block.
///
/// Returns `Some(body)` when `contents` starts with `---` on its own
/// line and a matching `---` closer follows (with or without a trailing
/// newline). Returns `None` if either marker is absent.
#[must_use]
pub fn extract(contents: &str) -> Option<&str> {
    let after_open = contents.strip_prefix("---\n")?;
    // Empty frontmatter: closer immediately follows the opener.
    if after_open == "---" || after_open.starts_with("---\n") {
        return Some("");
    }
    if let Some(end) = after_open.find("\n---\n") {
        return Some(&after_open[..end]);
    }
    // Closer at EOF without a trailing newline.
    after_open.strip_suffix("\n---")
}

/// Yields `(line_number, line)` for every line of `contents` that
/// is *outside* the YAML frontmatter and *outside* fenced code
/// blocks (triple-backtick or triple-tilde). Lines that are
/// themselves the fence open/close are also skipped.
///
/// Rules that only make sense in prose context (heading-shape
/// checks, list-marker checks, emphasis style) should iterate via
/// this helper so that `# ...` and `**...**` runs appearing inside
/// a shell example or YAML body don't fire as Markdown violations.
#[must_use]
pub fn body_lines(contents: &str) -> Vec<(usize, &str)> {
    let fm = line_range(contents);
    let mut out = Vec::new();
    let mut in_fence = false;
    for (idx, line) in contents.lines().enumerate() {
        let line_no = idx + 1;
        if fm.as_ref().is_some_and(|r| r.contains(&line_no)) {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        out.push((line_no, line));
    }
    out
}

/// Replace every `[text](url)` link's URL portion with spaces. Lets
/// emphasis/strong rules scan inline content without misreading
/// `_` or `*` inside link targets (e.g.
/// `[foo](../path/to/snake_case_name.rs)` looking like `_case_`
/// emphasis).
#[must_use]
pub fn mask_link_urls(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `](`
        if i + 1 < bytes.len() && bytes[i] == b']' && bytes[i + 1] == b'(' {
            // Find the matching `)` allowing nested parens.
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            // Emit `](` then spaces for the URL body, then `)`.
            out.push(b']');
            out.push(b'(');
            out.extend(std::iter::repeat_n(b' ', j.saturating_sub(i + 3)));
            if j > 0 {
                out.push(b')');
            }
            i = j;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

/// Replace every backtick-delimited code span in `line` with spaces
/// of the same byte length, leaving the rest of the line untouched.
/// Lets emphasis/strong rules scan inline content without misreading
/// characters that live inside `code`.
#[must_use]
pub fn mask_code_spans(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            // Find a closing run of the same length.
            let close_marker = "`".repeat(run);
            if let Some(end_offset) = line[i + run..].find(&close_marker) {
                let span_end = i + run + end_offset + run;
                out.extend(std::iter::repeat_n(b' ', span_end - i));
                i = span_end;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

/// Inclusive 1-based line range covered by the leading frontmatter
/// block, including the opening and closing `---` delimiters. Returns
/// `None` when `contents` has no recognized frontmatter. Rules that
/// scan body content line-by-line use this to skip the frontmatter so
/// markdown-shaped tokens inside YAML aren't misread (e.g. `---` as a
/// setext underline or `respondent_type: …` as a list marker).
#[must_use]
pub fn line_range(contents: &str) -> Option<RangeInclusive<usize>> {
    if !contents.starts_with("---\n") && contents != "---" {
        return None;
    }
    // Walk lines looking for the second `---`.
    let mut last = None;
    for (idx, line) in contents.lines().enumerate() {
        if idx == 0 {
            if line.trim_end() != "---" {
                return None;
            }
            continue;
        }
        if line.trim_end() == "---" {
            last = Some(idx + 1);
            break;
        }
    }
    last.map(|end| 1..=end)
}

/// Look up a top-level `key:` field in extracted frontmatter and
/// return its parsed value as a trimmed `String`. Uses `serde_yaml`
/// under the hood so folded (`>`) and literal (`|`) block scalars
/// produce the same value the rest of the pipeline reads — wrapping
/// `description: >\n  foo\n  bar` is semantically equivalent to
/// `description: foo bar`.
///
/// Bare keys (`title:`) parse to YAML `null` and surface as
/// `Some(String::new())` for backwards compatibility with the prior
/// hand-rolled parser. Non-scalar values (mappings, sequences) return
/// `None` — callers that care about structured content should call
/// `serde_yaml::from_str` directly with a typed shape (see
/// [`crate::f104`]).
#[must_use]
pub fn field(frontmatter: &str, key: &str) -> Option<String> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(frontmatter).ok()?;
    let mapping = parsed.as_mapping()?;
    let needle = serde_yaml::Value::String(key.to_string());
    let value = mapping.get(&needle)?;
    match value {
        serde_yaml::Value::String(s) => Some(s.trim().to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Null => Some(String::new()),
        _ => None,
    }
}

/// Inclusive 1-based line ranges (within `contents`) whose lines are
/// the *content* of a folded (`>`) block scalar inside the leading
/// frontmatter. The `key: >` opener line is **not** included, only
/// the indented lines that get folded into the scalar's value.
///
/// Folded scalars are the one place inside YAML frontmatter where
/// S102 line-packing is safe: every newline inside the block folds
/// to a single space, so pulling a word up from line `n+1` to line
/// `n` produces an identical parsed value. Literal (`|`) blocks are
/// deliberately excluded — their newlines are preserved as `\n` in
/// the parsed string, so packing them would change the value.
#[must_use]
pub fn folded_scalar_lines(contents: &str) -> Vec<RangeInclusive<usize>> {
    let Some(fm_range) = line_range(contents) else {
        return Vec::new();
    };
    let mut regions = Vec::new();
    let lines: Vec<&str> = contents.lines().collect();
    let mut idx = *fm_range.start(); // 1-based, opening `---`
    let end = *fm_range.end(); // 1-based, closing `---`
    while idx < end {
        let line = lines[idx - 1];
        if is_folded_opener(line) {
            let key_indent = leading_ws_len(line);
            let block_start = idx + 1;
            let mut block_end = block_start;
            while block_end < end {
                let candidate = lines[block_end - 1];
                if candidate.trim().is_empty() {
                    // Blank lines belong to the block in YAML — keep going.
                    block_end += 1;
                    continue;
                }
                if leading_ws_len(candidate) <= key_indent {
                    break;
                }
                block_end += 1;
            }
            if block_end > block_start {
                regions.push(block_start..=block_end - 1);
            }
            idx = block_end;
            continue;
        }
        idx += 1;
    }
    regions
}

fn is_folded_opener(line: &str) -> bool {
    // Matches `key: >`, `key: >-`, `key: >+`, with or without trailing
    // whitespace / comment. Does NOT match literal `|` openers.
    let trimmed = line.trim_end();
    let Some(colon) = trimmed.find(':') else {
        return false;
    };
    let after = trimmed[colon + 1..].trim_start();
    if !after.starts_with('>') {
        return false;
    }
    // After the `>` may come `-`, `+`, or whitespace/EOL only.
    let rest = &after[1..];
    rest.chars()
        .all(|c| c == '-' || c == '+' || c.is_whitespace())
}

fn leading_ws_len(line: &str) -> usize {
    line.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

#[cfg(test)]
mod tests {
    use super::{extract, field, folded_scalar_lines};

    #[test]
    fn extract_returns_body_between_markers() {
        assert_eq!(extract("---\nfoo: bar\n---\nrest"), Some("foo: bar"));
    }

    #[test]
    fn extract_handles_closer_at_eof_without_newline() {
        assert_eq!(extract("---\nfoo: bar\n---"), Some("foo: bar"));
    }

    #[test]
    fn extract_returns_none_without_opening_marker() {
        assert_eq!(extract("foo: bar\n---\n"), None);
    }

    #[test]
    fn extract_returns_none_without_closing_marker() {
        assert_eq!(extract("---\nfoo: bar\n"), None);
    }

    #[test]
    fn extract_handles_empty_frontmatter() {
        assert_eq!(extract("---\n---\n"), Some(""));
    }

    #[test]
    fn field_finds_top_level_key() {
        assert_eq!(
            field("title: Hello\nauthor: Jane", "title"),
            Some("Hello".to_string())
        );
        assert_eq!(
            field("title: Hello\nauthor: Jane", "author"),
            Some("Jane".to_string())
        );
    }

    #[test]
    fn field_returns_none_for_missing_key() {
        assert_eq!(field("title: Hello", "author"), None);
    }

    #[test]
    fn field_returns_empty_for_bare_key() {
        assert_eq!(field("title:", "title"), Some(String::new()));
        assert_eq!(field("title: ", "title"), Some(String::new()));
    }

    #[test]
    fn field_ignores_indented_nested_keys() {
        let fm = "owner:\n  title: Nested\ntitle: Top";
        assert_eq!(field(fm, "title"), Some("Top".to_string()));
    }

    #[test]
    fn field_folds_block_scalars_to_single_string() {
        // `>` folds newlines to spaces — the parsed value is identical
        // to the equivalent one-line form. This is the property that
        // makes S101 over frontmatter safe for skill descriptions.
        let fm = "description: >\n  Line one of the value.\n  Line two of the value.";
        assert_eq!(
            field(fm, "description"),
            Some("Line one of the value. Line two of the value.".to_string())
        );
    }

    #[test]
    fn field_returns_string_for_quoted_value() {
        let fm = "title: \"Quoted Hello\"";
        assert_eq!(field(fm, "title"), Some("Quoted Hello".to_string()));
    }

    #[test]
    fn field_returns_none_for_mapping_value() {
        // Structured values aren't representable as a single string —
        // callers wanting them should use `serde_yaml::from_str` with
        // a typed shape (see `f104`/`f106`).
        let fm = "workflow:\n  BEGIN:\n    a: END";
        assert_eq!(field(fm, "workflow"), None);
    }

    #[test]
    fn folded_scalar_lines_finds_simple_block() {
        let body = "---\ndescription: >\n  one\n  two\n---\n";
        // Frontmatter occupies lines 1..=5. `description: >` is line 2.
        // The folded content is lines 3 and 4.
        assert_eq!(folded_scalar_lines(body), vec![3..=4]);
    }

    #[test]
    fn folded_scalar_lines_skips_literal_block() {
        let body = "---\ndescription: |\n  one\n  two\n---\n";
        // Literal `|` preserves newlines — never include in S102 regions.
        assert!(folded_scalar_lines(body).is_empty());
    }

    #[test]
    fn folded_scalar_lines_handles_strip_and_keep_modifiers() {
        let body = "---\ndescription: >-\n  one\n  two\n---\n";
        assert_eq!(folded_scalar_lines(body), vec![3..=4]);
        let body = "---\ndescription: >+\n  one\n  two\n---\n";
        assert_eq!(folded_scalar_lines(body), vec![3..=4]);
    }

    #[test]
    fn folded_scalar_lines_returns_empty_for_no_frontmatter() {
        assert!(folded_scalar_lines("no frontmatter here").is_empty());
    }

    #[test]
    fn folded_scalar_lines_finds_multiple_blocks() {
        let body = "---\na: >\n  first\n  block\nb: plain\nc: >\n  second\n  block\n---\n";
        // a's block is lines 3-4, c's block is lines 7-8.
        assert_eq!(folded_scalar_lines(body), vec![3..=4, 7..=8]);
    }
}
