//! `M027` — no multiple spaces after blockquote `>` marker.
//! Mirrors MD027.

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M027NoMultipleSpaceBlockquote;

impl M027NoMultipleSpaceBlockquote {
    pub const CODE: &'static str = "M027";
}

impl Rule for M027NoMultipleSpaceBlockquote {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter_map(|(idx, line)| {
                let leading_ws = line.len() - line.trim_start().len();
                let trimmed = &line[leading_ws..];
                if !trimmed.starts_with('>') {
                    return None;
                }
                let after_gt = &trimmed[1..];
                let space_run = after_gt.bytes().take_while(|&b| b == b' ').count();
                if space_run > 1 {
                    let line_range = line_byte_range(&file.contents, idx + 1);
                    let run_start = line_range.start + leading_ws + 1;
                    let run_end = run_start + space_run;
                    Some(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: run_start..run_end,
                        message: format!(
                            "Blockquote has {space_run} spaces after `>` (expected 1)"
                        ),
                    })
                } else {
                    None
                }
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
    use super::M027NoMultipleSpaceBlockquote;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_single_space_after_gt() {
        assert!(M027NoMultipleSpaceBlockquote
            .lint(&f("> quote\n> more\n"))
            .is_empty());
    }
    #[test]
    fn flags_multiple_spaces_after_gt() {
        let v = M027NoMultipleSpaceBlockquote.lint(&f(">  two spaces\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn fix_collapses_to_one_space_and_relints_clean() {
        let src = f(">   three spaces\n");
        let violations = M027NoMultipleSpaceBlockquote.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M027NoMultipleSpaceBlockquote
            .fix(&src, &violations[0])
            .expect("M027 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "> three spaces\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M027NoMultipleSpaceBlockquote.lint(&after).is_empty());
    }
}
