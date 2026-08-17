//! `M018` — ATX heading must have a space after the `#` marks.
//! Mirrors markdownlint MD018 (no-missing-space-atx).

use crate::{frontmatter, line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M018NoMissingSpaceATX;

impl M018NoMissingSpaceATX {
    pub const CODE: &'static str = "M018";
}

impl Rule for M018NoMissingSpaceATX {
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
                if after.is_empty() || after[0] == b' ' || after[0] == b'\t' {
                    return None;
                }
                // Narrow range to the byte immediately after the last
                // `#` — `fix()` inserts a space at that position.
                let line_range = line_byte_range(&file.contents, line_no);
                let insert_at = line_range.start + hashes;
                Some(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: line_no,
                    range: insert_at..insert_at,
                    message: "ATX heading must have a space after `#`".to_string(),
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
    use super::M018NoMissingSpaceATX;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_space_after_hash() {
        assert!(M018NoMissingSpaceATX
            .lint(&f("# Title\n## Sub\n"))
            .is_empty());
    }
    #[test]
    fn flags_missing_space() {
        let v = M018NoMissingSpaceATX.lint(&f("#Title\n##Sub\nok\n"));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].line, 1);
        assert_eq!(v[1].line, 2);
    }

    #[test]
    fn fix_inserts_space_after_hashes_and_relints_clean() {
        let src = f("#Title\n");
        let violations = M018NoMissingSpaceATX.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M018NoMissingSpaceATX
            .fix(&src, &violations[0])
            .expect("M018 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "# Title\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M018NoMissingSpaceATX.lint(&after).is_empty());
    }
}
