//! `M028` — blockquotes must not have blank lines inside. MD028.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M028NoBlanksBlockquote;

impl M028NoBlanksBlockquote {
    pub const CODE: &'static str = "M028";
}

impl Rule for M028NoBlanksBlockquote {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        let mut violations = Vec::new();
        for i in 0..lines.len().saturating_sub(2) {
            let before = lines[i].trim_start();
            let blank = lines[i + 1].trim();
            let after = lines[i + 2].trim_start();
            if before.starts_with('>') && blank.is_empty() && after.starts_with('>') {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: i + 2,
                    range: line_byte_range(&file.contents, i + 2),
                    message: "Blank line inside a blockquote".to_string(),
                });
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M028NoBlanksBlockquote;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_contiguous_blockquote() {
        assert!(M028NoBlanksBlockquote
            .lint(&f("> a\n> b\n> c\n"))
            .is_empty());
    }
    #[test]
    fn flags_blank_line_inside_blockquote() {
        let v = M028NoBlanksBlockquote.lint(&f("> a\n\n> b\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
    }
}
