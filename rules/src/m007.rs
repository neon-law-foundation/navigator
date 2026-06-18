//! `M007` — UL items nested under UL items must indent by 2 or
//! more spaces. Mirrors MD007.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M007ULIndent;

impl M007ULIndent {
    pub const CODE: &'static str = "M007";
}

fn ul_indent(line: &str) -> Option<usize> {
    let lead = line.bytes().take_while(|&b| b == b' ').count();
    let after = &line[lead..];
    if matches!(after.as_bytes().first(), Some(b'*' | b'-' | b'+'))
        && after.as_bytes().get(1) == Some(&b' ')
    {
        Some(lead)
    } else {
        None
    }
}

impl Rule for M007ULIndent {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut prev_indent: Option<usize> = None;
        for (idx, line) in file.contents.lines().enumerate() {
            let Some(indent) = ul_indent(line) else {
                continue;
            };
            if let Some(prev) = prev_indent {
                if indent > prev && indent < prev + 2 {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: "Nested UL must indent by at least 2 spaces".to_string(),
                    });
                }
            }
            prev_indent = Some(indent);
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M007ULIndent;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_2space_nested_ul() {
        assert!(M007ULIndent.lint(&f("- a\n  - b\n")).is_empty());
    }
    #[test]
    fn flags_1space_nested_ul() {
        let v = M007ULIndent.lint(&f("- a\n - b\n"));
        assert_eq!(v.len(), 1);
    }
}
