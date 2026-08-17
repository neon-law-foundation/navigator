//! `M032` — lists must be surrounded by blank lines. MD032.

use std::collections::HashSet;

use crate::{frontmatter, line_byte_range, Rule, SourceFile, Violation};

/// True only when `line` is a top-level (column-0) list marker. An
/// indented list — the inner part of a numbered or bulleted continuation
/// inside a paragraph — is treated as paragraph text by markdownlint
/// MD032 and not subject to the surrounding-blank requirement.
fn is_top_level_list_marker(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    if let Some(c) = line.chars().next() {
        if matches!(c, '*' | '-' | '+') && line.as_bytes().get(1) == Some(&b' ') {
            return true;
        }
        if c.is_ascii_digit() {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            return matches!(line.as_bytes().get(digits), Some(b'.' | b')'))
                && line.as_bytes().get(digits + 1) == Some(&b' ');
        }
    }
    false
}

pub struct M032BlanksAroundLists;

impl M032BlanksAroundLists {
    pub const CODE: &'static str = "M032";
}

impl Rule for M032BlanksAroundLists {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        let body: HashSet<usize> = frontmatter::body_lines(&file.contents)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let mut violations = Vec::new();
        let mut in_list = false;
        for (idx, line) in lines.iter().enumerate() {
            let line_no = idx + 1;
            if !body.contains(&line_no) {
                continue;
            }
            let is_item = is_top_level_list_marker(line);
            let is_blank = line.trim().is_empty();
            if is_item && !in_list {
                in_list = true;
                if idx > 0 && !lines[idx - 1].trim().is_empty() {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: line_no,
                        range: line_byte_range(&file.contents, line_no),
                        message: "List must have a blank line before it".to_string(),
                    });
                }
            } else if in_list && !is_item && !is_blank && !line.starts_with(' ') {
                in_list = false;
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M032BlanksAroundLists;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_blank_before_list() {
        assert!(M032BlanksAroundLists
            .lint(&f("para\n\n- a\n- b\n"))
            .is_empty());
    }
    #[test]
    fn flags_list_without_blank_before() {
        let v = M032BlanksAroundLists.lint(&f("para\n- a\n- b\n"));
        assert_eq!(v.len(), 1);
    }
}
