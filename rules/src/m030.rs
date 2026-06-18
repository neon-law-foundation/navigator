//! `M030` — list markers must have exactly one space after them.
//! Mirrors MD030.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M030ListMarkerSpace;

impl M030ListMarkerSpace {
    pub const CODE: &'static str = "M030";
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
                let bytes = trimmed.as_bytes();
                if bytes.is_empty() {
                    return None;
                }
                let marker_len = if matches!(bytes[0], b'*' | b'-' | b'+') {
                    // A list marker is a single char followed by space
                    // or end-of-line. `**bold**`, `--em--`, `-->`, and
                    // bare emphasis like `*shape*` aren't lists. Reject
                    // any run of the same character, and require that
                    // the next byte (if any) be a space.
                    if bytes.get(1) == Some(&bytes[0]) {
                        return None;
                    }
                    match bytes.get(1) {
                        Some(&b' ') | None => 1,
                        _ => return None,
                    }
                } else if bytes[0].is_ascii_digit() {
                    let digits = bytes.iter().take_while(|&&b| b.is_ascii_digit()).count();
                    if bytes.get(digits) == Some(&b'.') || bytes.get(digits) == Some(&b')') {
                        digits + 1
                    } else {
                        return None;
                    }
                } else {
                    return None;
                };
                let after = &bytes[marker_len..];
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
}
