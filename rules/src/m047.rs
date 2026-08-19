//! `M047` — files must end with exactly one trailing newline.
//!
//! Mirrors `markdownlint`'s MD047 (single-trailing-newline).

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M047SingleTrailingNewline;

impl M047SingleTrailingNewline {
    pub const CODE: &'static str = "M047";
}

/// The line terminator this document uses, taken from its first one.
/// A Windows checkout materialises these files with CRLF, and a fix
/// that appended a bare `\n` would leave one lone LF in an otherwise
/// CRLF file. A document with no terminator at all takes `\n`.
fn terminator(contents: &str) -> &'static str {
    match contents.find('\n') {
        Some(end) if contents[..end].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// `contents` with exactly one trailing terminator removed, or `None`
/// when it does not end with one.
fn strip_one_terminator(contents: &str) -> Option<&str> {
    let stripped = contents.strip_suffix('\n')?;
    Some(stripped.strip_suffix('\r').unwrap_or(stripped))
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
        // Peel one terminator and ask whether another is under it,
        // rather than matching a byte pair: `ends_with("\n\n")` is
        // false for the `\r\n\r\n` a Windows checkout produces, which
        // left the rule silent on exactly the files it should flag.
        let message = if !contents.ends_with('\n') {
            "File must end with a newline"
        } else if strip_one_terminator(contents).is_some_and(|c| c.ends_with('\n')) {
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
        // Normalize the end-of-file to exactly one terminator: replace
        // the whole trailing run with a single one. Covers both
        // "missing" (empty run → insert) and "too many" (collapse to
        // one). The replacement is the terminator the document already
        // uses, so fixing a CRLF file does not leave a lone LF behind.
        let trimmed_len = contents.trim_end_matches(['\n', '\r']).len();
        let eol = terminator(contents);
        // Skip a no-op edit when the file already ends with exactly one
        // terminator, consistent with the other `fix()` impls in this crate.
        (&contents[trimmed_len..] != eol).then_some(TextEdit {
            range: trimmed_len..contents.len(),
            new_text: eol.to_string(),
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

    /// A Windows checkout materialises these files with CRLF. The rule
    /// must reach the same verdict on a document and its CRLF twin —
    /// `ends_with("\n\n")` did not, because `\r\n\r\n` does not match
    /// it, so the rule went silent on precisely the files it should
    /// flag. Asserting counts across the twins is the only shape that
    /// catches a rule that simply stops reporting.
    #[test]
    fn reaches_the_same_verdict_on_a_crlf_twin() {
        for body in ["hello", "hello\n", "hello\n\n", "hello\n\n\n\n"] {
            let crlf = body.replace('\n', "\r\n");
            assert_eq!(
                M047SingleTrailingNewline.lint(&file(body)).len(),
                M047SingleTrailingNewline.lint(&file(&crlf)).len(),
                "LF and CRLF disagree on {body:?}"
            );
        }
    }

    #[test]
    fn fix_preserves_crlf_rather_than_injecting_a_lone_lf() {
        // Missing terminator: the appended one matches the document.
        assert_eq!(fixed("line1\r\nline2"), "line1\r\nline2\r\n");
        // Surplus terminators collapse to one CRLF, not to one LF.
        assert_eq!(fixed("hello\r\n\r\n"), "hello\r\n");
        assert_eq!(fixed("hello\r\n\r\n\r\n\r\n"), "hello\r\n");
    }

    #[test]
    fn fix_on_a_crlf_twin_matches_the_lf_original() {
        // Every body must carry a terminator: a document with none is
        // its own CRLF twin, so it proves nothing here. The no-newline
        // case is covered by `fix_appends_a_missing_newline`.
        for body in ["hello\n\n", "hello\n\n\n\n", "line1\nline2"] {
            let crlf = body.replace('\n', "\r\n");
            assert_eq!(
                fixed(body).replace('\n', "\r\n"),
                fixed(&crlf),
                "LF and CRLF fixes diverge on {body:?}"
            );
        }
    }

    #[test]
    fn a_single_line_crlf_file_takes_crlf_not_lf() {
        // No interior terminator to learn from, so a bare `\n` is the
        // documented fallback — but a file whose only terminator is a
        // CRLF must keep it.
        assert_eq!(fixed("hello\r\n\r\n"), "hello\r\n");
        assert_eq!(fixed("hello"), "hello\n");
    }
}
