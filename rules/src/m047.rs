//! `M047` — files must end with exactly one trailing newline.
//!
//! Mirrors `markdownlint`'s MD047 (single-trailing-newline).

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M047SingleTrailingNewline;

impl M047SingleTrailingNewline {
    pub const CODE: &'static str = "M047";
}

impl Rule for M047SingleTrailingNewline {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let contents = &file.contents;
        // Empty files are exempt.
        if contents.is_empty() {
            return Vec::new();
        }
        let message = if !contents.ends_with('\n') {
            "File must end with a newline"
        } else if contents.ends_with("\n\n") {
            "File must end with exactly one newline"
        } else {
            return Vec::new();
        };
        // Line number = total line count.
        let line = contents.lines().count().max(1);
        vec![Violation {
            code: Self::CODE,
            path: file.path.clone(),
            line,
            range: line_byte_range(&file.contents, line),
            message: message.to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::M047SingleTrailingNewline;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("test.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_exactly_one_trailing_newline() {
        assert!(M047SingleTrailingNewline.lint(&file("hello\n")).is_empty());
        assert!(M047SingleTrailingNewline
            .lint(&file("line1\nline2\n"))
            .is_empty());
    }

    #[test]
    fn flags_missing_trailing_newline() {
        let v = M047SingleTrailingNewline.lint(&file("hello"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "M047");
        assert!(v[0].message.contains("must end with a newline"));
    }

    #[test]
    fn flags_multiple_trailing_newlines() {
        let v = M047SingleTrailingNewline.lint(&file("hello\n\n"));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exactly one newline"));
    }

    #[test]
    fn flags_many_trailing_newlines() {
        let v = M047SingleTrailingNewline.lint(&file("hello\n\n\n\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn empty_file_is_exempt() {
        assert!(M047SingleTrailingNewline.lint(&file("")).is_empty());
    }
}
