//! `M048` — fenced code blocks must use a consistent fence marker
//! (all backtick or all tilde). Mirrors MD048.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M048CodeFenceStyle;

impl M048CodeFenceStyle {
    pub const CODE: &'static str = "M048";
}

fn fence_marker(line: &str) -> Option<char> {
    let t = line.trim_start();
    if t.starts_with("```") {
        Some('`')
    } else if t.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

impl Rule for M048CodeFenceStyle {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut expected: Option<char> = None;
        let mut in_fence = false;
        let mut fence_char: Option<char> = None;
        for (idx, line) in file.contents.lines().enumerate() {
            let Some(marker) = fence_marker(line) else {
                continue;
            };
            if in_fence {
                if Some(marker) == fence_char {
                    in_fence = false;
                    fence_char = None;
                }
                continue;
            }
            in_fence = true;
            fence_char = Some(marker);
            match expected {
                None => expected = Some(marker),
                Some(prev) if prev != marker => {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: format!(
                            "Fence marker `{marker}` does not match first-use marker `{prev}`"
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
    use super::M048CodeFenceStyle;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_consistent_backtick_fences() {
        assert!(M048CodeFenceStyle
            .lint(&f("```rust\na\n```\n\n```rust\nb\n```\n"))
            .is_empty());
    }
    #[test]
    fn flags_tilde_fence_after_backtick_fence() {
        let v = M048CodeFenceStyle.lint(&f("```\na\n```\n\n~~~\nb\n~~~\n"));
        assert_eq!(v.len(), 1);
    }
}
