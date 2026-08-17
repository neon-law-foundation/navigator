//! `M019` — ATX heading must have exactly one space after `#`.
//! Mirrors markdownlint MD019.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M019NoMultipleSpaceATX;

impl M019NoMultipleSpaceATX {
    pub const CODE: &'static str = "M019";
}

impl Rule for M019NoMultipleSpaceATX {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        frontmatter::body_lines(&file.contents)
            .into_iter()
            .filter_map(|(line_no, line)| {
                let hashes = line.bytes().take_while(|&b| b == b'#').count();
                if hashes == 0 || hashes > 6 {
                    return None;
                }
                let after = &line.as_bytes()[hashes..];
                let space_run = after.iter().take_while(|&&b| b == b' ').count();
                if space_run <= 1 {
                    return None;
                }
                let line_range = line_byte_range(&file.contents, line_no);
                let run_start = line_range.start + hashes;
                Some(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: run_start..run_start + space_run,
                    message: "ATX heading must have exactly one space after `#`".to_string(),
                })
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
    use super::M019NoMultipleSpaceATX;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_single_space() {
        assert!(M019NoMultipleSpaceATX
            .lint(&f("# Title\n## Sub\n"))
            .is_empty());
    }
    #[test]
    fn flags_multiple_spaces_after_hash() {
        let v = M019NoMultipleSpaceATX.lint(&f("#  Two spaces\n###   Three\nok\n"));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].line, 1);
        assert_eq!(v[1].line, 2);
    }

    #[test]
    fn fix_collapses_to_single_space_and_relints_clean() {
        let src = f("###   Three\n");
        let violations = M019NoMultipleSpaceATX.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M019NoMultipleSpaceATX
            .fix(&src, &violations[0])
            .expect("M019 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "### Three\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M019NoMultipleSpaceATX.lint(&after).is_empty());
    }
}
