//! `M004` — unordered list bullet style must be consistent.
//! Mirrors MD004.

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

pub struct M004ULStyle;

impl M004ULStyle {
    pub const CODE: &'static str = "M004";
}

impl Rule for M004ULStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut established: Option<char> = None;
        let mut violations = Vec::new();
        for (line_no, line) in frontmatter::body_lines(&file.contents) {
            let trimmed = line.trim_start();
            let Some(first) = trimmed.chars().next() else {
                continue;
            };
            if !matches!(first, '*' | '-' | '+') {
                continue;
            }
            if trimmed.as_bytes().get(1) != Some(&b' ') {
                continue;
            }
            match established {
                None => established = Some(first),
                Some(c) if c != first => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: format!(
                            "UL bullet `{first}` differs from established style `{c}`"
                        ),
                    });
                }
                _ => {}
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M004ULStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_bullets() {
        assert!(M004ULStyle.lint(&f("- a\n- b\n- c\n")).is_empty());
    }
    #[test]
    fn flags_mixed_bullets() {
        let v = M004ULStyle.lint(&f("- a\n* b\n+ c\n"));
        assert_eq!(v.len(), 2);
    }
}
