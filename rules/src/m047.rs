//! `M047` — files must end with exactly one trailing newline.
//!
//! Mirrors `markdownlint`'s MD047 (single-trailing-newline).

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

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

    fn fix(&self, file: &SourceFile, _violation: &Violation) -> Option<TextEdit> {
        let contents = &file.contents;
        if contents.is_empty() {
            return None;
        }
        // Normalize the end-of-file to exactly one `\n`: replace the run
        // of trailing newlines with a single one. Covers both "missing"
        // (empty run → insert `\n`) and "too many" (collapse to one).
        let trimmed_len = contents.trim_end_matches('\n').len();
        // Skip a no-op edit when the file already ends with exactly one
        // `\n`, consistent with the other `fix()` impls in this crate.
        (&contents[trimmed_len..] != "\n").then_some(TextEdit {
            range: trimmed_len..contents.len(),
            new_text: "\n".to_string(),
        })
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

    /// Apply the rule's single fix and return the resulting contents.
    fn fixed(body: &str) -> String {
        let f = file(body);
        let v = M047SingleTrailingNewline.lint(&f);
        let edit = M047SingleTrailingNewline.fix(&f, &v[0]).expect("a fix");
        let mut out = f.contents.clone();
        out.replace_range(edit.range, &edit.new_text);
        out
    }

    #[test]
    fn fix_appends_a_missing_newline() {
        assert_eq!(fixed("hello"), "hello\n");
        assert_eq!(fixed("line1\nline2"), "line1\nline2\n");
    }

    #[test]
    fn fix_collapses_extra_trailing_newlines() {
        assert_eq!(fixed("hello\n\n"), "hello\n");
        assert_eq!(fixed("hello\n\n\n\n"), "hello\n");
    }

    #[test]
    fn fix_is_idempotent() {
        // A once-fixed file lints clean, so there is nothing left to fix.
        let once = fixed("hello\n\n\n");
        assert!(M047SingleTrailingNewline.lint(&file(&once)).is_empty());
    }

    #[test]
    fn fix_declines_a_noop_when_already_correct() {
        // lint() produces no violation here, but a caller invoking fix()
        // directly must get None rather than a redundant `\n` → `\n` edit,
        // matching the guard on the other fix() impls in this crate.
        let f = file("hello\n");
        let v = crate::Violation {
            code: M047SingleTrailingNewline::CODE,
            path: f.path.clone(),
            line: 1,
            range: 0..f.contents.len(),
            message: "unused".to_string(),
        };
        assert!(M047SingleTrailingNewline.fix(&f, &v).is_none());
    }
}
