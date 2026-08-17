//! `M021` — closed ATX heading must have exactly one space before
//! the closing `#` run. Mirrors markdownlint MD021.

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M021NoMultipleSpaceClosedATX;

impl M021NoMultipleSpaceClosedATX {
    pub const CODE: &'static str = "M021";
}

impl Rule for M021NoMultipleSpaceClosedATX {
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
                let inner = &bytes[open..bytes.len() - close];
                let trailing_spaces = inner.iter().rev().take_while(|&&b| b == b' ').count();
                if trailing_spaces > 1 {
                    let line_range = line_byte_range(&file.contents, idx + 1);
                    let run_end = line_range.start + trimmed.len() - close;
                    let run_start = run_end - trailing_spaces;
                    return Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: run_start..run_end,
                        message:
                            "Closed ATX heading must have exactly one space before closing `#`"
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
    use super::M021NoMultipleSpaceClosedATX;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_one_space_before_closing() {
        assert!(M021NoMultipleSpaceClosedATX
            .lint(&f("## Title ##\n"))
            .is_empty());
    }
    #[test]
    fn flags_multiple_spaces_before_closing() {
        let v = M021NoMultipleSpaceClosedATX.lint(&f("## Title   ##\nok\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn fix_collapses_to_one_space_and_relints_clean() {
        let src = f("## Title   ##\n");
        let violations = M021NoMultipleSpaceClosedATX.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M021NoMultipleSpaceClosedATX
            .fix(&src, &violations[0])
            .expect("M021 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "## Title ##\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M021NoMultipleSpaceClosedATX.lint(&after).is_empty());
    }
}
