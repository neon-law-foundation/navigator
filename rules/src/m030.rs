//! `M030` — list markers must have exactly one space after them.
//! Mirrors MD030.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M030ListMarkerSpace;

impl M030ListMarkerSpace {
    pub const CODE: &'static str = "M030";
}

/// The byte length of the list marker at the start of `bytes` (the
/// line with leading whitespace already trimmed), or `None` when the
/// line does not open a list item. A bullet marker (`*`/`-`/`+`) must
/// be a lone character followed by a space or end-of-line — this
/// rejects `**bold**`, `--em--`, `-->`, and `*shape*`. An ordered
/// marker is a digit run closed by `.` or `)`.
fn marker_len(bytes: &[u8]) -> Option<usize> {
    let &first = bytes.first()?;
    if matches!(first, b'*' | b'-' | b'+') {
        if bytes.get(1) == Some(&first) {
            return None;
        }
        return match bytes.get(1) {
            Some(&b' ') | None => Some(1),
            _ => None,
        };
    }
    if first.is_ascii_digit() {
        let digits = bytes.iter().take_while(|&&b| b.is_ascii_digit()).count();
        if bytes.get(digits) == Some(&b'.') || bytes.get(digits) == Some(&b')') {
            return Some(digits + 1);
        }
    }
    None
}

impl Rule for M030ListMarkerSpace {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        frontmatter::body_lines(&file.contents)
            .into_iter()
            .filter_map(|(line_no, line)| {
                let trimmed = line.trim_start();
                let marker_len = marker_len(trimmed.as_bytes())?;
                let after = &trimmed.as_bytes()[marker_len..];
                let space_run = after.iter().take_while(|&&b| b == b' ').count();
                if space_run != 1 && !after.is_empty() {
                    Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: format!(
                            "List marker has {space_run} spaces after it (expected 1)"
                        ),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn fix(&self, file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        // `violation.range` is the whole flagged line. Rebuild it with
        // exactly one space between the marker and the content.
        let line = &file.contents[violation.range.clone()];
        let trimmed = line.trim_start();
        let indent_len = line.len() - trimmed.len();
        let marker_len = marker_len(trimmed.as_bytes())?;
        let rest = trimmed[marker_len..].trim_start_matches(' ');
        // Marker followed only by spaces (or nothing) is left to M009 /
        // human judgment — there is no content to re-space against.
        if rest.is_empty() {
            return None;
        }
        let fixed = format!("{}{} {rest}", &line[..indent_len], &trimmed[..marker_len]);
        (fixed != line).then_some(TextEdit {
            range: violation.range.clone(),
            new_text: fixed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::M030ListMarkerSpace;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_one_space_after_marker() {
        assert!(M030ListMarkerSpace
            .lint(&f("- one\n- two\n1. three\n"))
            .is_empty());
    }
    #[test]
    fn flags_zero_spaces_after_ordered_marker() {
        // `1.no` is recognized as a list marker (`1.`) followed by 0
        // spaces. Bullet markers (`-`/`*`/`+`) without a trailing
        // space are plain text, not lists — those are handled below.
        let v = M030ListMarkerSpace.lint(&f("1.no space\n"));
        assert_eq!(v.len(), 1);
    }
    #[test]
    fn ignores_bullet_marker_without_trailing_space() {
        // `-foo` is a hyphenated word, not a list — must not fire.
        assert!(M030ListMarkerSpace.lint(&f("-foo\n")).is_empty());
    }
    #[test]
    fn ignores_emphasis_run_at_line_start() {
        // `*shape*.` is emphasis, not a list — must not fire.
        assert!(M030ListMarkerSpace.lint(&f("*shape*.\n")).is_empty());
    }
    #[test]
    fn flags_multiple_spaces_after_marker() {
        let v = M030ListMarkerSpace.lint(&f("-  two spaces\n"));
        assert_eq!(v.len(), 1);
    }

    /// Apply the fix for the first violation in `body`.
    fn fixed(body: &str) -> String {
        let file = f(body);
        let v = M030ListMarkerSpace.lint(&file);
        let edit = M030ListMarkerSpace.fix(&file, &v[0]).expect("a fix");
        let mut out = file.contents.clone();
        out.replace_range(edit.range, &edit.new_text);
        out
    }

    #[test]
    fn fix_collapses_extra_spaces_to_one() {
        assert_eq!(fixed("-  two spaces\n"), "- two spaces\n");
        assert_eq!(fixed("1.   three\n"), "1. three\n");
    }

    #[test]
    fn fix_preserves_indentation() {
        assert_eq!(fixed("  -   nested\n"), "  - nested\n");
    }

    #[test]
    fn fix_inserts_a_missing_space_after_ordered_marker() {
        assert_eq!(fixed("1.no space\n"), "1. no space\n");
    }

    #[test]
    fn fix_is_idempotent() {
        let once = fixed("-   spaced\n");
        assert!(M030ListMarkerSpace.lint(&f(&once)).is_empty());
    }
}
