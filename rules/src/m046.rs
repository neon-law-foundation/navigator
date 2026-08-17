//! `M046` — code block style must be consistent (fenced vs.
//! indented). Mirrors MD046.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M046CodeBlockStyle;

impl M046CodeBlockStyle {
    pub const CODE: &'static str = "M046";
}

fn classify(line: &str, in_paragraph: bool) -> Option<&'static str> {
    let t = line.trim_start();
    if t.starts_with("```") || t.starts_with("~~~") {
        return Some("fenced");
    }
    if line.starts_with("    ") && !in_paragraph {
        return Some("indented");
    }
    None
}

impl Rule for M046CodeBlockStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut established: Option<&'static str> = None;
        let mut violations = Vec::new();
        let mut in_paragraph = false;
        let mut in_fence = false;
        for (idx, line) in file.contents.lines().enumerate() {
            let trimmed = line.trim();
            if in_fence {
                if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                    in_fence = false;
                }
                continue;
            }
            let Some(style) = classify(line, in_paragraph) else {
                in_paragraph = !trimmed.is_empty();
                continue;
            };
            if style == "fenced" {
                in_fence = true;
            }
            match established {
                None => established = Some(style),
                Some(prev) if prev != style => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!(
                            "Code block style `{style}` differs from established `{prev}`"
                        ),
                    });
                }
                _ => {}
            }
            in_paragraph = !trimmed.is_empty();
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M046CodeBlockStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_only_fenced_blocks() {
        assert!(M046CodeBlockStyle
            .lint(&f("```rust\ncode\n```\n\n```rust\nmore\n```\n"))
            .is_empty());
    }
    #[test]
    fn flags_mix_of_fenced_and_indented() {
        let v = M046CodeBlockStyle.lint(&f("```rust\ncode\n```\n\n    indented\n"));
        assert_eq!(v.len(), 1);
    }
}
