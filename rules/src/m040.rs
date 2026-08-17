//! `M040` — fenced code blocks must declare a language tag.
//! Mirrors markdownlint MD040.

use crate::{line_byte_range, Rule, SourceFile, Violation};

pub struct M040FencedCodeLanguage;

impl M040FencedCodeLanguage {
    pub const CODE: &'static str = "M040";
}

impl Rule for M040FencedCodeLanguage {
    fn code(&self) -> &'static str {
        Self::CODE
    }

    fn lint(&self, file: &SourceFile) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut inside = false;
        for (idx, line) in file.contents.lines().enumerate() {
            let trimmed = line.trim_start();
            let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
            if !fence {
                continue;
            }
            if inside {
                // This is the closing fence — no language tag needed.
                inside = false;
                continue;
            }
            // Opening fence — check for language after the marker.
            let marker = if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            };
            let after = trimmed[marker.len()..].trim();
            if after.is_empty() {
                violations.push(Violation {
                    code: Self::CODE,
                    path: file.path.clone(),
                    line: idx + 1,
                    range: line_byte_range(&file.contents, idx + 1),
                    message: "Fenced code block is missing a language tag".to_string(),
                });
            }
            inside = true;
        }
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::M040FencedCodeLanguage;
    use crate::{Rule, SourceFile};
    use std::path::PathBuf;
    fn f(b: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from("t.md"),
            contents: b.to_string(),
        }
    }
    #[test]
    fn passes_when_fences_have_languages() {
        assert!(M040FencedCodeLanguage
            .lint(&f("```rust\nlet x = 1;\n```\n"))
            .is_empty());
    }
    #[test]
    fn flags_fence_without_language() {
        let v = M040FencedCodeLanguage.lint(&f("```\ncode\n```\n"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }
}
