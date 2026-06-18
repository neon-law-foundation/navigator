//! `M005` — list indent must be consistent. Mirrors MD005.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M005ListIndent;

impl M005ListIndent {
    pub const CODE: &'static str = "M005";
}

fn list_marker_indent(line: &str) -> Option<usize> {
    let lead_spaces = line.bytes().take_while(|&b| b == b' ').count();
    let after = &line[lead_spaces..];
    let bytes = after.as_bytes();
    if matches!(bytes.first(), Some(b'*' | b'-' | b'+')) && bytes.get(1) == Some(&b' ') {
        return Some(lead_spaces);
    }
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        let digits = bytes.iter().take_while(|&&b| b.is_ascii_digit()).count();
        if matches!(bytes.get(digits), Some(b'.' | b')')) && bytes.get(digits + 1) == Some(&b' ') {
            return Some(lead_spaces);
        }
    }
    None
}

impl Rule for M005ListIndent {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        // Group consecutive list items by indent level; each
        // depth-level should be a stable indent.
        let mut levels: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut violations = Vec::new();
        for (idx, line) in file.contents.lines().enumerate() {
            let Some(indent) = list_marker_indent(line) else {
                continue;
            };
            // Bucket by "level" = indent / 2 (rough heuristic).
            let level = indent / 2;
            match levels.get(&level) {
                None => {
                    levels.insert(level, indent);
                }
                Some(&prev) if prev != indent => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!(
                            "List item indent {indent} differs from previous level-{level} indent {prev}"
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
    use super::M005ListIndent;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_indent() {
        assert!(M005ListIndent
            .lint(&f("- a\n  - b\n  - c\n- d\n"))
            .is_empty());
    }
    #[test]
    fn flags_inconsistent_indent_at_same_level() {
        let v = M005ListIndent.lint(&f("- a\n  - b\n    - c\n   - d\n"));
        assert!(!v.is_empty());
    }
}
