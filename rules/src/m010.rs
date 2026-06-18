//! `M010` — lines must not contain hard tab characters. Mirrors
//! markdownlint MD010 (no-hard-tabs).

use crate::{line_byte_range, Rule, SourceFile, TextEdit, Violation};

pub struct M010NoHardTabs;

impl M010NoHardTabs {
    pub const CODE: &'static str = "M010";
    /// Spaces substituted for every tab — matches M007's
    /// two-space unordered-list indentation.
    pub const SPACES_PER_TAB: usize = 2;
}

impl Rule for M010NoHardTabs {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        file.contents
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains('\t'))
            .map(|(idx, _)| Violation {
                code: Self::CODE,
                path: file.path.clone(),
                line: idx + 1,
                range: line_byte_range(&file.contents, idx + 1),
                message: "Line contains a hard tab character".to_string(),
            })
            .collect()
    }

    fn fix(&self, file: &SourceFile, violation: &Violation) -> Option<TextEdit> {
        let line_text = file.contents.get(violation.range.clone())?;
        let replaced = line_text.replace('\t', &" ".repeat(Self::SPACES_PER_TAB));
        Some(TextEdit {
            range: violation.range.clone(),
            new_text: replaced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::M010NoHardTabs;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;

    fn file(body: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: body.to_string(),
        }
    }

    #[test]
    fn passes_when_no_tab_characters_present() {
        assert!(M010NoHardTabs
            .lint(&file("ok\n    indented spaces\n"))
            .is_empty());
    }

    #[test]
    fn flags_each_line_containing_a_tab() {
        let v = M010NoHardTabs.lint(&file("ok\n\tone tab\n\t\ttwo tabs\nok\n"));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].line, 2);
        assert_eq!(v[1].line, 3);
        assert_eq!(v[0].code, "M010");
    }

    #[test]
    fn fix_replaces_tabs_with_two_spaces_and_relint_is_clean() {
        let src = file("ok\n\tone tab\n");
        let violations = M010NoHardTabs.lint(&src);
        assert_eq!(violations.len(), 1);
        let edit = M010NoHardTabs
            .fix(&src, &violations[0])
            .expect("M010 must autofix");
        let mut fixed = src.contents.clone();
        fixed.replace_range(edit.range.clone(), &edit.new_text);
        assert_eq!(fixed, "ok\n  one tab\n");
        let after = SourceFile {
            path: src.path.clone(),
            contents: fixed,
        };
        assert!(M010NoHardTabs.lint(&after).is_empty());
    }
}
