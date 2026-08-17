//! `M031` — fenced code blocks must be surrounded by blank lines.
//! Mirrors MD031.

use crate::{line_byte_range, Rule, SourceFile, Violation};

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

pub struct M031BlanksAroundFences;

impl M031BlanksAroundFences {
    pub const CODE: &'static str = "M031";
}

impl Rule for M031BlanksAroundFences {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let lines: Vec<&str> = file.contents.lines().collect();
        let mut violations = Vec::new();
        let mut inside = false;
        for (idx, line) in lines.iter().enumerate() {
            if !is_fence(line) {
                continue;
            }
            if inside {
                // Closing fence — must have blank line after.
                if let Some(next) = lines.get(idx + 1) {
                    if !next.trim().is_empty() {
                        violations.push(Violation {
                            code: Self::CODE,
                            path: file.path.clone(),
                            line: idx + 1,
                            range: line_byte_range(&file.contents, idx + 1),
                            message: "Fenced code block must have a blank line after it"
                                .to_string(),
                        });
                    }
                }
                inside = false;
            } else {
                // Opening fence — must have blank line before.
                if idx > 0 && !lines[idx - 1].trim().is_empty() {
                    violations.push(Violation {
                        code: Self::CODE,
                        path: file.path.clone(),
                        line: idx + 1,
                        range: line_byte_range(&file.contents, idx + 1),
                        message: "Fenced code block must have a blank line before it".to_string(),
                    });
                }
                inside = true;
            }
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M031BlanksAroundFences;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_with_blank_before_fence() {
        assert!(M031BlanksAroundFences
            .lint(&f("para\n\n```rust\ncode\n```\n"))
            .is_empty());
    }
    #[test]
    fn flags_fence_without_preceding_blank() {
        let v = M031BlanksAroundFences.lint(&f("para\n```rust\ncode\n```\n"));
        assert_eq!(v.len(), 1);
    }
}
