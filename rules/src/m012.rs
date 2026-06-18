//! `M012` — no more than one consecutive blank line. Mirrors
//! markdownlint MD012 (no-multiple-blanks).

use crate::{Rule, SourceFile, TextEdit, Violation};

pub struct M012NoMultipleBlanks;

impl M012NoMultipleBlanks {
    pub const CODE: &'static str = "M012";
}

impl Rule for M012NoMultipleBlanks {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut offset = 0usize;
        let mut blank_run = 0usize;
        for (idx, segment) in file.contents.split_inclusive('\n').enumerate() {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run == 2 {
                    // Report on the second blank line. Range covers
                    // this blank's bytes (including its newline) so
                    // `fix()` can delete the surplus.
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: offset..offset + segment.len(),
                        message: "More than one consecutive blank line".to_string(),
                    });
                } else if blank_run > 2 {
                    // Extend the existing range to swallow this extra
                    // blank too.
                    if let Some(v) = violations.last_mut() {
                        v.range.end = offset + segment.len();
                    }
                }
            } else {
                blank_run = 0;
            }
            offset += segment.len();
        }
        violations
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
    use super::M012NoMultipleBlanks;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn f(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_with_single_blank_line_between_paragraphs() {
        assert!(M012NoMultipleBlanks
            .lint(&f("para 1\n\npara 2\n\npara 3\n"))
            .is_empty());
    }

    #[test]
    fn flags_two_consecutive_blank_lines() {
        let v = M012NoMultipleBlanks.lint(&f("a\n\n\nb\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn reports_once_per_run_not_per_extra_blank() {
        let v = M012NoMultipleBlanks.lint(&f("a\n\n\n\n\nb\n"));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn fix_collapses_long_blank_runs_to_a_single_blank() {
        let src = f("a\n\n\n\n\nb\n");
        let violations = M012NoMultipleBlanks.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M012NoMultipleBlanks
            .fix(&src, &violations[0])
            .expect("M012 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "a\n\nb\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M012NoMultipleBlanks.lint(&after).is_empty());
    }
}
