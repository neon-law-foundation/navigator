//! `M009` — lines must not contain trailing whitespace, except
//! for the markdownlint-default exception of exactly two trailing
//! spaces (the markdown hard-break marker). Mirrors MD009.

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M009NoTrailingSpaces;

impl M009NoTrailingSpaces {
    pub const CODE: &'static str = "M009";
}

impl Rule for M009NoTrailingSpaces {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let trimmed = line.trim_end_matches(' ');
                let trailing = line.len() - trimmed.len();
                // Allow exactly 2 trailing spaces on a non-blank line
                // as a markdown hard-break.
                if trailing == 0 {
                    return None;
                }
                if trailing == 2 && !trimmed.is_empty() {
                    return None;
                }
                // Narrow the range to just the trailing whitespace so
                // `fix()` deletes only the offending span.
                let line_range = line_byte_range(&file.contents, idx + 1);
                let trim_start = line_range.end - trailing;
                Some(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: idx + 1,
                    range: trim_start..line_range.end,
                    message: format!("Line has {trailing} trailing space(s)"),
                })
            })
            .collect()
    }

    fn fix(&self, _file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        Some(TextEdit {
            range: violation.range.clone(),
            new_text: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::M009NoTrailingSpaces;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_lines_without_trailing_whitespace() {
        assert!(M009NoTrailingSpaces
            .lint(&file("clean\nlines\n"))
            .is_empty());
    }

    #[test]
    fn allows_exactly_two_trailing_spaces_as_hard_break() {
        // "line1  \nline2" — two-space hard break.
        let body = "line1  \nline2\n";
        assert!(M009NoTrailingSpaces.lint(&file(body)).is_empty());
    }

    #[test]
    fn flags_one_trailing_space() {
        let v = M009NoTrailingSpaces.lint(&file("line \nok\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn flags_three_or_more_trailing_spaces() {
        let v = M009NoTrailingSpaces.lint(&file("line   \nline    \n"));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn flags_blank_line_with_spaces() {
        // Two spaces on an otherwise-blank line is NOT a hard break.
        let v = M009NoTrailingSpaces.lint(&file("a\n  \nb\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
    }

    #[test]
    fn fix_strips_trailing_whitespace_and_repeated_run_is_clean() {
        let src = file("line   \nok\n");
        let violations = M009NoTrailingSpaces.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M009NoTrailingSpaces
            .fix(&src, &violations[0])
            .expect("M009 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "line\nok\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M009NoTrailingSpaces.lint(&after).is_empty());
    }
}
