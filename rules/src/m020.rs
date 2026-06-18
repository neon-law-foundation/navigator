//! `M020` — closed ATX heading must have a space before the
//! closing run of `#`. Mirrors markdownlint MD020.
//!
//! A closed ATX heading is `## Title ##`. M020 flags
//! `## Title##` (missing space before the closing run).

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M020NoMissingSpaceClosedATX;

impl M020NoMissingSpaceClosedATX {
    pub const CODE: &'static str = "M020";
}

impl Rule for M020NoMissingSpaceClosedATX {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let trimmed = line.trim_end();
                let bytes = trimmed.as_bytes();
                let open = bytes.iter().take_while(|&&b| b == b'#').count();
                if open == 0 || open > 6 {
                    return None;
                }
                let close = bytes.iter().rev().take_while(|&&b| b == b'#').count();
                if close == 0 || close + open >= bytes.len() {
                    return None;
                }
                let before_close = bytes[bytes.len() - close - 1];
                if before_close != b' ' && before_close != b'#' {
                    let line_range = line_byte_range(&file.contents, idx + 1);
                    let insert_at = line_range.start + trimmed.len() - close;
                    return Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: insert_at..insert_at,
                        message: "Closed ATX heading must have a space before the closing `#` run"
                            .to_string(),
                    });
                }
                None
            })
            .collect()
    }

    fn fix(&self, _file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        Some(TextEdit {
            range: violation.range.clone(),
            new_text: " ".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::M020NoMissingSpaceClosedATX;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_proper_closed_atx() {
        assert!(M020NoMissingSpaceClosedATX
            .lint(&f("## Title ##\n# h1 #\n"))
            .is_empty());
    }
    #[test]
    fn passes_with_open_atx_no_closing_hash_run() {
        assert!(M020NoMissingSpaceClosedATX
            .lint(&f("## Title\n"))
            .is_empty());
    }
    #[test]
    fn flags_missing_space_before_closing_run() {
        let v = M020NoMissingSpaceClosedATX.lint(&f("## Title##\nok\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn fix_inserts_space_before_closing_run_and_relints_clean() {
        let src = f("## Title##\n");
        let violations = M020NoMissingSpaceClosedATX.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M020NoMissingSpaceClosedATX
            .fix(&src, &violations[0])
            .expect("M020 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "## Title ##\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M020NoMissingSpaceClosedATX.lint(&after).is_empty());
    }
}
